//! Shard's window, drawn as an instrument panel: a command bar in the title bar, a hero card with
//! total speed, the live connection graph and the throughput chart, the queue as cards with a
//! detail panel beside it, a stats view, and a limits panel. It only talks to the queue through
//! `Manager`'s public API, polling `rows()` and `throughput()` twice a second, and eases what it
//! draws toward each new poll.
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    Icon, Sizable as _, Theme, ThemeMode, TitleBar, h_flex,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    scroll::ScrollableElement as _,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::queue::{Id, Manager, Move, NetworkUse, Row, Segment, Settings, Status, WhenDone};

actions!(
    shard,
    [
        Quit,
        FocusCommand,
        ToggleLimits,
        PasteLink,
        TogglePause,
        SelectPrev,
        SelectNext,
        RemoveSelected,
        OpenSelected,
        Confirm,
        Dismiss,
    ]
);

// Lucide icons this window uses that are not in gpui-kit's default icon bundle.
gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        Check,
        CircleAlert,
        Copy,
        ExternalLink,
        FolderOpen,
        Globe,
        Link,
        Pause,
        Play,
        Plus,
        Power,
        AppWindow,
        RotateCcw,
        SlidersHorizontal,
        Trash,
        X,
    ]
);

/// The extra icons first, then gpui-kit's defaults (which the components themselves use).
struct IdmAssets;

impl AssetSource for IdmAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = ExtraIcons.list(path)?;
        all.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(all)
    }
}

const FONTS: [&[u8]; 7] = [
    include_bytes!("../assets/fonts/Geist-Regular.ttf"),
    include_bytes!("../assets/fonts/Geist-Medium.ttf"),
    include_bytes!("../assets/fonts/Geist-SemiBold.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Regular.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Medium.ttf"),
    include_bytes!("../assets/fonts/ArchivoExpanded-Bold.ttf"),
    include_bytes!("../assets/fonts/ArchivoExpanded-ExtraBold.ttf"),
];
const SANS: &str = "Geist";
const MONO: &str = "Geist Mono";
const DISPLAY: &str = "Archivo Expanded";

/// The palette. One accent, used sparingly; near-white for a second series; red only for errors.
mod ink {
    use gpui_kit::{Hsla, hsla, rgb};

    pub fn bg() -> Hsla {
        rgb(0x0a0a0b).into()
    }
    pub fn panel() -> Hsla {
        rgb(0x101012).into()
    }
    pub fn card() -> Hsla {
        rgb(0x161618).into()
    }
    pub fn inset() -> Hsla {
        rgb(0x1e1e21).into()
    }
    pub fn line() -> Hsla {
        hsla(0., 0., 1., 0.06)
    }
    pub fn line_strong() -> Hsla {
        rgb(0x2c2c30).into()
    }
    pub fn text() -> Hsla {
        rgb(0xededef).into()
    }
    pub fn muted() -> Hsla {
        rgb(0xa1a1a8).into()
    }
    pub fn faint() -> Hsla {
        rgb(0x6c6c74).into()
    }
    pub fn accent() -> Hsla {
        rgb(0xa3e635).into()
    }
    pub fn accent2() -> Hsla {
        rgb(0xd9f99d).into()
    }
    pub fn white2() -> Hsla {
        rgb(0xe6e6e6).into()
    }
    pub fn red() -> Hsla {
        rgb(0xf0554a).into()
    }
}

/// Start the GPUI app: the main window (unless `hidden`) and the tray; returns when the app quits.
///
/// On macOS GPUI quits through `[NSApp terminate:]`, which ends the process without returning.
/// With `SHARD_NO_FOCUS=1` the window opens without activating the app or taking focus.
pub fn run(manager: Arc<Manager>, updater: Arc<crate::update::Updater>, hidden: bool) {
    let no_focus = std::env::var_os("SHARD_NO_FOCUS").is_some_and(|v| v == "1");
    let app = gpui_kit::application().with_assets(IdmAssets).with_quit_mode(QuitMode::Explicit);
    // Launched again from Finder, Spotlight or the Dock while running: show the window.
    app.on_reopen(|cx| {
        if cx.has_global::<Shell>() {
            open_main(true, cx);
        }
    });
    app.run(move |cx: &mut App| {
            gpui_kit::init(cx);
            if let Err(err) = cx
                .text_system()
                .add_fonts(FONTS.map(Cow::Borrowed).to_vec())
            {
                eprintln!("shard: couldn't load the bundled fonts: {err:#}");
            }
            apply_theme(cx);

            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.bind_keys([
                KeyBinding::new("secondary-q", Quit, None),
                KeyBinding::new("secondary-l", FocusCommand, Some(CONTEXT)),
                KeyBinding::new("secondary-n", FocusCommand, Some(CONTEXT)),
                KeyBinding::new("secondary-,", ToggleLimits, Some(CONTEXT)),
                KeyBinding::new("secondary-v", PasteLink, Some(LIST)),
                KeyBinding::new("space", TogglePause, Some(LIST)),
                KeyBinding::new("up", SelectPrev, Some(LIST)),
                KeyBinding::new("down", SelectNext, Some(LIST)),
                KeyBinding::new("delete", RemoveSelected, Some(LIST)),
                KeyBinding::new("backspace", RemoveSelected, Some(LIST)),
                KeyBinding::new("enter", OpenSelected, Some(LIST)),
                KeyBinding::new("enter", Confirm, Some(MODAL)),
                KeyBinding::new("escape", Dismiss, Some(MODAL)),
                KeyBinding::new("escape", Dismiss, Some(PANEL)),
                KeyBinding::new("escape", Dismiss, Some(TRAY)),
            ]);
            cx.set_menus(vec![
                Menu::new("Shard").items([MenuItem::action("Quit Shard", Quit)]),
            ]);

            cx.set_global(Shell {
                manager: manager.clone(),
                main: None,
                panel: None,
                select: None,
                has_tray: true,
                docked: true,
                power: None,
                clip: None,
                updater,
            });
            cx.set_app_identity("dev.shard.app", "Shard");
            cx.on_system_notification_response(notification_response);
            if !hidden {
                open_main(!no_focus, cx);
            }
            // With a tray, closing the window leaves Shard running there; without one, it quits.
            let tray = crate::tray::Tray::new();
            if tray.is_none() {
                cx.global_mut::<Shell>().has_tray = false;
                cx.set_quit_mode(QuitMode::LastWindowClosed);
                if hidden {
                    open_main(!no_focus, cx); // no tray to come back from
                }
            } else if hidden {
                dock(false, cx);
            }
            run_shell(tray, cx);
        });
}

fn main_window_options(focus: bool, cx: &mut App) -> WindowOptions {
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("Shard".into()),
            traffic_light_position: Some(point(px(16.), px(19.))),
            ..TitleBar::title_bar_options()
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1200.), px(760.)),
            cx,
        ))),
        window_min_size: Some(size(px(1040.), px(640.))),
        focus,
        window_background: WindowBackgroundAppearance::Opaque,
        ..TitleBar::window_options()
    }
}

/// Force the dark theme and retune the components' colours to the palette.
fn apply_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |t| {
        t.font_family = SANS.into();
        t.mono_font_family = MONO.into();
        t.radius = px(10.);
        t.radius_lg = px(16.);
        t.shadow = false;
        let c = &mut t.colors;
        c.background = ink::bg();
        c.foreground = ink::text();
        c.muted = ink::inset();
        c.muted_foreground = ink::muted();
        c.border = ink::line_strong();
        c.input = ink::line_strong();
        c.ring = ink::accent().opacity(0.55);
        c.caret = ink::accent2();
        c.selection = ink::accent().opacity(0.32);
        c.primary = ink::accent();
        c.primary_hover = ink::accent2();
        c.primary_active = ink::accent();
        c.primary_foreground = ink::bg(); // dark text: white on lime doesn't read
        c.secondary = ink::inset();
        c.secondary_hover = ink::line_strong();
        c.secondary_active = ink::line_strong();
        c.secondary_foreground = ink::text();
        c.accent = ink::inset();
        c.accent_foreground = ink::text();
        c.popover = ink::card();
        c.popover_foreground = ink::text();
        c.danger = ink::red();
        c.title_bar = ink::bg();
        c.title_bar_border = ink::line();
        c.window_border = ink::line();
        c.overlay = hsla(0., 0., 0., 0.6);
        c.scrollbar = hsla(0., 0., 0., 0.);
        c.scrollbar_thumb = hsla(0., 0., 1., 0.1);
        c.scrollbar_thumb_hover = hsla(0., 0., 1., 0.18);
        c.list_active = ink::inset();
        c.list_hover = ink::card();
    });
}

const CONTEXT: &str = "Idm";
const LIST: &str = "IdmList";
const MODAL: &str = "IdmModal";
const PANEL: &str = "IdmPanel";
const TRAY: &str = "ShardTray";
const POLL: Duration = Duration::from_millis(400);
/// How long drawn values take to ease to a new poll.
const EASE: f32 = 0.4;
const DETAIL_WIDTH: f32 = 380.;
const CARD_HEIGHT: f32 = 72.;
const CARD_GAP: f32 = 8.;
const HERO_HEIGHT: f32 = 182.;
const GRAPH_HEIGHT: f32 = 128.;
/// Nodes that fit the graph at NODE_PITCH apart.
const MAX_NODES: usize = ((GRAPH_HEIGHT - 18.) / NODE_PITCH) as usize + 1;
const NODE_PITCH: f32 = 18.;
const CAP_MAX: u64 = 50 * MB;
/// Polls of summed connections kept for the stats chart (2 minutes), and the least it spans.
const CONN_SLOTS: usize = 300;
const CONN_MIN_SLOTS: usize = 75;
const MB: u64 = 1024 * 1024;

// ---------------------------------------------------------------------------------------------
// Filters

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StatusFilter {
    All,
    Active,
    Queued,
    Paused,
    Done,
    Failed,
}

impl StatusFilter {
    const ALL: [StatusFilter; 6] = [
        StatusFilter::All,
        StatusFilter::Active,
        StatusFilter::Queued,
        StatusFilter::Paused,
        StatusFilter::Done,
        StatusFilter::Failed,
    ];

    fn matches(self, row: &Row) -> bool {
        match self {
            StatusFilter::All => true,
            StatusFilter::Active => row.status == Status::Downloading,
            StatusFilter::Queued => row.status == Status::Queued,
            StatusFilter::Paused => row.status == Status::Paused,
            StatusFilter::Done => row.status == Status::Done,
            StatusFilter::Failed => row.status == Status::Failed,
        }
    }

    fn label(self) -> &'static str {
        match self {
            StatusFilter::All => "ALL",
            StatusFilter::Active => "ACTIVE",
            StatusFilter::Queued => "QUEUED",
            StatusFilter::Paused => "PAUSED",
            StatusFilter::Done => "DONE",
            StatusFilter::Failed => "FAILED",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Category {
    Archives,
    Video,
    Audio,
    Docs,
    Apps,
    Other,
}

impl Category {
    const ALL: [Category; 6] = [
        Category::Archives,
        Category::Video,
        Category::Audio,
        Category::Docs,
        Category::Apps,
        Category::Other,
    ];

    fn of(row: &Row) -> Category {
        match extension(row).as_str() {
            "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "tbz" | "xz" | "txz" | "zst"
            | "lz" | "lzma" | "z" | "cab" | "arj" | "ace" | "sit" | "sitx" | "iso" => {
                Category::Archives
            }
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "pps" | "odt" | "ods"
            | "odp" | "rtf" | "txt" | "md" | "csv" | "epub" | "mobi" | "pages" | "numbers"
            | "key" => Category::Docs,
            "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "oga" | "opus" | "wma" | "aif"
            | "aiff" | "alac" | "mid" | "midi" | "ra" => Category::Audio,
            "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpg" | "mpeg"
            | "mpe" | "ts" | "3gp" | "ogv" | "qt" | "asf" | "rm" => Category::Video,
            "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "apk" | "appimage" | "app" | "jar"
            | "bin" | "run" | "sh" | "msix" | "flatpak" | "snap" => Category::Apps,
            _ => Category::Other,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Category::Archives => "ARCHIVES",
            Category::Video => "VIDEO",
            Category::Audio => "AUDIO",
            Category::Docs => "DOCS",
            Category::Apps => "APPS",
            Category::Other => "OTHER",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Queue,
    Stats,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RowAction {
    Open,
    Reveal,
    Toggle,
    Restart,
    Remove,
    CopyUrl,
    RefreshLink,
    Move(Move),
}

/// How the queue list is ordered.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
    Newest,
    Oldest,
    Name,
    Size,
    Progress,
    /// Waiting downloads in the order they'll start.
    Queue,
}

impl Sort {
    const ALL: [Sort; 6] = [Sort::Newest, Sort::Oldest, Sort::Name, Sort::Size, Sort::Progress, Sort::Queue];

    fn label(self) -> &'static str {
        match self {
            Sort::Newest => "NEWEST",
            Sort::Oldest => "OLDEST",
            Sort::Name => "NAME",
            Sort::Size => "SIZE",
            Sort::Progress => "PROGRESS",
            Sort::Queue => "QUEUE ORDER",
        }
    }

    fn next(self) -> Sort {
        let at = Sort::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Sort::ALL[(at + 1) % Sort::ALL.len()]
    }
}

// ---------------------------------------------------------------------------------------------
// Animation state for the connection graph: one node per unfinished part, easing in, out and
// between positions as parts split and finish.

struct GraphNode {
    key: u32,
    y: f32,
    alpha: f32,
    target_y: f32,
    target_alpha: f32,
    speed: f32,
    target_speed: f32,
    active: bool,
    /// Where the particles on this edge are, in edge lengths (0..2 spans both curves).
    phase: f32,
}

#[derive(Default)]
struct GraphAnim {
    row: Option<Id>,
    nodes: Vec<GraphNode>,
    /// Unfinished parts that didn't fit, shown as one "+N" node in the last slot.
    hidden: usize,
    hidden_y: f32,
}

impl GraphAnim {
    /// Move the nodes one frame toward `row`'s current parts.
    fn step(&mut self, row: Option<&Row>, dt: f32) {
        let id = row.map(|r| r.id);
        if id != self.row {
            self.row = id;
            self.nodes.clear();
        }
        // Targets: unfinished parts, the busy ones first, then idle ones; shown in file order.
        let mut targets: Vec<(u32, bool, f32)> = match row {
            Some(r) if r.parts.is_empty() && r.status == Status::Downloading => {
                vec![(u32::MAX, true, r.speed as f32)]
            }
            Some(r) => r
                .parts
                .iter()
                .filter(|s| s.done < s.end)
                .map(|s| {
                    (
                        s.start.to_bits(),
                        s.active && r.status == Status::Downloading,
                        s.speed as f32,
                    )
                })
                .collect(),
            None => Vec::new(),
        };
        // When they don't all fit, keep the fastest and fold the rest into a "+N" node.
        let unfinished = targets.len();
        if unfinished > MAX_NODES {
            targets.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.total_cmp(&a.2)));
            targets.truncate(MAX_NODES - 1);
        }
        targets.sort_by(|a, b| f32::from_bits(a.0).total_cmp(&f32::from_bits(b.0)));
        self.hidden = unfinished - targets.len();

        // Spread the nodes over the whole height, at least NODE_PITCH apart.
        let n = targets.len() + usize::from(self.hidden > 0);
        let spacing = if n > 1 {
            ((GRAPH_HEIGHT - 18.) / (n - 1) as f32).min(26.)
        } else {
            0.
        };
        let mid = GRAPH_HEIGHT / 2.;
        self.hidden_y = mid + (n as f32 - 1.) / 2. * spacing;
        // Ease at frame rate; when frames are rare (a hidden window) or the graph is new, snap.
        let k = if dt >= 0.1 || self.nodes.is_empty() {
            1.
        } else {
            1. - (-dt / 0.14).exp()
        };
        let max_speed = targets.iter().map(|t| t.2).fold(1., f32::max);

        for node in &mut self.nodes {
            node.target_alpha = 0.;
            node.target_speed = 0.;
        }
        for (i, (key, active, speed)) in targets.iter().enumerate() {
            let y = mid + (i as f32 - (n as f32 - 1.) / 2.) * spacing;
            let alpha = if *active { 1. } else { 0.42 };
            match self.nodes.iter_mut().find(|n| n.key == *key) {
                Some(node) => {
                    node.target_y = y;
                    node.target_alpha = alpha;
                    node.target_speed = *speed;
                    node.active = *active;
                }
                None => {
                    // A new part grows out of the one it was split from (the node above it).
                    let from = self
                        .nodes
                        .iter()
                        .filter(|n| f32::from_bits(n.key) < f32::from_bits(*key))
                        .max_by(|a, b| f32::from_bits(a.key).total_cmp(&f32::from_bits(b.key)))
                        .map_or(y, |n| n.y);
                    self.nodes.push(GraphNode {
                        key: *key,
                        y: from,
                        alpha: if k >= 1. { alpha } else { 0. },
                        target_y: y,
                        target_alpha: alpha,
                        speed: if k >= 1. { *speed } else { 0. },
                        target_speed: *speed,
                        active: *active,
                        phase: (*key as f32 * 0.618).fract() * 2.,
                    });
                }
            }
        }
        for node in &mut self.nodes {
            node.y += (node.target_y - node.y) * k;
            node.alpha += (node.target_alpha - node.alpha) * k;
            node.speed += (node.target_speed - node.speed) * k;
            if node.active {
                let rate = 0.25 + 1.1 * (node.speed / max_speed).clamp(0., 1.);
                node.phase = (node.phase + dt * rate) % 2.;
            }
        }
        self.nodes.retain(|n| n.target_alpha > 0. || n.alpha > 0.02);
        self.nodes.sort_by(|a, b| a.target_y.total_cmp(&b.target_y));
    }

    fn settled(&self) -> bool {
        self.nodes.iter().all(|n| {
            !n.active && (n.y - n.target_y).abs() < 0.3 && (n.alpha - n.target_alpha).abs() < 0.01
        })
    }
}

#[derive(Default)]
struct Session {
    peak: u64,
    sum: f64,
    samples: u64,
}

struct RemoveModal {
    id: Id,
    name: SharedString,
    finished: bool,
    delete_file: bool,
}

/// Giving a download a fresh link, its old one having expired.
struct RefreshModal {
    id: Id,
    name: SharedString,
    input: Entity<InputState>,
    error: Option<SharedString>,
}

// ---------------------------------------------------------------------------------------------
// The view

struct IdmView {
    manager: Arc<Manager>,
    /// Every download, newest first, as of the last poll, and the poll before it (for easing).
    rows: Vec<Row>,
    prev: HashMap<Id, Row>,
    polled_at: Instant,
    throughput: Vec<u64>,
    throughput_at: Instant,
    settings: Settings,
    status_filter: StatusFilter,
    category: Option<Category>,
    tab: Tab,
    selected: Option<Id>,
    hovered: Option<Id>,
    command: Entity<InputState>,
    command_error: Option<SharedString>,
    prefilled: Option<String>,
    limits_open: bool,
    folder: Entity<InputState>,
    /// The computer's networks and their speeds, for combining.
    networks: Vec<NetworkUse>,
    /// Where updating has got to.
    update: crate::update::State,
    search: Entity<InputState>,
    /// What's typed in `search`, read when it changes.
    search_query: String,
    sort: Sort,
    start_at: Entity<InputState>,
    stop_at: Entity<InputState>,
    refresh: Option<RefreshModal>,
    limits_error: Option<SharedString>,
    remove: Option<RemoveModal>,

    copied_at: Option<Instant>,
    graph: GraphAnim,
    /// Connections in use, summed over downloads, once per poll.
    connections: VecDeque<usize>,
    session: Session,
    list_focus: FocusHandle,
    modal_focus: FocusHandle,
    panel_focus: FocusHandle,
    scroll: UniformListScrollHandle,
    /// Where the speed-cap slider's track was last drawn, for mouse-downs on it.
    cap_track: Rc<Cell<Bounds<Pixels>>>,
    started: Instant,
    last_frame: Instant,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl IdmView {
    fn new(manager: Arc<Manager>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let command =
            cx.new(|cx| InputState::new(window, cx).placeholder("Paste a link to download…"));
        let folder = cx.new(|cx| InputState::new(window, cx));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let start_at = cx.new(|cx| InputState::new(window, cx).placeholder("--:--"));
        let stop_at = cx.new(|cx| InputState::new(window, cx).placeholder("--:--"));
        let schedule = |this: &mut Self, _: &Entity<InputState>, event: &InputEvent, _: &mut Window, cx: &mut Context<Self>| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.apply_schedule(cx);
            }
        };
        let subscriptions = vec![
            cx.subscribe_in(&search, window, |this, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.search_query = input.read(cx).value().trim().to_string();
                    cx.notify();
                }
            }),
            cx.subscribe_in(&start_at, window, schedule),
            cx.subscribe_in(&stop_at, window, schedule),
            cx.subscribe_in(
                &command,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.submit_command(window, cx),
                    InputEvent::Change if this.command_error.take().is_some() => cx.notify(),
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &folder,
                window,
                |this, _, event: &InputEvent, _, cx| match event {
                    InputEvent::PressEnter { .. } | InputEvent::Blur => this.apply_folder(cx),
                    InputEvent::Change if this.limits_error.take().is_some() => cx.notify(),
                    _ => {}
                },
            ),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.prefill_from_clipboard(window, cx);
                }
            }),
        ];
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        });
        let list_focus = cx.focus_handle();
        list_focus.focus(window, cx);

        let mut this = Self {
            rows: Vec::new(),
            prev: HashMap::new(),
            polled_at: Instant::now(),
            throughput: Vec::new(),
            throughput_at: Instant::now(),
            settings: manager.settings(),
            manager,
            status_filter: StatusFilter::All,
            category: None,
            tab: Tab::Queue,
            selected: None,
            hovered: None,
            command,
            command_error: None,
            prefilled: None,
            limits_open: false,
            folder,
            networks: Vec::new(),
            update: crate::update::State::Idle,
            search,
            search_query: String::new(),
            sort: Sort::Newest,
            start_at,
            stop_at,
            refresh: None,
            limits_error: None,
            remove: None,
            copied_at: None,
            graph: GraphAnim::default(),
            connections: VecDeque::new(),
            session: Session::default(),
            list_focus,
            modal_focus: cx.focus_handle(),
            panel_focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            cap_track: Rc::new(Cell::new(Bounds::default())),
            started: Instant::now(),
            last_frame: Instant::now(),
            _poll: poll,
            _subscriptions: subscriptions,
        };
        this.refresh(cx);
        this.prev.clear();
        this.selected = this.visible().first().map(|r| r.id);
        this.prefill_from_clipboard(window, cx);
        this
    }

    /// Read the queue again; what's drawn eases from the last poll to this one.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let rows = self.manager.rows();
        self.prev = std::mem::replace(&mut self.rows, rows)
            .into_iter()
            .map(|r| (r.id, r))
            .collect();
        self.polled_at = Instant::now();
        let throughput = self.manager.throughput();
        if throughput != self.throughput {
            self.throughput = throughput;
            self.throughput_at = Instant::now();
        }
        self.settings = self.manager.settings();
        self.networks = self.manager.networks();
        if let Some(shell) = cx.try_global::<Shell>() {
            self.update = shell.updater.state();
        }
        // The tray's panel asked for a download to be shown.
        if let Some(id) = cx.try_global::<Shell>().and_then(|shell| shell.select) {
            cx.global_mut::<Shell>().select = None;
            self.selected = Some(id);
        }

        let speed: u64 = self
            .rows
            .iter()
            .filter(|r| r.status == Status::Downloading)
            .map(|r| r.speed)
            .sum();
        self.session.peak = self.session.peak.max(speed);
        if speed > 0 {
            self.session.sum += speed as f64;
            self.session.samples += 1;
        }
        self.connections
            .push_back(self.rows.iter().map(|r| r.connections).sum());
        while self.connections.len() > CONN_SLOTS {
            self.connections.pop_front();
        }
        if self
            .selected
            .is_some_and(|id| !self.rows.iter().any(|r| r.id == id))
        {
            self.selected = None;
        }
        cx.notify();
    }

    fn visible(&self) -> Vec<&Row> {
        let query = self.search_query.to_lowercase();
        let mut rows: Vec<&Row> = self
            .rows
            .iter()
            .filter(|r| {
                self.status_filter.matches(r)
                    && self.category.is_none_or(|c| Category::of(r) == c)
                    && (query.is_empty() || r.name.to_lowercase().contains(&query) || r.url.to_lowercase().contains(&query))
            })
            .collect();
        match self.sort {
            Sort::Newest => {} // the manager's order
            Sort::Oldest => rows.reverse(),
            Sort::Name => rows.sort_by_key(|r| r.name.to_lowercase()),
            Sort::Size => rows.sort_by_key(|r| std::cmp::Reverse(r.total)),
            Sort::Progress => rows.sort_by(|a, b| fraction(b).total_cmp(&fraction(a))),
            // Waiting ones in start order first, then the rest as they were.
            Sort::Queue => rows.sort_by_key(|r| if r.position == 0 { usize::MAX } else { r.position }),
        }
        rows
    }

    fn row(&self, id: Id) -> Option<&Row> {
        self.rows.iter().find(|r| r.id == id)
    }

    fn selected_row(&self) -> Option<&Row> {
        self.selected.and_then(|id| self.row(id))
    }

    /// 0 → 1 over the easing time since the last poll.
    fn ease(&self) -> f32 {
        ease_out((self.polled_at.elapsed().as_secs_f32() / EASE).min(1.))
    }

    fn shown_fraction(&self, row: &Row) -> f32 {
        let now = fraction(row);
        match self.prev.get(&row.id) {
            Some(prev) if prev.total == row.total => lerp(fraction(prev), now, self.ease()),
            _ => now,
        }
    }

    fn shown_speed(&self, row: &Row) -> f32 {
        let prev = self.prev.get(&row.id).map_or(row.speed, |p| p.speed);
        lerp(prev as f32, row.speed as f32, self.ease())
    }

    fn shown_total_speed(&self) -> f32 {
        let sum = |rows: &mut dyn Iterator<Item = &Row>| {
            rows.filter(|r| r.status == Status::Downloading)
                .map(|r| r.speed)
                .sum::<u64>() as f32
        };
        let now = sum(&mut self.rows.iter());
        if self.prev.is_empty() {
            return now;
        }
        lerp(sum(&mut self.prev.values()), now, self.ease())
    }

    /// The segment map, with each part's fill and end eased from the last poll. A part that was
    /// just split off starts empty, and the part it came from shrinks to make room.
    fn shown_parts(&self, row: &Row) -> Vec<Segment> {
        let Some(prev) = self.prev.get(&row.id) else {
            return row.parts.clone();
        };
        let t = self.ease();
        row.parts
            .iter()
            .map(|s| {
                let before = prev.parts.iter().find(|p| (p.start - s.start).abs() < 1e-5);
                let (done, end) = before.map_or((s.start, s.end), |p| (p.done, p.end));
                let end = lerp(end, s.end, t);
                Segment {
                    done: lerp(done, s.done, t).min(end),
                    end,
                    ..*s
                }
            })
            .collect()
    }

    // --- Actions ------------------------------------------------------------------------------

    fn select(&mut self, id: Id, cx: &mut Context<Self>) {
        self.selected = Some(id);
        if let Some(ix) = self.visible().iter().position(|r| r.id == id) {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    fn move_selection(&mut self, by: isize, cx: &mut Context<Self>) {
        let ids: Vec<Id> = self.visible().iter().map(|r| r.id).collect();
        if ids.is_empty() {
            return;
        }
        let ix = match self
            .selected
            .and_then(|id| ids.iter().position(|i| *i == id))
        {
            Some(ix) => (ix as isize + by).clamp(0, ids.len() as isize - 1) as usize,
            None if by > 0 => 0,
            None => ids.len() - 1,
        };
        self.select(ids[ix], cx);
    }

    fn run_action(
        &mut self,
        action: RowAction,
        id: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.row(id).cloned() else {
            return;
        };
        match action {
            RowAction::Open => open_file(&row, cx),
            RowAction::Reveal => reveal(&row, &self.settings, cx),
            RowAction::Toggle => {
                match row.status {
                    Status::Paused | Status::Failed => self.manager.start(id),
                    Status::Queued | Status::Downloading => self.manager.pause(id),
                    Status::Done => {}
                }
                self.refresh(cx);
            }
            RowAction::Restart => {
                self.manager.restart(id);
                self.refresh(cx);
            }
            RowAction::Remove => {
                self.remove = Some(RemoveModal {
                    id,
                    name: display_name(&row).into(),
                    finished: row.status == Status::Done,
                    delete_file: false,
                });
                self.modal_focus.focus(window, cx);
                cx.notify();
            }
            RowAction::CopyUrl => {
                cx.write_to_clipboard(ClipboardItem::new_string(row.url));
                self.copied_at = Some(Instant::now());
                cx.notify();
            }
            RowAction::RefreshLink => {
                // A fresh link on the clipboard is the likely one; otherwise start empty.
                let start = clipboard_link(cx).filter(|url| *url != row.url).unwrap_or_default();
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("Paste the new link…"));
                input.update(cx, |input, cx| {
                    input.set_value(start, window, cx);
                    input.focus(window, cx);
                });
                self.refresh = Some(RefreshModal { id, name: display_name(&row).into(), input, error: None });
                cx.notify();
            }
            RowAction::Move(to) => {
                self.manager.move_to(id, to);
                self.refresh(cx);
            }
        }
    }

    fn confirm_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.refresh else { return };
        let url = modal.input.read(cx).value().trim().to_string();
        match self.manager.refresh_link(modal.id, &url) {
            Ok(()) => {
                self.refresh = None;
                self.list_focus.focus(window, cx);
                self.refresh(cx);
            }
            Err(err) => modal.error = Some(err.into()),
        }
        cx.notify();
    }

    /// The schedule's two times, as typed: empty turns one off.
    fn apply_schedule(&mut self, cx: &mut Context<Self>) {
        let (start, stop) = (self.start_at.read(cx).value().to_string(), self.stop_at.read(cx).value().to_string());
        let (start_at, stop_at) = (crate::queue::parse_clock(&start), crate::queue::parse_clock(&stop));
        let bad = |text: &str, parsed: Option<(u8, u8)>| !text.trim().is_empty() && parsed.is_none();
        if bad(&start, start_at) || bad(&stop, stop_at) {
            self.limits_error = Some("Times are 24-hour HH:MM, like 01:30.".into());
            cx.notify();
            return;
        }
        if (start_at, stop_at) != (self.settings.start_at, self.settings.stop_at) {
            self.set_settings(|s| (s.start_at, s.stop_at) = (start_at, stop_at), cx);
        }
    }

    fn on_selected(&mut self, action: RowAction, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.selected {
            self.run_action(action, id, window, cx);
        }
    }

    fn add(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.settings.ask_where {
            self.add_asking_where(url.to_string(), window, cx);
            return true;
        }
        let added = self.manager.add(url);
        self.added(added, window, cx)
    }

    /// Ask for a folder, then add the link there.
    fn add_asking_where(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Save here".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            this.update_in(cx, |this, window, cx| {
                let added = this.manager.add_to(&url, dir);
                this.added(added, window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn added(&mut self, added: Result<Id, String>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match added {
            Ok(id) => {
                self.command_error = None;
                self.refresh(cx);
                if self.row(id).is_some_and(|r| {
                    !self.status_filter.matches(r)
                        || self.category.is_some_and(|c| Category::of(r) != c)
                }) {
                    self.status_filter = StatusFilter::All;
                    self.category = None;
                }
                self.tab = Tab::Queue;
                self.select(id, cx);
                true
            }
            Err(err) => {
                self.command_error = Some(err.into());
                self.command.update(cx, |input, cx| input.focus(window, cx));
                cx.notify();
                false
            }
        }
    }

    fn submit_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.command.read(cx).value().trim().to_string();
        if url.is_empty() {
            self.command_error = Some("Paste a link first.".into());
            cx.notify();
            return;
        }
        if self.add(&url, window, cx) {
            self.command
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    /// ⌘V outside an input: add the link on the clipboard straight away.
    fn paste_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match clipboard_link(cx) {
            Some(url) => {
                if self.add(&url, window, cx) {
                    self.prefilled = Some(url);
                    self.command
                        .update(cx, |input, cx| input.set_value("", window, cx));
                }
            }
            None => {
                self.command_error = Some("The clipboard doesn't hold an http(s) link.".into());
                cx.notify();
            }
        }
    }

    /// Put a link from the clipboard in the command bar, once per link.
    fn prefill_from_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = clipboard_link(cx) else {
            return;
        };
        if self.prefilled.as_ref() == Some(&url)
            || !self.command.read(cx).value().is_empty()
            || self.rows.iter().any(|r| r.url == url)
        {
            return;
        }
        self.prefilled = Some(url.clone());
        self.command
            .update(cx, |input, cx| input.set_value(url, window, cx));
    }

    fn set_settings(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) -> bool {
        let mut settings = self.manager.settings();
        change(&mut settings);
        let ok = match self.manager.set_settings(settings) {
            Ok(()) => {
                self.limits_error = None;
                true
            }
            Err(err) => {
                self.limits_error = Some(err.into());
                false
            }
        };
        self.settings = self.manager.settings();
        cx.notify();
        ok
    }

    fn set_cap_from(&mut self, x: Pixels, track: Bounds<Pixels>, cx: &mut Context<Self>) {
        let f = ((x - track.left()) / track.size.width).clamp(0., 1.);
        // Half-MB/s steps; the far left is "no cap".
        let cap = ((f * (CAP_MAX / MB) as f32 * 2.).round() as u64) * MB / 2;
        if cap != self.settings.speed_limit {
            self.set_settings(|s| s.speed_limit = cap, cx);
        }
    }

    fn apply_folder(&mut self, cx: &mut Context<Self>) {
        let text = self.folder.read(cx).value().trim().to_string();
        if text.is_empty() {
            self.limits_error = Some("Choose a download folder.".into());
            cx.notify();
            return;
        }
        let dir = expand_home(&text);
        if dir != self.settings.dir {
            self.set_settings(|s| s.dir = dir, cx);
        }
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.folder
                    .update(cx, |input, cx| input.set_value(tilde(&path), window, cx));
                this.apply_folder(cx);
            })
            .ok();
        })
        .detach();
    }

    fn toggle_limits(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.limits_open = !self.limits_open;
        self.limits_error = None;
        if self.limits_open {
            let dir = tilde(&self.settings.dir);
            self.folder
                .update(cx, |input, cx| input.set_value(dir, window, cx));
            let (start, stop) = (crate::queue::format_clock(self.settings.start_at), crate::queue::format_clock(self.settings.stop_at));
            self.start_at.update(cx, |input, cx| input.set_value(start, window, cx));
            self.stop_at.update(cx, |input, cx| input.set_value(stop, window, cx));
            self.panel_focus.focus(window, cx);
        } else {
            self.list_focus.focus(window, cx);
        }
        cx.notify();
    }

    fn confirm_remove(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(modal) = self.remove.take() {
            self.manager.remove(modal.id, modal.delete_file);
            if self.selected == Some(modal.id) {
                self.selected = None;
            }
            self.refresh(cx);
        }
        self.list_focus.focus(window, cx);
        cx.notify();
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remove.take().is_none() && self.refresh.take().is_none() {
            self.limits_open = false;
        }
        self.list_focus.focus(window, cx);
        cx.notify();
    }

    /// The download the connection graph shows: the selection, else the busiest one.
    fn graph_row(&self) -> Option<&Row> {
        self.selected_row().or_else(|| {
            self.rows
                .iter()
                .filter(|r| r.status == Status::Downloading)
                .max_by_key(|r| r.speed)
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Rendering

impl Render for IdmView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dt = self.last_frame.elapsed().as_secs_f32().min(0.1);
        self.last_frame = Instant::now();
        let graph_row = self.graph_row().cloned();
        self.graph.step(graph_row.as_ref(), dt);

        // Keep drawing while something moves: easing, the chart scroll, edges and parts filling.
        let downloading = self.rows.iter().any(|r| r.status == Status::Downloading);
        let easing = self.polled_at.elapsed().as_secs_f32() < EASE;
        if downloading
            || easing
            || !self.graph.settled()
            || self.throughput_at.elapsed() < Duration::from_secs(1)
        {
            window.request_animation_frame();
        }

        let empty = self.rows.is_empty();
        v_flex()
            .id("shard")
            .size_full()
            .relative()
            .key_context(CONTEXT)
            .bg(ink::bg())
            .text_color(ink::text())
            .font_family(SANS)
            .text_size(px(13.))
            .on_action(cx.listener(|this, _: &FocusCommand, window, cx| {
                this.limits_open = false;
                this.command.update(cx, |input, cx| {
                    input.focus(window, cx);
                    input.select_all(window, cx);
                });
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &ToggleLimits, window, cx| this.toggle_limits(window, cx)),
            )
            .child(
                self.render_title_bar(
                    self.command
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx),
                    cx,
                ),
            )
            .child(
                v_flex()
                    .id("body")
                    .flex_1()
                    .min_h_0()
                    .px(px(16.))
                    .pt(px(16.))
                    .pb(px(12.))
                    .gap(px(12.))
                    .key_context(LIST)
                    .track_focus(&self.list_focus)
                    .on_action(
                        cx.listener(|this, _: &PasteLink, window, cx| this.paste_link(window, cx)),
                    )
                    .on_action(cx.listener(|this, _: &TogglePause, window, cx| {
                        this.on_selected(RowAction::Toggle, window, cx)
                    }))
                    .on_action(
                        cx.listener(|this, _: &SelectPrev, _, cx| this.move_selection(-1, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &SelectNext, _, cx| this.move_selection(1, cx)),
                    )
                    .on_action(cx.listener(|this, _: &RemoveSelected, window, cx| {
                        this.on_selected(RowAction::Remove, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &OpenSelected, _, cx| {
                        if let Some(row) = this.selected_row().filter(|r| r.status == Status::Done)
                        {
                            open_file(row, cx);
                        }
                    }))
                    .child(self.render_hero(graph_row.as_ref(), cx))
                    .when(empty, |this| this.child(render_empty_state()))
                    .when(!empty, |this| {
                        this.child(
                            self.render_filters(window.viewport_size().width < px(1180.), cx),
                        )
                        .child(match self.tab {
                            Tab::Queue => h_flex()
                                .flex_1()
                                .min_h_0()
                                .items_stretch()
                                .gap(px(12.))
                                .child(self.render_queue(cx))
                                .child(self.render_detail(cx))
                                .into_any_element(),
                            Tab::Stats => self.render_stats().into_any_element(),
                        })
                    }),
            )
            .child(self.render_status_bar())
            .when_some(self.command_error.clone(), |this, err| {
                this.child(render_command_error(err, cx))
            })
            .when_some(power_alert(cx), |this, alert| {
                this.child(render_power_banner(alert, cx))
            })
            .when(self.limits_open, |this| this.child(self.render_limits(cx)))
            .when(self.remove.is_some(), |this| {
                this.child(self.render_remove(cx))
            })
            .when(self.refresh.is_some(), |this| {
                this.child(self.render_refresh(cx))
            })
    }
}

impl IdmView {
    fn render_title_bar(&self, focused: bool, cx: &Context<Self>) -> impl IntoElement {
        let cap = self.settings.speed_limit;
        TitleBar::new()
            .h(px(52.))
            .bg(ink::bg())
            .border_color(ink::line())
            .child(
                h_flex()
                    .w_full()
                    .h_full()
                    .pr(px(14.))
                    .gap(px(16.))
                    .child(wordmark())
                    .child(
                        h_flex().flex_1().min_w_0().justify_center().child(
                            // Keep clicks and drags here from moving the window.
                            h_flex()
                                .id("command")
                                .w_full()
                                .max_w(px(560.))
                                .h(px(34.))
                                .pl(px(12.))
                                .pr(px(4.))
                                .gap(px(8.))
                                .rounded(px(10.))
                                .bg(ink::panel())
                                .border_1()
                                .border_color(if self.command_error.is_some() {
                                    ink::red().opacity(0.45)
                                } else if focused {
                                    ink::accent().opacity(0.45)
                                } else {
                                    ink::line_strong()
                                })
                                .when(focused, |this| this.shadow(glow(ink::accent(), 0.14, 18.)))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(
                                    Icon::new(Lucide::Link)
                                        .with_size(px(14.))
                                        .text_color(ink::faint()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .font_family(MONO)
                                        .text_size(px(12.5))
                                        .child(
                                            Input::new(&self.command)
                                                .appearance(false)
                                                .font_family(MONO)
                                                .text_size(px(12.5))
                                                .h(px(28.))
                                                .px_0(),
                                        ),
                                )
                                .child(div().text_color(ink::faint()).child(kbd("⌘L")))
                                .child(
                                    accent_button("command-add", "Add", Some(Lucide::Plus))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.submit_command(window, cx)
                                        })),
                                ),
                        ),
                    )
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .children(self.render_update_pill(cx))
                            .child(
                                h_flex()
                                    .id("cap-pill")
                                    .h(px(28.))
                                    .px(px(12.))
                                    .gap(px(6.))
                                    .rounded_full()
                                    .border_1()
                                    .cursor_pointer()
                                    .map(|this| {
                                        if cap > 0 {
                                            this.border_color(ink::accent().opacity(0.4))
                                                .bg(ink::accent().opacity(0.08))
                                        } else {
                                            this.border_color(ink::line_strong())
                                        }
                                    })
                                    .when(self.limits_open, |this| this.bg(ink::inset()))
                                    .hover(|this| this.bg(ink::inset()))
                                    .child(dot(
                                        if cap > 0 { ink::accent() } else { ink::faint() },
                                        6.,
                                    ))
                                    .child(if cap > 0 {
                                        caps(format!("CAP {}", format_speed(cap)), ink::accent2())
                                    } else {
                                        caps("NO CAP", ink::muted())
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_limits(window, cx)
                                    })),
                            )
                            .child(
                                icon_button("limits", Lucide::SlidersHorizontal, "Limits (⌘,)")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_limits(window, cx)
                                    })),
                            ),
                    ),
            )
    }

    fn render_hero(&self, graph_row: Option<&Row>, cx: &Context<Self>) -> impl IntoElement {
        let speed = self.shown_total_speed();
        let active = self
            .rows
            .iter()
            .filter(|r| r.status == Status::Downloading)
            .count();
        let queued = self
            .rows
            .iter()
            .filter(|r| r.status == Status::Queued)
            .count();
        let conns: usize = self.rows.iter().map(|r| r.connections).sum();
        let (value, unit) = speed_parts(speed as u64);
        let cap = self.settings.speed_limit;
        let live = speed > 0.;

        let number = div()
            .relative()
            .child(
                // The glow behind the number: bright while data flows, dim when idle.
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(22.))
                    .w(px(120.))
                    .h(px(18.))
                    .shadow(glow(ink::accent(), if live { 0.30 } else { 0.06 }, 64.)),
            )
            .child(
                h_flex()
                    .items_baseline()
                    .gap(px(6.))
                    .child(
                        div()
                            .font_family(MONO)
                            .font_weight(FontWeight::MEDIUM)
                            .text_size(px(if cap > 0 { 46. } else { 56. }))
                            .line_height(px(if cap > 0 { 52. } else { 60. }))
                            .text_color(if live { ink::text() } else { ink::muted() })
                            .child(value),
                    )
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(14.))
                            .text_color(ink::muted())
                            .child(unit),
                    ),
            );

        let speed_block = v_flex()
            .gap(px(8.))
            .child(caps("TOTAL SPEED", ink::faint()))
            .child(number)
            .child(caps(
                format!("{active} ACTIVE · {queued} QUEUED · {conns} CONNECTIONS"),
                ink::muted(),
            ))
            .children(self.render_network_share());

        let gauge = (cap > 0).then(|| {
            let frac = (speed / cap as f32).min(1.);
            div()
                .relative()
                .size(px(116.))
                .flex_shrink_0()
                .child(
                    canvas(
                        |_, _, _| {},
                        move |b, _, window, _| paint_gauge(b, frac, window),
                    )
                    .size_full(),
                )
                .child(
                    v_flex()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .gap(px(2.))
                        .child(
                            div()
                                .font_family(MONO)
                                .font_weight(FontWeight::MEDIUM)
                                .text_size(px(20.))
                                .child(format!("{:.0}%", frac * 100.)),
                        )
                        .child(caps("OF CAP", ink::faint()))
                        .child(
                            div()
                                .font_family(MONO)
                                .text_size(px(10.5))
                                .text_color(ink::accent2())
                                .child(format_speed(cap)),
                        ),
                )
        });

        let graph = self.render_graph(graph_row);
        let chart = self.render_throughput();

        h_flex()
            .id("hero")
            .relative()
            .flex_shrink_0()
            .h(px(HERO_HEIGHT))
            .px(px(24.))
            .py(px(18.))
            .gap(px(24.))
            .rounded(px(16.))
            .bg(ink::panel())
            .border_1()
            .border_color(ink::line())
            .overflow_hidden()
            .child(corner_glow(live, HERO_HEIGHT))
            .child(
                h_flex()
                    .flex_shrink_0()
                    .h_full()
                    .gap(px(18.))
                    .children(gauge)
                    .child(speed_block),
            )
            .child(vrule())
            .child(div().flex_1().min_w(px(300.)).h_full().child(graph))
            .child(vrule())
            .child(div().flex_1().min_w(px(260.)).h_full().child(chart))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.list_focus.focus(window, cx)),
            )
    }

    /// Server → one node per unfinished part → file, with particles running along busy edges.
    fn render_graph(&self, row: Option<&Row>) -> impl IntoElement {
        let host = row.map_or("—".to_string(), |r| host(&r.url));
        let name = row.map_or("NO ACTIVE DOWNLOAD".to_string(), display_name);
        let ext = row.map_or("—".to_string(), extension_label);
        let pct = row.map(|r| {
            if r.total > 0 || r.status == Status::Done {
                format!("{:.0}%", self.shown_fraction(r) * 100.)
            } else {
                format_bytes(r.done)
            }
        });
        let live = row.is_some_and(|r| r.status == Status::Downloading);
        let status = row.map(|r| r.status);
        let max_speed = self.graph.nodes.iter().map(|n| n.speed).fold(1., f32::max);
        let fastest = self
            .graph
            .nodes
            .iter()
            .filter(|n| n.active)
            .max_by(|a, b| a.speed.total_cmp(&b.speed))
            .map(|n| n.key);
        let nodes: Vec<(f32, f32, f32, bool, bool, f32)> = self
            .graph
            .nodes
            .iter()
            .map(|n| {
                (
                    n.y,
                    n.alpha,
                    (n.speed / max_speed).clamp(0., 1.),
                    n.active,
                    Some(n.key) == fastest,
                    n.phase,
                )
            })
            .collect();
        let labels: Vec<(f32, f32, bool, bool, String)> = self
            .graph
            .nodes
            .iter()
            .map(|n| {
                let label = if n.active {
                    format_speed(n.speed as u64)
                } else {
                    "idle".into()
                };
                (n.y, n.alpha, n.active, Some(n.key) == fastest, label)
            })
            .collect();
        let hidden = self.graph.hidden;
        let hidden_y = self.graph.hidden_y;
        let idle = nodes.is_empty();
        let summary = match row {
            Some(r) if r.status == Status::Downloading => {
                format!("{} PARTS · {} LIVE", r.parts.len().max(1), r.connections)
            }
            Some(r) => status_word(r.status).to_string(),
            None => "IDLE".into(),
        };

        v_flex()
            .size_full()
            .gap(px(8.))
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(caps("CONNECTIONS", ink::faint()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(ink::muted())
                            .child(name),
                    )
                    .child(caps(
                        summary,
                        if live { ink::accent2() } else { ink::faint() },
                    )),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(GRAPH_HEIGHT))
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |b, _, window, _| paint_graph(b, &nodes, live, window),
                        )
                        .absolute()
                        .inset_0(),
                    )
                    // Server node.
                    .child(
                        h_flex()
                            .absolute()
                            .left_0()
                            .top(px(GRAPH_HEIGHT / 2. - 13.))
                            .w(px(GRAPH_SERVER_W))
                            .h(px(26.))
                            .px(px(8.))
                            .gap(px(6.))
                            .rounded(px(8.))
                            .bg(ink::inset())
                            .border_1()
                            .border_color(if live {
                                ink::line_strong()
                            } else {
                                ink::line()
                            })
                            .child(
                                Icon::new(Lucide::Globe)
                                    .with_size(px(12.))
                                    .text_color(ink::faint()),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(MONO)
                                    .text_size(px(10.5))
                                    .text_color(ink::muted())
                                    .child(host),
                            ),
                    )
                    // Part nodes.
                    .children(labels.into_iter().map(|(y, alpha, active, fast, label)| {
                        h_flex()
                            .absolute()
                            .top(px(y - 8.))
                            .left(relative(0.5))
                            .ml(px(GRAPH_SHIFT - GRAPH_NODE_W / 2.))
                            .w(px(GRAPH_NODE_W))
                            .h(px(16.))
                            .px(px(6.))
                            .gap(px(5.))
                            .rounded_full()
                            .bg(ink::card())
                            .border_1()
                            .border_color(if fast {
                                ink::accent().opacity(0.6)
                            } else {
                                ink::line_strong()
                            })
                            .opacity(alpha)
                            .child(dot(
                                if fast {
                                    ink::accent()
                                } else if active {
                                    ink::white2()
                                } else {
                                    ink::faint()
                                },
                                5.,
                            ))
                            .child(
                                div()
                                    .font_family(MONO)
                                    .text_size(px(9.5))
                                    .line_height(px(12.))
                                    .text_color(if fast {
                                        ink::accent2()
                                    } else if active {
                                        ink::text()
                                    } else {
                                        ink::faint()
                                    })
                                    .child(label),
                            )
                    }))
                    .when(hidden > 0, |this| {
                        this.child(
                            h_flex()
                                .absolute()
                                .top(px(hidden_y - 8.))
                                .left(relative(0.5))
                                .ml(px(GRAPH_SHIFT - GRAPH_NODE_W / 2.))
                                .w(px(GRAPH_NODE_W))
                                .h(px(16.))
                                .justify_center()
                                .rounded_full()
                                .border_1()
                                .border_color(ink::line_strong())
                                .child(caps(format!("+{hidden} PARTS"), ink::faint())),
                        )
                    })
                    .when(idle, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top(px(GRAPH_HEIGHT / 2. - 18.))
                                .left(relative(0.5))
                                .ml(px(GRAPH_SHIFT - 60.))
                                .w(px(120.))
                                .flex()
                                .justify_center()
                                .child(caps(
                                    match status {
                                        Some(Status::Done) => "COMPLETE",
                                        Some(Status::Queued) => "WAITING",
                                        Some(Status::Paused) => "PAUSED",
                                        Some(Status::Failed) => "FAILED",
                                        _ => "IDLE",
                                    },
                                    ink::faint(),
                                )),
                        )
                    })
                    // File node: the tile sits on the centre line the edges meet; the percent hangs below.
                    .child(
                        v_flex()
                            .absolute()
                            .right_0()
                            .top(px(GRAPH_HEIGHT / 2. - GRAPH_FILE_TILE / 2.))
                            .w(px(GRAPH_FILE_W))
                            .items_center()
                            .gap(px(4.))
                            .child(type_tile(&ext, GRAPH_FILE_TILE, live))
                            .child(
                                div()
                                    .font_family(MONO)
                                    .text_size(px(11.))
                                    .text_color(if live { ink::accent2() } else { ink::muted() })
                                    .child(pct.unwrap_or_default()),
                            ),
                    ),
            )
    }

    fn render_throughput(&self) -> impl IntoElement {
        let data = self.throughput.clone();
        let selected: Vec<u64> = self
            .selected_row()
            .filter(|r| r.status == Status::Downloading)
            .map(|r| r.history.clone())
            .unwrap_or_default();
        let peak = data.iter().copied().max().unwrap_or(0);
        let nonzero: Vec<u64> = data.iter().copied().filter(|v| *v > 0).collect();
        let avg = if nonzero.is_empty() {
            0
        } else {
            nonzero.iter().sum::<u64>() / nonzero.len() as u64
        };
        let scale = nice_max(peak.max(self.settings.speed_limit.min(peak.saturating_mul(2))));
        let phase = (self.throughput_at.elapsed().as_secs_f32()).min(1.);
        let cap = self.settings.speed_limit;
        let has_selected = !selected.is_empty();

        v_flex()
            .size_full()
            .gap(px(8.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(caps("THROUGHPUT", ink::faint()))
                    .child(div().flex_1())
                    .when(has_selected, |this| {
                        this.child(
                            h_flex()
                                .gap(px(4.))
                                .child(dot(ink::white2(), 5.))
                                .child(caps("SELECTED", ink::faint())),
                        )
                    })
                    .child(
                        h_flex()
                            .gap(px(4.))
                            .child(dot(ink::accent(), 5.))
                            .child(caps("ALL", ink::faint())),
                    ),
            )
            .child(
                div().relative().flex_1().w_full().child(
                    canvas(
                        |_, _, _| {},
                        move |b, _, window, cx| {
                            paint_throughput(
                                b, &data, &selected, scale, avg, cap, phase, window, cx,
                            )
                        },
                    )
                    .absolute()
                    .inset_0(),
                ),
            )
            .child(
                h_flex()
                    .justify_between()
                    .child(caps("-60S", ink::faint()))
                    .child(caps("-30S", ink::faint()))
                    .child(caps("NOW", ink::faint())),
            )
    }

    fn render_filters(&self, compact: bool, cx: &Context<Self>) -> impl IntoElement {
        let count = |f: StatusFilter| self.rows.iter().filter(|r| f.matches(r)).count();
        let ccount = |c: Category| self.rows.iter().filter(|r| Category::of(r) == c).count();
        let tab = |id: &'static str, label: &'static str, t: Tab| {
            let on = self.tab == t;
            div()
                .id(id)
                .h(px(24.))
                .px(px(if compact { 9. } else { 12. }))
                .flex()
                .items_center()
                .rounded(px(6.))
                .cursor_pointer()
                .when(on, |this| {
                    this.bg(ink::inset())
                        .border_1()
                        .border_color(ink::line_strong())
                })
                .when(!on, |this| this.hover(|s| s.bg(ink::card())))
                .child(caps(label, if on { ink::text() } else { ink::faint() }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tab = t;
                    cx.notify();
                }))
        };
        h_flex()
            .flex_shrink_0()
            .h(px(32.))
            .gap(px(if compact { 4. } else { 6. }))
            .child(
                h_flex()
                    .p(px(3.))
                    .gap(px(2.))
                    .rounded(px(8.))
                    .bg(ink::panel())
                    .border_1()
                    .border_color(ink::line())
                    .child(tab("tab-queue", "QUEUE", Tab::Queue))
                    .child(tab("tab-stats", "STATS", Tab::Stats)),
            )
            .child(div().w(px(10.)))
            .children(StatusFilter::ALL.map(|f| {
                chip(
                    ("status", f as usize),
                    f.label(),
                    Some(count(f)),
                    self.status_filter == f,
                    compact,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.status_filter = f;
                    cx.notify();
                }))
            }))
            .child(div().w(px(1.)).h(px(16.)).mx(px(6.)).bg(ink::line_strong()))
            .children(Category::ALL.map(|c| {
                chip(
                    ("category", c as usize),
                    c.label(),
                    (!compact).then(|| ccount(c)),
                    self.category == Some(c),
                    compact,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.category = if this.category == Some(c) {
                        None
                    } else {
                        Some(c)
                    };
                    cx.notify();
                }))
            }))
    }

    fn render_queue(&self, cx: &Context<Self>) -> impl IntoElement {
        let count = self.visible().len();
        let active = self
            .rows
            .iter()
            .filter(|r| r.status == Status::Downloading)
            .count();
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .rounded(px(16.))
            .bg(ink::panel())
            .border_1()
            .border_color(ink::line())
            .overflow_hidden()
            .child(
                h_flex()
                    .flex_shrink_0()
                    .h(px(40.))
                    .px(px(16.))
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Queue"),
                    )
                    .child(
                        h_flex()
                            .w(px(190.))
                            .h(px(26.))
                            .px(px(8.))
                            .gap(px(6.))
                            .rounded(px(7.))
                            .bg(ink::inset())
                            .border_1()
                            .border_color(ink::line())
                            .child(Icon::new(Lucide::Link).with_size(px(11.)).text_color(ink::faint()))
                            .child(Input::new(&self.search).appearance(false).text_size(px(12.)).h(px(22.)).px_0()),
                    )
                    .child(
                        h_flex()
                            .id("sort")
                            .h(px(26.))
                            .px(px(8.))
                            .rounded(px(7.))
                            .border_1()
                            .border_color(ink::line())
                            .cursor_pointer()
                            .hover(|s| s.bg(ink::inset()))
                            .child(caps(format!("SORT · {}", self.sort.label()), ink::muted()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sort = this.sort.next();
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(caps(
                        format!("{active} DOWNLOADING · {count} SHOWN"),
                        ink::faint(),
                    ))
                    .child(div().w(px(8.)))
                    .child(
                        icon_button("resume-all", Lucide::Play, "Resume all").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.manager.start_all();
                                this.refresh(cx);
                            }),
                        ),
                    )
                    .child(
                        icon_button("pause-all", Lucide::Pause, "Pause all").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.manager.pause_all();
                                this.refresh(cx);
                            },
                        )),
                    ),
            )
            .child(if count == 0 {
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child(caps("NOTHING HERE", ink::faint()))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(ink::faint())
                            .child("No downloads match these filters."),
                    )
                    .into_any_element()
            } else {
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        uniform_list(
                            "queue",
                            count,
                            cx.processor(|this, range: Range<usize>, _window, cx| {
                                let rows: Vec<Row> = this.visible()[range.clone()]
                                    .iter()
                                    .map(|r| (*r).clone())
                                    .collect();
                                rows.iter()
                                    .map(|row| this.render_card(row, cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .size_full()
                        .px(px(8.))
                        .pb(px(8.))
                        .track_scroll(&self.scroll),
                    )
                    .vertical_scrollbar(&self.scroll)
                    .into_any_element()
            })
    }

    fn render_card(&self, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let id = row.id;
        let selected = self.selected == Some(id);
        let hovered = self.hovered == Some(id);
        let frac = self.shown_fraction(row);
        let parts = self.shown_parts(row);
        let status = row.status;
        let state = state_label(row, frac);
        let meta = meta_line(row, self.shown_speed(row));
        let ext = extension_label(row);
        let history = row.history.clone();
        let time = self.started.elapsed().as_secs_f32();
        let total = row.total;

        let actions = h_flex()
            .gap(px(2.))
            .child(
                icon_button(
                    ("card-toggle", id as u64),
                    if matches!(status, Status::Paused | Status::Failed) {
                        Lucide::Play
                    } else {
                        Lucide::Pause
                    },
                    if matches!(status, Status::Paused | Status::Failed) {
                        "Resume"
                    } else {
                        "Pause"
                    },
                )
                .when(status == Status::Done, |b| b.opacity(0.3))
                .when(status != Status::Done, |b| {
                    b.on_click(cx.listener(move |this, _, window, cx| {
                        this.run_action(RowAction::Toggle, id, window, cx)
                    }))
                }),
            )
            .child(
                icon_button(
                    ("card-reveal", id as u64),
                    Lucide::FolderOpen,
                    "Show in folder",
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run_action(RowAction::Reveal, id, window, cx)
                })),
            )
            .child(
                icon_button(("card-remove", id as u64), Lucide::Trash, "Remove").on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.run_action(RowAction::Remove, id, window, cx)
                    }),
                ),
            );

        let view = cx.entity().downgrade();
        div()
            .id(("card", id as u64))
            .h(px(CARD_HEIGHT + CARD_GAP))
            .pb(px(CARD_GAP))
            .child(
                h_flex()
                    .id(("card-body", id as u64))
                    .relative()
                    .size_full()
                    .px(px(14.))
                    .gap(px(14.))
                    .rounded(px(10.))
                    .bg(ink::card())
                    .border_1()
                    .border_color(if selected {
                        ink::line_strong()
                    } else {
                        ink::line()
                    })
                    .overflow_hidden()
                    .cursor_pointer()
                    .when(!selected, |this| this.hover(|s| s.bg(rgb(0x19191b))))
                    .when(selected, |this| this.child(corner_glow(true, CARD_HEIGHT)))
                    .child(type_tile(&ext, 44., status == Status::Downloading))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(4.))
                            .child(
                                h_flex()
                                    .h(px(20.))
                                    .gap(px(8.))
                                    .child(state_dot(status, id))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(if status == Status::Done {
                                                ink::muted()
                                            } else {
                                                ink::text()
                                            })
                                            .child(display_name(row)),
                                    )
                                    .when(hovered, |this| this.child(actions))
                                    .when(!hovered, |this| {
                                        this.when(
                                            status == Status::Downloading && history.len() > 1,
                                            |this| {
                                                this.child(
                                                    canvas(
                                                        |_, _, _| {},
                                                        move |b, _, window, _| {
                                                            paint_sparkline(
                                                                b, &history, false, window,
                                                            )
                                                        },
                                                    )
                                                    .w(px(56.))
                                                    .h(px(16.)),
                                                )
                                            },
                                        )
                                        .child(state)
                                    }),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .font_family(MONO)
                                    .text_size(px(11.))
                                    .text_color(if status == Status::Failed {
                                        ink::red().opacity(0.85)
                                    } else {
                                        ink::faint()
                                    })
                                    .child(meta),
                            )
                            .child(
                                canvas(
                                    |_, _, _| {},
                                    move |b, _, window, _| {
                                        paint_segments(
                                            b, &parts, status, total, frac, time, true, window,
                                        )
                                    },
                                )
                                .w_full()
                                .h(px(3.))
                                .mt(px(4.)),
                            ),
                    )
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.hovered = Some(id);
                        } else if this.hovered == Some(id) {
                            this.hovered = None;
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.list_focus.focus(window, cx);
                        this.select(id, cx);
                        if event.click_count() == 2
                            && let Some(row) = this.row(id).filter(|r| r.status == Status::Done) {
                                open_file(row, cx);
                            }
                    }))
                    .context_menu(move |menu, _, cx| row_menu(menu, &view, id, cx)),
            )
            .into_any_element()
    }

    fn render_detail(&self, cx: &Context<Self>) -> impl IntoElement {
        let panel = v_flex()
            .w(px(DETAIL_WIDTH))
            .flex_shrink_0()
            .h_full()
            .rounded(px(16.))
            .bg(ink::panel())
            .border_1()
            .border_color(ink::line())
            .overflow_hidden();
        let Some(row) = self.selected_row().cloned() else {
            return panel
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(mark(1.4))
                .child(caps("SELECT A DOWNLOAD", ink::faint()))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(ink::faint())
                        .child("Its parts, speed and details show here."),
                );
        };
        let id = row.id;
        let frac = self.shown_fraction(&row);
        let parts = self.shown_parts(&row);
        let status = row.status;
        let time = self.started.elapsed().as_secs_f32();
        let speed = self.shown_speed(&row);
        let total = row.total;
        let history = row.history.clone();
        let live = status == Status::Downloading;
        let copied = self
            .copied_at
            .is_some_and(|t| t.elapsed() < Duration::from_millis(1600));
        let want = self.settings.connections;
        let server_limited = live && !row.parts.is_empty() && row.connections < want;
        let avg = if history.is_empty() {
            0
        } else {
            history.iter().sum::<u64>() / history.len() as u64
        };
        let toggle_label = match status {
            Status::Paused | Status::Failed => "Resume",
            Status::Queued | Status::Downloading => "Pause",
            Status::Done => "Open",
        };
        let toggle_icon = match status {
            Status::Paused | Status::Failed => Lucide::Play,
            Status::Queued | Status::Downloading => Lucide::Pause,
            Status::Done => Lucide::ExternalLink,
        };
        let parts_label = if row.parts.is_empty() {
            if total == 0 {
                "SINGLE STREAM".to_string()
            } else {
                "NOT SPLIT YET".into()
            }
        } else {
            format!("{} PARTS · {} CONN", row.parts.len(), row.connections)
        };
        let eta = (live && row.total > row.done && row.speed > 0)
            .then(|| format_duration((row.total - row.done) / row.speed));
        let folder = row
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(tilde)
            .unwrap_or_else(|| tilde(&self.settings.dir));

        let tiles = v_flex()
            .gap(px(6.))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(stat_tile(
                        "SIZE",
                        if total > 0 {
                            format_bytes(total)
                        } else {
                            "—".into()
                        },
                        None,
                    ))
                    .child(stat_tile(
                        "DOWNLOADED",
                        format_bytes(if total > 0 {
                            (frac as f64 * total as f64) as u64
                        } else {
                            row.done
                        }),
                        None,
                    ))
                    .child(stat_tile(
                        "SPEED",
                        if live {
                            format_speed(speed as u64)
                        } else {
                            "—".into()
                        },
                        None,
                    )),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(div().w(px(92.)).flex_shrink_0().child(stat_tile(
                        "TIME LEFT",
                        eta.unwrap_or_else(|| "—".into()),
                        None,
                    )))
                    .child(div().w(px(76.)).flex_shrink_0().child(stat_tile(
                        "RESUME",
                        match row.resumable {
                            Some(true) => "YES".into(),
                            Some(false) => "NO".into(),
                            None => "—".into(),
                        },
                        None,
                    )))
                    .child(stat_tile("SAVED TO", folder, None)),
            )
            .child(connections_tile(
                if live { row.connections } else { 0 },
                want,
                server_limited,
            ));

        let percent = format!("{:.1}%", frac * 100.);
        panel
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(16.))
                    .pt(px(14.))
                    .pb(px(12.))
                    .gap(px(12.))
                    .child(
                        v_flex()
                            .gap(px(6.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .line_height(px(20.))
                                    .child(display_name(&row)),
                            )
                            .child(
                                h_flex()
                                    .gap(px(8.))
                                    .child(status_badge(status))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .font_family(MONO)
                                            .text_size(px(11.))
                                            .text_color(ink::faint())
                                            .child(row.url.clone()),
                                    )
                                    .child(
                                        h_flex()
                                            .id("copy-url")
                                            .flex_shrink_0()
                                            .h(px(22.))
                                            .px(px(8.))
                                            .gap(px(4.))
                                            .rounded(px(6.))
                                            .border_1()
                                            .border_color(ink::line_strong())
                                            .cursor_pointer()
                                            .hover(|s| s.bg(ink::inset()))
                                            .child(
                                                Icon::new(if copied {
                                                    Lucide::Check
                                                } else {
                                                    Lucide::Copy
                                                })
                                                .with_size(px(11.))
                                                .text_color(ink::muted()),
                                            )
                                            .child(caps(
                                                if copied { "COPIED" } else { "COPY" },
                                                ink::muted(),
                                            ))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.run_action(RowAction::CopyUrl, id, window, cx)
                                            })),
                                    ),
                            ),
                    )
                    .when_some(row.error.clone(), |this, err| {
                        this.child(
                            v_flex()
                                .gap(px(10.))
                                .p(px(12.))
                                .rounded(px(10.))
                                .bg(ink::red().opacity(0.06))
                                .border_1()
                                .border_color(ink::red().opacity(0.35))
                                .child(
                                    h_flex()
                                        .items_start()
                                        .gap(px(8.))
                                        .child(
                                            Icon::new(Lucide::CircleAlert)
                                                .with_size(px(14.))
                                                .text_color(ink::red())
                                                .mt(px(1.)),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(px(12.))
                                                .line_height(px(17.))
                                                .text_color(ink::text())
                                                .child(err),
                                        ),
                                )
                                .child(
                                    h_flex().gap(px(8.)).child(
                                        outline_button(
                                            "restart",
                                            "Restart from scratch",
                                            Lucide::RotateCcw,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.run_action(RowAction::Restart, id, window, cx)
                                            }),
                                        ),
                                    ),
                                ),
                        )
                    })
                    .child(
                        v_flex()
                            .gap(px(6.))
                            .child(
                                h_flex()
                                    .gap(px(8.))
                                    .child(caps("SEGMENT MAP", ink::faint()))
                                    .child(div().flex_1())
                                    .child(caps(
                                        parts_label,
                                        if live { ink::accent2() } else { ink::faint() },
                                    ))
                                    .child(caps(percent, ink::text())),
                            )
                            .child(
                                canvas(
                                    |_, _, _| {},
                                    move |b, _, window, _| {
                                        paint_segments(
                                            b, &parts, status, total, frac, time, false, window,
                                        )
                                    },
                                )
                                .w_full()
                                .h(px(26.)),
                            ),
                    )
                    .child(
                        div()
                            .h(px(56.))
                            .flex_shrink_0()
                            .rounded(px(10.))
                            .bg(ink::inset())
                            .border_1()
                            .border_color(ink::line())
                            .overflow_hidden()
                            .child(
                                canvas(
                                    |_, _, _| {},
                                    move |b, _, window, cx| {
                                        paint_detail_speed(b, &history, avg, live, window, cx)
                                    },
                                )
                                .size_full(),
                            ),
                    )
                    .child(tiles)
                    .overflow_y_scrollbar(),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .px(px(12.))
                    .py(px(10.))
                    .gap(px(8.))
                    .border_t_1()
                    .border_color(ink::line())
                    .child(
                        accent_button("detail-toggle", toggle_label, Some(toggle_icon))
                            .flex_1()
                            .h(px(34.))
                            .justify_center()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let action =
                                    if this.row(id).is_some_and(|r| r.status == Status::Done) {
                                        RowAction::Open
                                    } else {
                                        RowAction::Toggle
                                    };
                                this.run_action(action, id, window, cx)
                            })),
                    )
                    .child(
                        square_button("detail-open", Lucide::ExternalLink, "Open")
                            .when(status != Status::Done, |b| b.opacity(0.35))
                            .when(status == Status::Done, |b| {
                                b.on_click(cx.listener(move |this, _, window, cx| {
                                    this.run_action(RowAction::Open, id, window, cx)
                                }))
                            }),
                    )
                    .child(
                        square_button("detail-reveal", Lucide::FolderOpen, "Show in folder")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.run_action(RowAction::Reveal, id, window, cx)
                            })),
                    )
                    .child(
                        square_button("detail-remove", Lucide::Trash, "Remove").on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.run_action(RowAction::Remove, id, window, cx)
                            }),
                        ),
                    ),
            )
    }

    fn render_stats(&self) -> impl IntoElement {
        let done: Vec<&Row> = self
            .rows
            .iter()
            .filter(|r| r.status == Status::Done)
            .collect();
        let failed = self
            .rows
            .iter()
            .filter(|r| r.status == Status::Failed)
            .count();
        let downloaded: u64 = self.rows.iter().map(|r| r.done).sum();
        let rate = if done.len() + failed == 0 {
            "—".to_string()
        } else {
            format!(
                "{:.0}%",
                done.len() as f32 / (done.len() + failed) as f32 * 100.
            )
        };
        let avg = if self.session.samples == 0 {
            0
        } else {
            (self.session.sum / self.session.samples as f64) as u64
        };

        // Bytes finished per day, for the last 14 days (UTC).
        let today = unix_now().div_euclid(86_400);
        let mut days = vec![0u64; 14];
        for r in &done {
            let ago = today - r.added.div_euclid(86_400);
            if (0..14).contains(&ago) {
                days[13 - ago as usize] += r.total;
            }
        }
        let day_labels: Vec<String> = (0..14)
            .map(|i| {
                let (_, _, d) = civil_from_days(today - 13 + i);
                if i == 13 {
                    "TODAY".to_string()
                } else {
                    d.to_string()
                }
            })
            .collect();
        let day_peak = days.iter().copied().max().unwrap_or(0);

        // Downloaded bytes by category.
        let mut by_cat: Vec<(Category, u64)> = Category::ALL
            .iter()
            .map(|c| {
                (
                    *c,
                    self.rows
                        .iter()
                        .filter(|r| Category::of(r) == *c)
                        .map(|r| r.done)
                        .sum(),
                )
            })
            .filter(|(_, b)| *b > 0)
            .collect();
        by_cat.sort_by(|a, b| b.1.cmp(&a.1));
        let cat_total: u64 = by_cat.iter().map(|c| c.1).sum();
        let slices: Vec<(f32, Hsla)> = by_cat
            .iter()
            .enumerate()
            .map(|(i, (_, b))| (*b as f32 / cat_total.max(1) as f32, series_color(i)))
            .collect();

        let conns: Vec<u64> = self.connections.iter().map(|c| *c as u64).collect();
        let conn_peak = conns.iter().copied().max().unwrap_or(0);
        let conn_now = conns.last().copied().unwrap_or(0);
        // The chart spans what's been seen so far, at least 30 s, at most the 2 min kept.
        let conn_slots = conns.len().clamp(CONN_MIN_SLOTS, CONN_SLOTS);
        let conn_span = (conn_slots as f32 * POLL.as_secs_f32()).round() as u64;
        let days2 = days.clone();

        v_flex()
            .flex_1()
            .min_h_0()
            .gap(px(12.))
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(big_tile(
                        "TOTAL DOWNLOADED",
                        format_bytes(downloaded),
                        "ACROSS ALL DOWNLOADS",
                    ))
                    .child(big_tile("FILES", done.len().to_string(), "FINISHED"))
                    .child(big_tile(
                        "PEAK SPEED",
                        format_speed(self.session.peak),
                        "THIS SESSION",
                    ))
                    .child(big_tile(
                        "AVG SPEED",
                        format_speed(avg),
                        "WHILE DOWNLOADING",
                    ))
                    .child(big_tile("SUCCESS RATE", rate, &format!("{failed} FAILED"))),
            )
            .child(
                h_flex()
                    .gap(px(12.))
                    .h(px(186.))
                    .items_stretch()
                    .child(
                        card_panel()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex()
                                    .child(caps("DATA PER DAY", ink::faint()))
                                    .child(div().flex_1())
                                    .child(caps(
                                        format!("PEAK {}", format_bytes(day_peak)),
                                        ink::faint(),
                                    )),
                            )
                            .child(
                                div().flex_1().w_full().child(
                                    canvas(
                                        |_, _, _| {},
                                        move |b, _, window, cx| {
                                            paint_day_bars(b, &days2, window, cx)
                                        },
                                    )
                                    .size_full(),
                                ),
                            )
                            .child(h_flex().justify_between().children(
                                day_labels.into_iter().map(|l| {
                                    div()
                                        .flex_1()
                                        .flex()
                                        .justify_center()
                                        .child(caps(l, ink::faint()))
                                }),
                            )),
                    )
                    .child(
                        card_panel()
                            .w(px(420.))
                            .child(caps("BY CATEGORY", ink::faint()))
                            .child(
                                h_flex()
                                    .flex_1()
                                    .gap(px(20.))
                                    .child(
                                        div()
                                            .relative()
                                            .size(px(124.))
                                            .flex_shrink_0()
                                            .child(
                                                canvas(
                                                    |_, _, _| {},
                                                    move |b, _, window, _| {
                                                        paint_donut(b, &slices, window)
                                                    },
                                                )
                                                .size_full(),
                                            )
                                            .child(
                                                v_flex()
                                                    .absolute()
                                                    .inset_0()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(
                                                        div()
                                                            .font_family(MONO)
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_size(px(15.))
                                                            .child(format_bytes(cat_total)),
                                                    )
                                                    .child(caps("TOTAL", ink::faint())),
                                            ),
                                    )
                                    .child(v_flex().flex_1().min_w_0().gap(px(6.)).children(
                                        by_cat.iter().enumerate().map(|(i, (c, b))| {
                                            h_flex()
                                                .h(px(22.))
                                                .px(px(8.))
                                                .gap(px(8.))
                                                .rounded(px(6.))
                                                .bg(ink::inset())
                                                .child(
                                                    div()
                                                        .size(px(8.))
                                                        .rounded(px(2.))
                                                        .bg(series_color(i)),
                                                )
                                                .child(caps(c.label(), ink::muted()))
                                                .child(div().flex_1())
                                                .child(
                                                    div()
                                                        .font_family(MONO)
                                                        .text_size(px(10.5))
                                                        .text_color(ink::text())
                                                        .child(format!(
                                                            "{:.0}%",
                                                            *b as f32 / cat_total.max(1) as f32
                                                                * 100.
                                                        )),
                                                )
                                        }),
                                    )),
                            ),
                    ),
            )
            .child(
                card_panel()
                    .h(px(124.))
                    .child(
                        h_flex()
                            .child(caps("CONNECTIONS IN USE", ink::faint()))
                            .child(div().flex_1())
                            .child(caps(
                                format!("NOW {conn_now} · PEAK {conn_peak}"),
                                ink::faint(),
                            )),
                    )
                    .child(
                        div().flex_1().w_full().child(
                            canvas(
                                |_, _, _| {},
                                move |b, _, window, cx| {
                                    paint_steps(b, &conns, conn_slots, window, cx)
                                },
                            )
                            .size_full(),
                        ),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .child(caps(format!("-{}S", conn_span), ink::faint()))
                            .child(caps(format!("-{}S", conn_span / 2), ink::faint()))
                            .child(caps("NOW", ink::faint())),
                    ),
            )
            .overflow_y_scrollbar()
    }

    fn render_status_bar(&self) -> impl IntoElement {
        let speed: u64 = self
            .rows
            .iter()
            .filter(|r| r.status == Status::Downloading)
            .map(|r| r.speed)
            .sum();
        let running = self
            .rows
            .iter()
            .filter(|r| r.status == Status::Downloading)
            .count();
        let queued = self
            .rows
            .iter()
            .filter(|r| r.status == Status::Queued)
            .count();
        let cap = self.settings.speed_limit;
        let sep = || div().text_color(ink::line_strong()).child("·");
        h_flex()
            .flex_shrink_0()
            .h(px(28.))
            .px(px(16.))
            .gap(px(8.))
            .border_t_1()
            .border_color(ink::line())
            .bg(ink::bg())
            .font_family(MONO)
            .text_size(px(11.))
            .text_color(ink::faint())
            .child(
                div()
                    .text_color(if speed > 0 {
                        ink::accent2()
                    } else {
                        ink::faint()
                    })
                    .child(format!("↓ {}", format_speed(speed))),
            )
            .child(sep())
            .child(if cap > 0 {
                format!("CAP {}", format_speed(cap))
            } else {
                "NO CAP".into()
            })
            .child(sep())
            .child(format!("{running} running"))
            .child(sep())
            .child(format!("{queued} queued"))
            .child(sep())
            .child(div().min_w_0().truncate().child(tilde(&self.settings.dir)))
            .child(div().flex_1())
            .child(format!("{} downloads", self.rows.len()))
            .child(div().w(px(12.)))
            .child(format!("v{}", crate::update::VERSION))
    }

    fn render_limits(&self, cx: &Context<Self>) -> impl IntoElement {
        let cap = self.settings.speed_limit;
        let frac = (cap as f32 / CAP_MAX as f32).clamp(0., 1.);
        let toggle = |id: &'static str, label: &'static str, hint: &'static str, on: bool, set: fn(&mut Settings, bool)| {
            h_flex()
                .p(px(12.))
                .gap(px(12.))
                .rounded(px(10.))
                .bg(ink::inset())
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(2.))
                        .child(div().text_size(px(13.)).child(label))
                        .child(caps(hint, ink::faint())),
                )
                .child(
                    div()
                        .id(id)
                        .w(px(34.))
                        .h(px(20.))
                        .p(px(2.))
                        .rounded_full()
                        .cursor_pointer()
                        .bg(if on { ink::accent() } else { ink::line_strong() })
                        .child(div().size(px(16.)).rounded_full().bg(ink::text()).when(on, |knob| knob.ml(px(14.))))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_settings(|s| set(s, !on), cx);
                        })),
                )
        };
        let stepper = |id: &'static str,
                       label: &'static str,
                       hint: &'static str,
                       value: usize,
                       min: usize,
                       max: usize,
                       set: fn(&mut Settings, usize)| {
            h_flex()
                .p(px(12.))
                .gap(px(12.))
                .rounded(px(10.))
                .bg(ink::inset())
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(2.))
                        .child(div().text_size(px(13.)).child(label))
                        .child(caps(hint, ink::faint())),
                )
                .child(
                    h_flex()
                        .h(px(28.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(ink::line_strong())
                        .child(
                            step_button((id, 0), "−", value > min).when(value > min, |b| {
                                b.on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_settings(|s| set(s, value - 1), cx);
                                }))
                            }),
                        )
                        .child(
                            div()
                                .w(px(36.))
                                .flex()
                                .justify_center()
                                .font_family(MONO)
                                .text_size(px(13.))
                                .child(value.to_string()),
                        )
                        .child(
                            step_button((id, 1), "+", value < max).when(value < max, |b| {
                                b.on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_settings(|s| set(s, value + 1), cx);
                                }))
                            }),
                        ),
                )
        };

        div()
            .absolute()
            .inset_0()
            .child(
                // Clicking outside closes the panel.
                div()
                    .id("limits-backdrop")
                    .absolute()
                    .inset_0()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.dismiss(window, cx)),
                    ),
            )
            .child(
                v_flex()
                    .id("limits")
                    .absolute()
                    .top(px(56.))
                    .right(px(14.))
                    .w(px(372.))
                    .p(px(14.))
                    .gap(px(10.))
                    .rounded(px(16.))
                    .bg(ink::panel())
                    .border_1()
                    .border_color(ink::line_strong())
                    .shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.5),
                        offset: point(px(0.), px(16.)),
                        blur_radius: px(40.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .max_h(relative(0.88))
                    .overflow_y_scroll()
                    .occlude()
                    .key_context(PANEL)
                    .track_focus(&self.panel_focus)
                    .on_action(
                        cx.listener(|this, _: &Dismiss, window, cx| this.dismiss(window, cx)),
                    )
                    .child(corner_glow(cap > 0, 120.))
                    .child(
                        h_flex()
                            .h(px(24.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Limits"),
                            )
                            .child(div().flex_1())
                            .child(caps(
                                if cap > 0 { "LIMITS ON" } else { "NO LIMITS" },
                                if cap > 0 {
                                    ink::accent2()
                                } else {
                                    ink::faint()
                                },
                            )),
                    )
                    .child(
                        v_flex()
                            .p(px(12.))
                            .gap(px(10.))
                            .rounded(px(10.))
                            .bg(ink::inset())
                            .child(
                                h_flex()
                                    .child(div().text_size(px(13.)).child("Speed cap"))
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .font_family(MONO)
                                            .text_size(px(13.))
                                            .text_color(if cap > 0 {
                                                ink::text()
                                            } else {
                                                ink::muted()
                                            })
                                            .child(if cap > 0 {
                                                format_speed(cap)
                                            } else {
                                                "Off".into()
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("cap-track")
                                    .relative()
                                    .h(px(18.))
                                    .cursor_pointer()
                                    .child(
                                        div()
                                            .absolute()
                                            .left_0()
                                            .right_0()
                                            .top(px(7.))
                                            .h(px(4.))
                                            .rounded_full()
                                            .bg(ink::line_strong()),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .left_0()
                                            .top(px(7.))
                                            .h(px(4.))
                                            .w(relative(frac))
                                            .rounded_full()
                                            .bg(linear_gradient(
                                                90.,
                                                linear_color_stop(ink::accent(), 0.),
                                                linear_color_stop(ink::accent2(), 1.),
                                            )),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(1.))
                                            .left(relative(frac))
                                            .ml(px(-8.))
                                            .size(px(16.))
                                            .rounded_full()
                                            .bg(ink::text())
                                            .border_2()
                                            .border_color(ink::inset())
                                            .shadow(glow(
                                                ink::accent(),
                                                if cap > 0 { 0.5 } else { 0. },
                                                10.,
                                            )),
                                    )
                                    .child({
                                        let track = self.cap_track.clone();
                                        canvas(move |b, _, _| track.set(b), |_, _, _, _| {})
                                            .absolute()
                                            .inset_0()
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, e: &MouseDownEvent, _, cx| {
                                            let track = this.cap_track.get();
                                            if track.size.width > px(0.) {
                                                this.set_cap_from(e.position.x, track, cx);
                                            }
                                        }),
                                    )
                                    .on_drag(DragCap, |_, _, _, cx| cx.new(|_| DragCap))
                                    .on_drag_move(cx.listener(
                                        |this, e: &DragMoveEvent<DragCap>, _, cx| {
                                            this.set_cap_from(e.event.position.x, e.bounds, cx);
                                        },
                                    )),
                            )
                            .child(
                                h_flex()
                                    .justify_between()
                                    .child(caps("0 · OFF", ink::faint()))
                                    .child(caps("50 MB/S", ink::faint())),
                            ),
                    )
                    .child(stepper(
                        "max-active",
                        "Simultaneous downloads",
                        "1 TO 10",
                        self.settings.max_active,
                        1,
                        10,
                        |s, v| s.max_active = v,
                    ))
                    .child(stepper(
                        "connections",
                        "Connections per download",
                        "1 TO 32",
                        self.settings.connections,
                        1,
                        32,
                        |s, v| s.connections = v,
                    ))
                    .child(toggle(
                        "keep-awake",
                        "Keep the computer awake",
                        "WHILE ANYTHING DOWNLOADS",
                        self.settings.keep_awake,
                        |s, on| s.keep_awake = on,
                    ))
                    .child(toggle(
                        "launch-at-login",
                        "Launch at login",
                        "STARTS IN THE TRAY",
                        self.settings.launch_at_login,
                        |s, on| s.launch_at_login = on,
                    ))
                    .child(toggle("notify", "Notify when done", "OR WHEN ONE FAILS", self.settings.notify, |s, on| s.notify = on))
                    .child(toggle(
                        "watch-clipboard",
                        "Offer copied links",
                        "WHEN A FILE LINK IS COPIED",
                        self.settings.watch_clipboard,
                        |s, on| s.watch_clipboard = on,
                    ))
                    .child(toggle("ask-where", "Ask where to save", "EACH LINK ADDED HERE", self.settings.ask_where, |s, on| s.ask_where = on))
                    .child(toggle(
                        "sort-into-folders",
                        "Sort into folders",
                        "VIDEO, MUSIC, ARCHIVES…",
                        self.settings.sort_into_folders,
                        |s, on| s.sort_into_folders = on,
                    ))
                    .child(toggle(
                        "auto-update",
                        "Update automatically",
                        "DOWNLOADS, THEN ASKS TO RESTART",
                        self.settings.auto_update,
                        |s, on| s.auto_update = on,
                    ))
                    .child(
                        h_flex()
                            .px(px(12.))
                            .gap(px(8.))
                            .child(div().flex_1().min_w_0().truncate().child(caps(self.update_line(), ink::faint())))
                            .child(ghost_button("check-updates", "Check now").on_click(cx.listener(|_, _, _, cx| {
                                if let Some(shell) = cx.try_global::<Shell>() {
                                    shell.updater.check_now();
                                }
                            }))),
                    )
                    .child(self.render_networks(cx))
                    .child(self.render_schedule(cx))
                    .child(
                        v_flex()
                            .p(px(12.))
                            .gap(px(8.))
                            .rounded(px(10.))
                            .bg(ink::inset())
                            .child(div().text_size(px(13.)).child("Download folder"))
                            .child(
                                h_flex()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .h(px(30.))
                                            .px(px(10.))
                                            .flex()
                                            .items_center()
                                            .rounded(px(8.))
                                            .bg(ink::panel())
                                            .border_1()
                                            .border_color(if self.limits_error.is_some() {
                                                ink::red().opacity(0.45)
                                            } else {
                                                ink::line_strong()
                                            })
                                            .child(
                                                Input::new(&self.folder)
                                                    .appearance(false)
                                                    .font_family(MONO)
                                                    .text_size(px(12.))
                                                    .h(px(26.))
                                                    .px_0(),
                                            ),
                                    )
                                    .child(
                                        outline_button("browse", "Browse", Lucide::FolderOpen)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.browse(window, cx)
                                            })),
                                    ),
                            )
                            .when_some(self.limits_error.clone(), |this, err| {
                                this.child(
                                    h_flex()
                                        .items_start()
                                        .gap(px(6.))
                                        .text_size(px(12.))
                                        .text_color(ink::red())
                                        .child(
                                            Icon::new(Lucide::CircleAlert)
                                                .with_size(px(13.))
                                                .mt(px(1.)),
                                        )
                                        .child(div().flex_1().min_w_0().child(err)),
                                )
                            }),
                    )
                    .child(caps("CHANGES APPLY RIGHT AWAY", ink::faint())),
            )
    }

    /// "Restart to update" once an update is ready; "Updating 42%" while it comes down.
    fn render_update_pill(&self, cx: &Context<Self>) -> Option<impl IntoElement + use<>> {
        use crate::update::State;
        match &self.update {
            State::Ready { version } => Some(
                h_flex()
                    .id("update-pill")
                    .h(px(28.))
                    .px(px(12.))
                    .gap(px(6.))
                    .rounded_full()
                    .bg(ink::accent().opacity(0.14))
                    .border_1()
                    .border_color(ink::accent().opacity(0.6))
                    .cursor_pointer()
                    .hover(|s| s.bg(ink::accent().opacity(0.22)))
                    .child(dot(ink::accent(), 6.))
                    .child(caps(format!("RESTART TO UPDATE · {version}"), ink::accent2()))
                    .on_click(cx.listener(|_, _, _, cx| restart_to_update(cx))),
            ),
            State::Downloading { done, total, .. } if *total > 0 => Some(
                h_flex()
                    .id("update-pill")
                    .h(px(28.))
                    .px(px(12.))
                    .rounded_full()
                    .border_1()
                    .border_color(ink::line_strong())
                    .child(caps(format!("UPDATING {:.0}%", *done as f64 * 100. / *total as f64), ink::faint())),
            ),
            _ => None,
        }
    }

    /// The update state, as one line for the Limits panel.
    fn update_line(&self) -> String {
        use crate::update::State;
        let v = crate::update::VERSION;
        match &self.update {
            State::Idle => format!("V{v} · UP TO DATE"),
            State::Checking => format!("V{v} · CHECKING…"),
            State::Downloading { version, done, total } => {
                format!("DOWNLOADING {version} · {}%", if *total > 0 { done * 100 / total } else { 0 })
            }
            State::Ready { version } => format!("{version} READY · RESTART TO INSTALL"),
            State::Failed(e) => format!("COULDN'T CHECK: {}", e.to_uppercase()),
            State::Manual(why) => why.to_uppercase(),
        }
    }

    /// While combining networks carries a download: each one's share of the speed, as a split bar.
    fn render_network_share(&self) -> Option<impl IntoElement + use<>> {
        let busy: Vec<&NetworkUse> = self.networks.iter().filter(|n| n.on && n.speed > 0).collect();
        let total: u64 = busy.iter().map(|n| n.speed).sum();
        if !self.settings.combine_networks || busy.len() < 2 || total == 0 {
            return None;
        }
        let colours = [ink::accent(), ink::white2(), ink::accent2(), ink::muted()];
        let share = |n: &NetworkUse| n.speed as f32 / total as f32;
        Some(
            v_flex()
                .w(px(260.))
                .gap(px(5.))
                .child(
                    h_flex().h(px(5.)).w_full().gap(px(2.)).children(busy.iter().enumerate().map(|(i, n)| {
                        div().h_full().rounded(px(2.)).bg(colours[i % colours.len()]).w(relative(share(n)))
                    })),
                )
                .child(h_flex().gap(px(10.)).children(busy.iter().enumerate().map(|(i, n)| {
                    h_flex()
                        .gap(px(5.))
                        .child(dot(colours[i % colours.len()], 6.))
                        .child(caps(format!("{} {:.0}%", n.network.label.to_uppercase(), share(n) * 100.), ink::faint()))
                }))),
        )
    }

    /// Combining networks in the Limits panel: the switch, then each network with its speed.
    fn render_networks(&self, cx: &Context<Self>) -> impl IntoElement {
        let on = self.settings.combine_networks;
        v_flex()
            .p(px(12.))
            .gap(px(8.))
            .rounded(px(10.))
            .bg(ink::inset())
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(
                        v_flex()
                            .flex_1()
                            .gap(px(2.))
                            .child(div().text_size(px(13.)).child("Combine networks"))
                            .child(caps("WI-FI, ETHERNET, A PHONE… AT ONCE", ink::faint())),
                    )
                    .child(switch("combine-networks", on).on_click(cx.listener(move |this, _, _, cx| {
                        this.set_settings(|s| s.combine_networks = !on, cx);
                    }))),
            )
            .when(on, |this| {
                this.children(self.networks.iter().enumerate().map(|(i, n)| {
                    let (name, used) = (n.network.name.clone(), n.on);
                    h_flex()
                        .gap(px(10.))
                        .pt(px(6.))
                        .border_t_1()
                        .border_color(ink::line())
                        .child(dot(if used { ink::accent() } else { ink::faint() }, 6.))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().text_size(px(12.5)).child(n.network.label.clone()))
                                .child(caps(format!("{} · {}", n.network.name, n.network.ip), ink::faint())),
                        )
                        .child(caps(if n.speed > 0 { format_speed(n.speed) } else { "—".into() }, ink::muted()))
                        .child(switch(("network", i), used).on_click(cx.listener(move |this, _, _, cx| {
                            let name = name.clone();
                            this.set_settings(
                                move |s| {
                                    if used {
                                        s.networks_off.push(name);
                                    } else {
                                        s.networks_off.retain(|n| *n != name);
                                    }
                                },
                                cx,
                            );
                        })))
                }))
                .when(self.networks.len() < 2, |this| {
                    this.child(caps("CONNECT A SECOND NETWORK TO COMBINE", ink::faint()))
                })
            })
    }

    /// The scheduler's part of the Limits panel: when to start, when to stop, what to do after.
    fn render_schedule(&self, cx: &Context<Self>) -> impl IntoElement {
        let time = |label: &'static str, input: &Entity<InputState>| {
            h_flex()
                .flex_1()
                .gap(px(8.))
                .child(caps(label, ink::faint()))
                .child(
                    div()
                        .w(px(64.))
                        .h(px(28.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .rounded(px(7.))
                        .bg(ink::panel())
                        .border_1()
                        .border_color(ink::line_strong())
                        .child(Input::new(input).appearance(false).font_family(MONO).text_size(px(12.)).h(px(24.)).px_0()),
                )
        };
        const AFTER: [(WhenDone, &str); 3] = [(WhenDone::Nothing, "NOTHING"), (WhenDone::Sleep, "SLEEP"), (WhenDone::ShutDown, "SHUT DOWN")];
        v_flex()
            .p(px(12.))
            .gap(px(10.))
            .rounded(px(10.))
            .bg(ink::inset())
            .child(
                v_flex()
                    .gap(px(2.))
                    .child(div().text_size(px(13.)).child("Schedule"))
                    .child(caps("RESUMES AND PAUSES THE QUEUE · 24-HOUR TIMES", ink::faint())),
            )
            .child(h_flex().gap(px(10.)).child(time("START", &self.start_at)).child(time("STOP", &self.stop_at)))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(caps("WHEN DONE", ink::faint()))
                    .child(div().flex_1())
                    .children(AFTER.iter().enumerate().map(|(i, &(when, label))| {
                        chip(("when-done", i), label, None, self.settings.when_done == when, true)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_settings(|s| s.when_done = when, cx);
                            }))
                    })),
            )
    }

    fn render_refresh(&self, cx: &Context<Self>) -> impl IntoElement {
        let Some(modal) = &self.refresh else {
            return div();
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("refresh-backdrop")
                    .absolute()
                    .inset_0()
                    .bg(hsla(0., 0., 0., 0.62))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.dismiss(window, cx))),
            )
            .child(
                v_flex()
                    .id("refresh")
                    .relative()
                    .w(px(460.))
                    .p(px(20.))
                    .gap(px(14.))
                    .rounded(px(16.))
                    .bg(ink::panel())
                    .border_1()
                    .border_color(ink::line_strong())
                    .occlude()
                    .key_context(MODAL)
                    .on_action(cx.listener(|this, _: &Confirm, window, cx| this.confirm_refresh(window, cx)))
                    .on_action(cx.listener(|this, _: &Dismiss, window, cx| this.dismiss(window, cx)))
                    .child(
                        v_flex()
                            .gap(px(6.))
                            .child(caps("REFRESH LINK", ink::faint()))
                            .child(div().text_size(px(15.)).font_weight(FontWeight::MEDIUM).line_clamp(2).child(modal.name.clone())),
                    )
                    .child(div().text_size(px(13.)).text_color(ink::muted()).child(
                        "Links from many sites expire. Paste a fresh one for the same file: what's downloaded is kept when its size matches.",
                    ))
                    .child(
                        div()
                            .h(px(34.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(8.))
                            .bg(ink::inset())
                            .border_1()
                            .border_color(if modal.error.is_some() { ink::red().opacity(0.45) } else { ink::line_strong() })
                            .child(Input::new(&modal.input).appearance(false).font_family(MONO).text_size(px(12.)).h(px(28.)).px_0()),
                    )
                    .when_some(modal.error.clone(), |this, err| {
                        this.child(div().text_size(px(12.)).text_color(ink::red()).child(err))
                    })
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .justify_end()
                            .child(caps("ESC", ink::faint()))
                            .child(ghost_button("refresh-cancel", "Cancel").on_click(cx.listener(|this, _, window, cx| this.dismiss(window, cx))))
                            .child(
                                accent_button("refresh-ok", "Use this link", Some(Lucide::RotateCcw))
                                    .h(px(32.))
                                    .on_click(cx.listener(|this, _, window, cx| this.confirm_refresh(window, cx))),
                            ),
                    ),
            )
    }

    fn render_remove(&self, cx: &Context<Self>) -> impl IntoElement {
        let Some(modal) = &self.remove else {
            return div();
        };
        let checked = modal.delete_file;
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("remove-backdrop")
                    .absolute()
                    .inset_0()
                    .bg(hsla(0., 0., 0., 0.62))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.dismiss(window, cx)),
                    ),
            )
            .child(
                v_flex()
                    .id("remove")
                    .relative()
                    .w(px(420.))
                    .p(px(20.))
                    .gap(px(16.))
                    .rounded(px(16.))
                    .bg(ink::panel())
                    .border_1()
                    .border_color(ink::line_strong())
                    .occlude()
                    .key_context(MODAL)
                    .track_focus(&self.modal_focus)
                    .on_action(
                        cx.listener(|this, _: &Confirm, window, cx| {
                            this.confirm_remove(window, cx)
                        }),
                    )
                    .on_action(
                        cx.listener(|this, _: &Dismiss, window, cx| this.dismiss(window, cx)),
                    )
                    .child(
                        v_flex()
                            .gap(px(6.))
                            .child(caps("REMOVE DOWNLOAD", ink::faint()))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .line_clamp(2)
                                    .child(modal.name.clone()),
                            ),
                    )
                    .child(div().text_size(px(13.)).text_color(ink::muted()).child(
                        "It leaves the queue. The file stays where it is unless you tick the box.",
                    ))
                    .child(
                        h_flex()
                            .id("delete-file")
                            .gap(px(10.))
                            .p(px(12.))
                            .rounded(px(10.))
                            .bg(ink::inset())
                            .cursor_pointer()
                            .child(
                                div()
                                    .size(px(16.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(4.))
                                    .border_1()
                                    .map(|this| {
                                        if checked {
                                            this.bg(ink::accent()).border_color(ink::accent())
                                        } else {
                                            this.border_color(ink::faint())
                                        }
                                    })
                                    .when(checked, |this| {
                                        this.child(
                                            Icon::new(Lucide::Check)
                                                .with_size(px(11.))
                                                .text_color(ink::bg()),
                                        )
                                    }),
                            )
                            .child(div().text_size(px(13.)).child(if modal.finished {
                                "Also delete the file from disk"
                            } else {
                                "Also delete the file from disk (the partial download)"
                            }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(modal) = &mut this.remove {
                                    modal.delete_file = !modal.delete_file;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .justify_end()
                            .child(caps("ESC", ink::faint()))
                            .child(ghost_button("remove-cancel", "Cancel").on_click(
                                cx.listener(|this, _, window, cx| this.dismiss(window, cx)),
                            ))
                            .child(
                                h_flex()
                                    .id("remove-ok")
                                    .h(px(32.))
                                    .px(px(14.))
                                    .gap(px(6.))
                                    .rounded(px(8.))
                                    .bg(ink::red().opacity(0.14))
                                    .border_1()
                                    .border_color(ink::red().opacity(0.5))
                                    .text_color(ink::red())
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .cursor_pointer()
                                    .hover(|s| s.bg(ink::red().opacity(0.22)))
                                    .child(Icon::new(Lucide::Trash).with_size(px(13.)))
                                    .child(if checked {
                                        "Remove and delete"
                                    } else {
                                        "Remove"
                                    })
                                    .child(div().ml(px(4.)).child(kbd("↵")))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.confirm_remove(window, cx)
                                    })),
                            ),
                    ),
            )
    }
}

/// What the cap slider's drag carries; it draws nothing.
#[derive(Clone)]
struct DragCap;

impl Render for DragCap {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

fn row_menu(menu: PopupMenu, view: &WeakEntity<IdmView>, id: Id, cx: &App) -> PopupMenu {
    let Some(row) = view.upgrade().and_then(|v| v.read(cx).row(id).cloned()) else {
        return menu;
    };
    let item = |label: &'static str, icon: Lucide, action: RowAction, enabled: bool| {
        let view = view.clone();
        PopupMenuItem::new(label)
            .icon(Icon::new(icon))
            .disabled(!enabled)
            .on_click(move |_, window, cx| {
                view.update(cx, |view, cx| view.run_action(action, id, window, cx))
                    .ok();
            })
    };
    let resumable = matches!(row.status, Status::Paused | Status::Failed);
    menu.item(item(
        "Open",
        Lucide::ExternalLink,
        RowAction::Open,
        row.status == Status::Done && row.path.is_some(),
    ))
    .item(item(
        "Show in folder",
        Lucide::FolderOpen,
        RowAction::Reveal,
        true,
    ))
    .separator()
    .item(item(
        if resumable { "Resume" } else { "Pause" },
        if resumable {
            Lucide::Play
        } else {
            Lucide::Pause
        },
        RowAction::Toggle,
        row.status != Status::Done,
    ))
    .when(row.status == Status::Failed, |menu| {
        menu.item(item(
            "Restart from scratch",
            Lucide::RotateCcw,
            RowAction::Restart,
            true,
        ))
    })
    .item(item(
        "Refresh link…",
        Lucide::Link,
        RowAction::RefreshLink,
        matches!(row.status, Status::Paused | Status::Failed | Status::Queued),
    ))
    .when(row.status == Status::Queued, |menu| {
        menu.separator()
            .item(item("Move to top", Lucide::Play, RowAction::Move(Move::Top), row.position > 1))
            .item(item("Move up", Lucide::Play, RowAction::Move(Move::Up), row.position > 1))
            .item(item("Move down", Lucide::Pause, RowAction::Move(Move::Down), true))
            .item(item("Move to bottom", Lucide::Pause, RowAction::Move(Move::Bottom), true))
    })
    .separator()
    .item(item("Copy URL", Lucide::Copy, RowAction::CopyUrl, true))
    .item(item("Remove…", Lucide::Trash, RowAction::Remove, true))
}

// ---------------------------------------------------------------------------------------------
// Building blocks

const GRAPH_SERVER_W: f32 = 124.;
/// The part nodes sit midway between the server and file nodes, not at the graph's centre.
const GRAPH_SHIFT: f32 = (GRAPH_SERVER_W - GRAPH_FILE_W) / 2.;
const GRAPH_NODE_W: f32 = 84.;
const GRAPH_FILE_W: f32 = 60.;
const GRAPH_FILE_TILE: f32 = 40.;

/// Small uppercase mono label, tracked wide (GPUI has no letter spacing, so each glyph is its
/// own box with a gap).
fn caps(text: impl AsRef<str>, color: Hsla) -> Div {
    h_flex()
        .flex_shrink_0()
        .gap(px(1.2))
        .font_family(MONO)
        .text_size(px(10.5))
        .line_height(px(13.))
        .text_color(color)
        .children(text.as_ref().chars().map(|ch| {
            if ch == ' ' {
                div().w(px(3.))
            } else {
                div().child(ch.to_string())
            }
        }))
}

fn kbd(text: &'static str) -> impl IntoElement {
    div()
        .h(px(18.))
        .px(px(5.))
        .flex()
        .items_center()
        .rounded(px(5.))
        .border_1()
        .border_color(ink::line_strong())
        .font_family(MONO)
        .text_size(px(10.))
        .text_color(ink::faint())
        .child(text)
}

fn glow(color: Hsla, alpha: f32, blur: f32) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: color.opacity(alpha),
        offset: point(px(0.), px(0.)),
        blur_radius: px(blur),
        spread_radius: px(blur / 6.),
        inset: false,
    }]
}

/// A soft accent glow rising from a card's bottom-left: a blurred blob kept far enough from the
/// rounded corner that the (rectangular) clip never shows.
fn corner_glow(on: bool, height: f32) -> impl IntoElement {
    let (w, left, alpha, blur) = if height > 100. {
        (220., 70., 0.20, 90.)
    } else {
        (90., 46., 0.26, 34.)
    };
    div()
        .absolute()
        .left(px(left))
        .top(px(height - 4.))
        .w(px(w))
        .h(px(12.))
        .when(on, |this| this.shadow(glow(ink::accent(), alpha, blur)))
}

fn vrule() -> impl IntoElement {
    div().w(px(1.)).h_full().flex_shrink_0().bg(ink::line())
}

fn dot(color: Hsla, size: f32) -> Div {
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded_full()
        .bg(color)
}

/// Shard's mark: the display font's S cut in three, the middle slice slid along like a part
/// mid-download. The outline is `crate::logo`'s, made by scripts/logo.py.
fn mark(scale: f32) -> impl IntoElement {
    use crate::logo::{BANDS, S, SHIFT};
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            let k = b.size.width.as_f32() / 100.;
            let (x0, y0) = (b.left().as_f32(), b.top().as_f32());
            for (i, &(top, bottom)) in BANDS.iter().enumerate() {
                let (shift, fill): (f32, Background) = if i == 1 {
                    (SHIFT, linear_gradient(90., linear_color_stop(ink::accent(), 0.), linear_color_stop(ink::accent2(), 1.)))
                } else {
                    (0., linear_gradient(180., linear_color_stop(ink::accent2(), 0.), linear_color_stop(ink::accent(), 1.)))
                };
                let band = Bounds {
                    origin: point(px(x0 - 10.), px(y0 + top * k)),
                    size: size(px(b.size.width.as_f32() + 20.), px((bottom - top) * k)),
                };
                window.with_content_mask(Some(ContentMask { bounds: band }), |window| {
                    for contour in S {
                        let mut path = PathBuilder::fill();
                        path.move_to(point(px(x0 + (contour[0].0 + shift) * k), px(y0 + contour[0].1 * k)));
                        for &(x, y) in &contour[1..] {
                            path.line_to(point(px(x0 + (x + shift) * k), px(y0 + y * k)));
                        }
                        path.close();
                        if let Ok(path) = path.build() {
                            window.paint_path(path, fill);
                        }
                    }
                });
            }
        },
    )
    .size(px(18. * scale))
    .flex_shrink_0()
}

fn wordmark() -> impl IntoElement {
    h_flex().flex_shrink_0().gap(px(10.)).child(mark(1.)).child(
        div()
            .font_family(DISPLAY)
            .font_weight(FontWeight::EXTRA_BOLD)
            .text_size(px(15.))
            .text_color(ink::text())
            .child("SHARD"),
    )
}

fn type_tile(ext: &str, size: f32, live: bool) -> impl IntoElement {
    div()
        .size(px(size))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(10.))
        .bg(ink::inset())
        .border_1()
        .border_color(if live {
            ink::accent().opacity(0.35)
        } else {
            ink::line_strong()
        })
        .font_family(MONO)
        .text_size(px(if ext.len() > 3 { 10. } else { 11. }))
        .text_color(if live { ink::accent2() } else { ink::muted() })
        .child(ext.to_string())
}

fn state_dot(status: Status, id: Id) -> AnyElement {
    match status {
        Status::Downloading => div()
            .size(px(7.))
            .flex_shrink_0()
            .rounded_full()
            .bg(ink::accent())
            .shadow(glow(ink::accent(), 0.7, 6.))
            .with_animation(
                ("pulse", id as u64),
                Animation::new(Duration::from_millis(1400)).repeat(),
                |this, t| {
                    this.opacity(0.45 + 0.55 * (0.5 + 0.5 * (t * std::f32::consts::TAU).cos()))
                },
            )
            .into_any_element(),
        Status::Queued => dot(ink::white2(), 7.).into_any_element(),
        Status::Paused => div()
            .size(px(7.))
            .flex_shrink_0()
            .rounded_full()
            .border_1()
            .border_color(ink::muted())
            .into_any_element(),
        Status::Done => Icon::new(Lucide::Check)
            .with_size(px(11.))
            .text_color(ink::faint())
            .into_any_element(),
        Status::Failed => dot(ink::red(), 7.).into_any_element(),
    }
}

fn status_word(status: Status) -> &'static str {
    match status {
        Status::Downloading => "DOWNLOADING",
        Status::Queued => "QUEUED",
        Status::Paused => "PAUSED",
        Status::Done => "COMPLETE",
        Status::Failed => "FAILED",
    }
}

fn status_badge(status: Status) -> impl IntoElement {
    let color = match status {
        Status::Downloading => ink::accent2(),
        Status::Failed => ink::red(),
        _ => ink::muted(),
    };
    h_flex()
        .h(px(22.))
        .px(px(9.))
        .gap(px(6.))
        .rounded_full()
        .border_1()
        .border_color(match status {
            Status::Downloading => ink::accent().opacity(0.4),
            Status::Failed => ink::red().opacity(0.4),
            _ => ink::line_strong(),
        })
        .child(state_dot(status, -1))
        .child(caps(status_word(status), color))
}

fn state_label(row: &Row, frac: f32) -> AnyElement {
    let (text, color) = match row.status {
        Status::Downloading if row.total > 0 => (format!("{:.0}%", frac * 100.), ink::text()),
        Status::Downloading => (format_bytes(row.done), ink::text()),
        Status::Queued => ("QUEUED".into(), ink::faint()),
        Status::Paused => (format!("PAUSED {:.0}%", frac * 100.), ink::muted()),
        Status::Done => ("DONE".into(), ink::faint()),
        Status::Failed => ("FAILED".into(), ink::red()),
    };
    if row.status == Status::Downloading {
        div()
            .font_family(MONO)
            .text_size(px(12.))
            .text_color(color)
            .child(text)
            .into_any_element()
    } else {
        caps(text, color).into_any_element()
    }
}

fn meta_line(row: &Row, speed: f32) -> String {
    let size = |done: u64| {
        if row.total > 0 {
            format!("{} of {}", format_bytes(done), format_bytes(row.total))
        } else {
            format_bytes(done)
        }
    };
    match row.status {
        Status::Downloading => {
            let mut parts = vec![size(row.done), format_speed(speed as u64)];
            if row.total > row.done && row.speed > 0 {
                parts.push(format!(
                    "{} left",
                    format_duration((row.total - row.done) / row.speed)
                ));
            }
            if row.total == 0 {
                parts.push("stream".into());
            }
            parts.push(format!("{} conn", row.connections));
            parts.join(" · ")
        }
        Status::Queued if row.total == 0 => format!("#{} in queue · waiting for a free slot", row.position.max(1)),
        Status::Queued => format!("#{} in queue · {}", row.position.max(1), format_bytes(row.total)),
        Status::Paused if row.resumable == Some(false) => {
            format!("{} · can't resume, restarts", size(row.done))
        }
        Status::Paused => format!("{} · paused", size(row.done)),
        Status::Done => format!(
            "{} · {}",
            format_bytes(row.total.max(row.done)),
            format_added(row.added, unix_now())
        ),
        Status::Failed => row.error.clone().unwrap_or_else(|| "Failed".into()),
    }
}

fn chip(
    id: (&'static str, usize),
    label: &'static str,
    count: Option<usize>,
    on: bool,
    compact: bool,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(26.))
        .px(px(if compact { 8. } else { 10. }))
        .gap(px(6.))
        .rounded(px(6.))
        .border_1()
        .cursor_pointer()
        .map(|this| {
            if on {
                this.bg(ink::inset()).border_color(ink::line_strong())
            } else {
                this.border_color(ink::line()).hover(|s| s.bg(ink::panel()))
            }
        })
        .child(caps(label, if on { ink::text() } else { ink::muted() }))
        .when_some(count, |this, count| {
            this.child(
                div()
                    .font_family(MONO)
                    .text_size(px(10.5))
                    .text_color(if on && count > 0 {
                        ink::accent2()
                    } else {
                        ink::faint()
                    })
                    .child(count.to_string()),
            )
        })
}

fn icon_button(id: impl Into<ElementId>, icon: Lucide, tip: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(26.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .cursor_pointer()
        .text_color(ink::muted())
        .hover(|s| s.bg(ink::inset()).text_color(ink::text()))
        .child(Icon::new(icon).with_size(px(14.)))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
}

fn square_button(id: &'static str, icon: Lucide, tip: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(34.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .border_1()
        .border_color(ink::line_strong())
        .cursor_pointer()
        .text_color(ink::muted())
        .hover(|s| s.bg(ink::inset()).text_color(ink::text()))
        .child(Icon::new(icon).with_size(px(14.)))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
}

fn accent_button(id: &'static str, label: &'static str, icon: Option<Lucide>) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(26.))
        .px(px(12.))
        .gap(px(6.))
        .flex_shrink_0()
        .rounded(px(8.))
        .bg(linear_gradient(
            180.,
            linear_color_stop(ink::accent2(), 0.),
            linear_color_stop(ink::accent(), 1.),
        ))
        .shadow(glow(ink::accent(), 0.35, 14.))
        .text_color(ink::bg()) // dark text: white on lime doesn't read
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|s| s.opacity(0.92))
        .active(|s| s.opacity(0.8))
        .when_some(icon, |this, icon| {
            this.child(Icon::new(icon).with_size(px(13.)))
        })
        .child(label)
}

fn outline_button(id: &'static str, label: &'static str, icon: Lucide) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(30.))
        .px(px(12.))
        .gap(px(6.))
        .flex_shrink_0()
        .rounded(px(8.))
        .border_1()
        .border_color(ink::line_strong())
        .text_size(px(12.5))
        .text_color(ink::text())
        .cursor_pointer()
        .hover(|s| s.bg(ink::inset()))
        .child(Icon::new(icon).with_size(px(13.)).text_color(ink::muted()))
        .child(label)
}

fn ghost_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(32.))
        .px(px(14.))
        .rounded(px(8.))
        .text_size(px(13.))
        .text_color(ink::muted())
        .cursor_pointer()
        .hover(|s| s.bg(ink::inset()).text_color(ink::text()))
        .child(label)
}

fn step_button(id: (&'static str, usize), label: &'static str, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(28.))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .font_family(MONO)
        .text_size(px(14.))
        .text_color(if enabled {
            ink::text()
        } else {
            ink::line_strong()
        })
        .when(enabled, |this| {
            this.cursor_pointer().hover(|s| s.bg(ink::card()))
        })
        .child(label)
}

fn stat_tile(label: &'static str, value: String, note: Option<&'static str>) -> impl IntoElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .h(px(42.))
        .px(px(10.))
        .py(px(6.))
        .gap(px(2.))
        .rounded(px(10.))
        .bg(ink::inset())
        .child(caps(label, ink::faint()))
        .child(
            h_flex()
                .gap(px(6.))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(MONO)
                        .text_size(px(13.))
                        .text_color(ink::text())
                        .child(value),
                )
                .when_some(note, |this, note| {
                    this.child(div().flex_1()).child(caps(note, ink::accent2()))
                }),
        )
}

/// CONNECTIONS: in use out of the setting, one slot per connection.
fn connections_tile(used: usize, want: usize, server_limited: bool) -> impl IntoElement {
    let slots = want.min(32);
    h_flex()
        .h(px(42.))
        .px(px(10.))
        .gap(px(12.))
        .rounded(px(10.))
        .bg(ink::inset())
        .child(
            v_flex()
                .flex_shrink_0()
                .gap(px(2.))
                .child(caps("CONNECTIONS", ink::faint()))
                .child(
                    div()
                        .font_family(MONO)
                        .text_size(px(13.))
                        .child(format!("{used} / {want}")),
                ),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap(px(3.))
                .children((0..slots).map(|i| {
                    div()
                        .flex_1()
                        .max_w(px(14.))
                        .h(px(8.))
                        .rounded(px(2.))
                        .bg(if i < used {
                            ink::accent()
                        } else {
                            ink::line_strong()
                        })
                        .when(i < used, |this| this.shadow(glow(ink::accent(), 0.35, 4.)))
                })),
        )
        .when(server_limited, |this| {
            this.child(caps("SERVER LIMITED", ink::accent2()))
        })
}

fn big_tile(label: &'static str, value: String, note: &str) -> impl IntoElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .px(px(14.))
        .py(px(11.))
        .gap(px(4.))
        .rounded(px(16.))
        .bg(ink::panel())
        .border_1()
        .border_color(ink::line())
        .child(caps(label, ink::faint()))
        .child(
            div()
                .truncate()
                .font_family(MONO)
                .font_weight(FontWeight::MEDIUM)
                .text_size(px(20.))
                .line_height(px(24.))
                .child(value),
        )
        .child(caps(note, ink::faint()))
}

fn card_panel() -> Div {
    v_flex()
        .p(px(16.))
        .gap(px(10.))
        .rounded(px(16.))
        .bg(ink::panel())
        .border_1()
        .border_color(ink::line())
}

/// Accent first, near-white second, then greys: the two-series rule, extended for the donut.
fn series_color(i: usize) -> Hsla {
    match i {
        0 => ink::accent(),
        1 => ink::white2(),
        2 => ink::muted(),
        3 => ink::faint(),
        4 => rgb(0x45454b).into(),
        _ => ink::line_strong(),
    }
}

fn render_empty_state() -> impl IntoElement {
    v_flex()
        .flex_1()
        .min_h_0()
        .items_center()
        .justify_center()
        .gap(px(18.))
        .rounded(px(16.))
        .bg(ink::panel())
        .border_1()
        .border_color(ink::line())
        .child(
            v_flex()
                .items_center()
                .gap(px(4.))
                .font_family(DISPLAY)
                .font_weight(FontWeight::EXTRA_BOLD)
                .text_size(px(40.))
                .line_height(px(46.))
                .child(div().text_color(ink::accent()).child("PASTE A LINK."))
                .child(
                    div()
                        .text_color(ink::text())
                        .child("WE'LL SPLIT IT EIGHT WAYS."),
                ),
        )
        .child(
            h_flex().gap(px(8.)).child(kbd("⌘V")).child(
                div()
                    .text_size(px(13.))
                    .text_color(ink::muted())
                    .child("to paste a link from the clipboard"),
            ),
        )
        .child(caps(
            "OR SEND DOWNLOADS FROM YOUR BROWSER WITH THE SHARD EXTENSION",
            ink::faint(),
        ))
}

/// An on/off switch, as the Limits panel draws them.
fn switch(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_shrink_0()
        .w(px(34.))
        .h(px(20.))
        .p(px(2.))
        .rounded_full()
        .cursor_pointer()
        .bg(if on { ink::accent() } else { ink::line_strong() })
        .child(div().size(px(16.)).rounded_full().bg(ink::text()).when(on, |knob| knob.ml(px(14.))))
}

/// "Shutting down in 42 s · Cancel", across the top while the countdown runs.
fn render_power_banner(alert: String, cx: &Context<IdmView>) -> impl IntoElement {
    h_flex()
        .absolute()
        .top(px(60.))
        .left_0()
        .right_0()
        .justify_center()
        .child(
            h_flex()
                .gap(px(10.))
                .pl(px(14.))
                .pr(px(6.))
                .py(px(6.))
                .rounded_full()
                .bg(ink::panel())
                .border_1()
                .border_color(ink::accent().opacity(0.55))
                .shadow(glow(ink::accent(), 0.25, 18.))
                .child(dot(ink::accent(), 7.))
                .child(div().text_size(px(13.)).child(format!("{alert}. The downloads finished.")))
                .child(ghost_button("power-cancel", "Cancel").on_click(cx.listener(|_, _, _, cx| cancel_power(cx)))),
        )
}

fn render_command_error(err: SharedString, cx: &Context<IdmView>) -> impl IntoElement {
    div()
        .absolute()
        .top(px(50.))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            h_flex()
                .id("command-error")
                .max_w(px(560.))
                .px(px(12.))
                .py(px(8.))
                .gap(px(8.))
                .rounded(px(10.))
                .bg(rgb(0x1a1112))
                .border_1()
                .border_color(ink::red().opacity(0.45))
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.5),
                    offset: point(px(0.), px(8.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .occlude()
                .child(
                    Icon::new(Lucide::CircleAlert)
                        .with_size(px(14.))
                        .text_color(ink::red()),
                )
                .child(div().text_size(px(12.5)).child(err))
                .child(
                    icon_button("command-error-close", Lucide::X, "Dismiss").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.command_error = None;
                            cx.notify();
                        },
                    )),
                ),
        )
}

// ---------------------------------------------------------------------------------------------
// Painting

fn paint_text(
    window: &mut Window,
    cx: &mut App,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    color: Hsla,
    right: bool,
) {
    if text.is_empty() {
        return;
    }
    let run = TextRun {
        len: text.len(),
        font: font(MONO),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        SharedString::from(text.to_string()),
        px(size),
        &[run],
        None,
    );
    let x = if right { x - line.width.as_f32() } else { x };
    let _ = line.paint(
        point(px(x), px(y)),
        px(size * 1.25),
        TextAlign::Left,
        None,
        window,
        cx,
    );
}

fn hline(window: &mut Window, x0: f32, x1: f32, y: f32, color: Hsla) {
    window.paint_quad(fill(
        Bounds {
            origin: point(px(x0), px(y)),
            size: size(px(x1 - x0), px(1.)),
        },
        color,
    ));
}

fn dashed(window: &mut Window, x0: f32, x1: f32, y: f32, color: Hsla) {
    let mut b = PathBuilder::stroke(px(1.)).dash_array(&[px(3.), px(4.)]);
    b.move_to(point(px(x0), px(y)));
    b.line_to(point(px(x1), px(y)));
    if let Ok(path) = b.build() {
        window.paint_path(path, color);
    }
}

fn glow_dot(window: &mut Window, x: f32, y: f32, r: f32, color: Hsla, halo: f32) {
    let b = Bounds {
        origin: point(px(x - r), px(y - r)),
        size: size(px(2. * r), px(2. * r)),
    };
    if halo > 0. {
        window.paint_drop_shadows(b, Corners::all(px(r)), &glow(color, halo, r * 4.));
    }
    window.paint_quad(fill(b, color).corner_radii(px(r)));
}

/// An area series: fill fading to transparent, then the line on top.
fn paint_area(
    window: &mut Window,
    pts: &[(f32, f32)],
    base: f32,
    line: Hsla,
    fill_color: Hsla,
    width: f32,
    fill_alpha: f32,
) {
    if pts.len() < 2 {
        return;
    }
    if fill_alpha > 0. {
        let mut area = PathBuilder::fill();
        area.move_to(point(px(pts[0].0), px(base)));
        for (x, y) in pts {
            area.line_to(point(px(*x), px(*y)));
        }
        area.line_to(point(px(pts[pts.len() - 1].0), px(base)));
        area.close();
        if let Ok(path) = area.build() {
            window.paint_path(
                path,
                linear_gradient(
                    180.,
                    linear_color_stop(fill_color.opacity(fill_alpha), 0.),
                    linear_color_stop(fill_color.opacity(0.), 1.),
                ),
            );
        }
    }
    let mut stroke = PathBuilder::stroke(px(width));
    stroke.move_to(point(px(pts[0].0), px(pts[0].1)));
    for (x, y) in &pts[1..] {
        stroke.line_to(point(px(*x), px(*y)));
    }
    if let Ok(path) = stroke.build() {
        window.paint_path(path, line);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_throughput(
    b: Bounds<Pixels>,
    data: &[u64],
    selected: &[u64],
    scale: u64,
    avg: u64,
    cap: u64,
    phase: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let (x0, y0, w, h) = (
        b.left().as_f32(),
        b.top().as_f32(),
        b.size.width.as_f32(),
        b.size.height.as_f32(),
    );
    let top = y0 + 14.;
    let base = y0 + h - 1.;
    let ch = base - top;
    for i in 0..4 {
        hline(
            window,
            x0,
            x0 + w,
            top + ch * i as f32 / 3.,
            hsla(0., 0., 1., if i == 3 { 0.08 } else { 0.035 }),
        );
    }
    paint_text(
        window,
        cx,
        &format_speed(scale).to_string(),
        x0 + w,
        y0 - 2.,
        9.5,
        ink::faint(),
        true,
    );

    if data.iter().all(|v| *v == 0) {
        hline(window, x0, x0 + w, base - 1., ink::faint().opacity(0.5));
        paint_text(
            window,
            cx,
            "IDLE",
            x0 + w / 2. - 12.,
            top + ch / 2. - 6.,
            10.,
            ink::faint(),
            false,
        );
        return;
    }
    let dx = w / (HISTORY_POINTS as f32 - 1.);
    let n = data.len();
    // The newest sample slides in from the right over its first second.
    let x_of = |i: usize, len: usize| x0 + w - (len - 1 - i) as f32 * dx + (1. - phase) * dx;
    let y_of = |v: u64| base - (v as f32 / scale.max(1) as f32).min(1.) * ch;
    let pts: Vec<(f32, f32)> = data
        .iter()
        .enumerate()
        .map(|(i, v)| (x_of(i, n), y_of(*v)))
        .collect();

    window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
        paint_area(window, &pts, base, ink::accent2(), ink::accent(), 1.5, 0.38);
        if selected.len() > 1 {
            let m = selected.len();
            let sel: Vec<(f32, f32)> = selected
                .iter()
                .enumerate()
                .map(|(i, v)| (x_of(i, m), y_of(*v)))
                .collect();
            paint_area(
                window,
                &sel,
                base,
                ink::white2().opacity(0.8),
                ink::white2(),
                1.,
                0.,
            );
        }
        if cap > 0 && cap <= scale {
            dashed(window, x0, x0 + w, y_of(cap), ink::accent().opacity(0.45));
        }
        if avg > 0 {
            dashed(window, x0, x0 + w, y_of(avg), ink::white2().opacity(0.28));
        }
    });
    if avg > 0 {
        paint_text(
            window,
            cx,
            &format!("AVG {}", format_speed(avg)),
            x0 + 2.,
            y_of(avg) - 13.,
            9.5,
            ink::muted(),
            false,
        );
    }
    if let Some((i, v)) = data.iter().enumerate().max_by_key(|(_, v)| **v) {
        let (x, y) = (x_of(i, n), y_of(*v));
        if x >= x0 && x <= x0 + w {
            glow_dot(window, x, y, 2.5, ink::accent2(), 0.6);
            let label = format!("PEAK {}", format_speed(*v));
            let right = x > x0 + w - 90.;
            paint_text(
                window,
                cx,
                &label,
                if right { x - 6. } else { x + 6. },
                (y - 14.).max(y0 - 2.),
                9.5,
                ink::text(),
                right,
            );
        }
    }
}

const HISTORY_POINTS: usize = 60;

/// The tiny line in an active queue card.
fn paint_sparkline(b: Bounds<Pixels>, values: &[u64], _filled: bool, window: &mut Window) {
    let (x0, y0, w, h) = (
        b.left().as_f32(),
        b.top().as_f32(),
        b.size.width.as_f32(),
        b.size.height.as_f32(),
    );
    let max = values.iter().copied().max().unwrap_or(1).max(1) as f32;
    let n = values.len().max(2);
    let pts: Vec<(f32, f32)> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            (
                x0 + w * i as f32 / (n - 1) as f32,
                y0 + h - 1. - (*v as f32 / max) * (h - 2.),
            )
        })
        .collect();
    paint_area(window, &pts, y0 + h, ink::accent2(), ink::accent(), 1., 0.3);
}

fn paint_detail_speed(
    b: Bounds<Pixels>,
    values: &[u64],
    avg: u64,
    live: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let (x0, y0, w, h) = (
        b.left().as_f32(),
        b.top().as_f32(),
        b.size.width.as_f32(),
        b.size.height.as_f32(),
    );
    let base = y0 + h - 5.;
    let top = y0 + 20.;
    paint_caps(window, cx, "SPEED", x0 + 10., y0 + 6., ink::faint(), false);
    if values.iter().all(|v| *v == 0) || values.len() < 2 {
        hline(
            window,
            x0 + 10.,
            x0 + w - 10.,
            base - 1.,
            ink::faint().opacity(0.5),
        );
        paint_caps(
            window,
            cx,
            if live { "STARTING" } else { "IDLE" },
            x0 + w - 10.,
            y0 + 6.,
            ink::faint(),
            true,
        );
        return;
    }
    let max = nice_max(values.iter().copied().max().unwrap_or(1)) as f32;
    paint_caps(
        window,
        cx,
        &format!("MAX {}", format_speed(max as u64)),
        x0 + w - 10.,
        y0 + 6.,
        ink::faint(),
        true,
    );
    let n = values.len();
    let y_of = |v: u64| base - (v as f32 / max).min(1.) * (base - top);
    let pts: Vec<(f32, f32)> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            (
                x0 + w * i as f32 / (HISTORY_POINTS.max(n) - 1) as f32,
                y_of(*v),
            )
        })
        .collect();
    paint_area(
        window,
        &pts,
        base + 5.,
        ink::accent2(),
        ink::accent(),
        1.5,
        0.35,
    );
    if avg > 0 {
        dashed(window, x0, x0 + w, y_of(avg), ink::white2().opacity(0.25));
    }
    if let Some((x, y)) = pts.last()
        && live {
            glow_dot(window, *x - 2., *y, 2.5, ink::accent2(), 0.7);
        }
}

/// Tracked uppercase mono text painted in a canvas, like `caps`.
fn paint_caps(
    window: &mut Window,
    cx: &mut App,
    text: &str,
    x: f32,
    y: f32,
    color: Hsla,
    right: bool,
) {
    const SIZE: f32 = 9.5;
    const TRACK: f32 = 1.2;
    let advance = {
        let run = TextRun {
            len: 1,
            font: font(MONO),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window
            .text_system()
            .shape_line("0".into(), px(SIZE), &[run], None)
            .width
            .as_f32()
    };
    let n = text.chars().count() as f32;
    let width = n * advance + (n - 1.).max(0.) * TRACK;
    let mut at = if right { x - width } else { x };
    for ch in text.chars() {
        if ch != ' ' {
            paint_text(window, cx, &ch.to_string(), at, y, SIZE, color, false);
        }
        at += advance + TRACK;
    }
}

/// The segment map: every part at its real place, fill in accent, a glowing cursor where a
/// connection is writing. `thin` is the 3px bar on a queue card.
#[allow(clippy::too_many_arguments)]
fn paint_segments(
    b: Bounds<Pixels>,
    parts: &[Segment],
    status: Status,
    total: u64,
    frac: f32,
    time: f32,
    thin: bool,
    window: &mut Window,
) {
    let (x0, y0, w, h) = (
        b.left().as_f32(),
        b.top().as_f32(),
        b.size.width.as_f32(),
        b.size.height.as_f32(),
    );
    let gap = if thin { 1. } else { 2. };
    let radius = if thin { 1.5 } else { 3. };
    let track = if thin {
        hsla(0., 0., 1., 0.07)
    } else {
        ink::inset()
    };
    let filled: Background = match status {
        Status::Downloading => linear_gradient(
            90.,
            linear_color_stop(ink::accent(), 0.),
            linear_color_stop(ink::accent2(), 1.),
        ),
        Status::Paused | Status::Queued => ink::white2().opacity(0.5).into(),
        Status::Done => ink::muted().opacity(0.45).into(),
        Status::Failed => ink::faint().into(),
    };
    let rect = |a: f32, z: f32| Bounds {
        origin: point(px(a), px(y0)),
        size: size(px((z - a).max(0.)), px(h)),
    };

    if parts.is_empty() {
        window.paint_quad(fill(rect(x0, x0 + w), track).corner_radii(px(radius)));
        if status == Status::Done || (total > 0 && frac > 0.) {
            window.paint_quad(
                fill(
                    rect(
                        x0,
                        x0 + w * frac.max(if status == Status::Done { 1. } else { 0. }),
                    ),
                    filled,
                )
                .corner_radii(px(radius)),
            );
        } else if status == Status::Downloading {
            // Size unknown: a shimmer slides along the track.
            let t = (time * 0.6).fract();
            let a = x0 + (w + 80.) * t - 80.;
            window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
                window.paint_quad(
                    fill(
                        rect(a, a + 80.),
                        linear_gradient(
                            90.,
                            linear_color_stop(ink::accent().opacity(0.), 0.),
                            linear_color_stop(ink::accent2(), 1.),
                        ),
                    )
                    .corner_radii(px(radius)),
                );
            });
        }
        return;
    }
    for s in parts {
        let (a, z, d) = segment_span(s, x0, w, gap);
        window.paint_quad(fill(rect(a, z), track).corner_radii(px(radius)));
        if d > a + 0.5 {
            window.paint_quad(fill(rect(a, d), filled).corner_radii(px(radius)));
        }
        if s.active && status == Status::Downloading && !thin {
            let cursor = Bounds {
                origin: point(px(d - 1.), px(y0 - 2.)),
                size: size(px(2.), px(h + 4.)),
            };
            window.paint_drop_shadows(cursor, Corners::all(px(1.)), &glow(ink::accent2(), 0.9, 8.));
            window.paint_quad(fill(cursor, hsla(0., 0., 1., 0.95)).corner_radii(px(1.)));
        }
    }
}

/// Where part `s` sits on a track `w` wide from `x0`: (left, right, filled up to). A part too narrow
/// for the gap between parts is drawn without one, so its edges never cross.
fn segment_span(s: &Segment, x0: f32, w: f32, gap: f32) -> (f32, f32, f32) {
    let (mut a, mut z) = (x0 + s.start * w, x0 + s.end * w);
    if z - a > 2. * gap {
        if s.start > 0. {
            a += gap / 2.;
        }
        if s.end < 1. {
            z -= gap / 2.;
        }
    }
    let z = z.max(a);
    (a, z, (x0 + s.done * w).clamp(a, z))
}

/// The speed-cap gauge: a 270° arc, accent up to the share of the cap in use.
fn paint_gauge(b: Bounds<Pixels>, frac: f32, window: &mut Window) {
    let c = b.center();
    let (cx_, cy_) = (c.x.as_f32(), c.y.as_f32());
    let r = b.size.width.as_f32().min(b.size.height.as_f32()) / 2. - 6.;
    let start = 135f32.to_radians();
    let sweep = 270f32.to_radians();
    let arc = |from: f32, to: f32, width: f32| -> Option<gpui_kit::Path<Pixels>> {
        let mut p = PathBuilder::stroke(px(width));
        let steps = 48;
        for i in 0..=steps {
            let a = from + (to - from) * i as f32 / steps as f32;
            let pt = point(px(cx_ + r * a.cos()), px(cy_ + r * a.sin()));
            if i == 0 { p.move_to(pt) } else { p.line_to(pt) }
        }
        p.build().ok()
    };
    if let Some(path) = arc(start, start + sweep, 4.) {
        window.paint_path(path, ink::line_strong());
    }
    // Tick marks at every quarter of the cap.
    for i in 0..=4 {
        let a = start + sweep * i as f32 / 4.;
        let (r0, r1) = (r - 9., r - 6.);
        let mut t = PathBuilder::stroke(px(1.));
        t.move_to(point(px(cx_ + r0 * a.cos()), px(cy_ + r0 * a.sin())));
        t.line_to(point(px(cx_ + r1 * a.cos()), px(cy_ + r1 * a.sin())));
        if let Ok(path) = t.build() {
            window.paint_path(path, ink::faint());
        }
    }
    if frac > 0.005 {
        if let Some(path) = arc(start, start + sweep * frac, 4.) {
            window.paint_path(
                path,
                linear_gradient(
                    90.,
                    linear_color_stop(ink::accent(), 0.),
                    linear_color_stop(ink::accent2(), 1.),
                ),
            );
        }
        let a = start + sweep * frac;
        glow_dot(
            window,
            cx_ + r * a.cos(),
            cy_ + r * a.sin(),
            3.5,
            ink::accent2(),
            0.8,
        );
    }
}

/// The connection graph's edges and particles; the nodes are elements on top.
fn paint_graph(
    b: Bounds<Pixels>,
    nodes: &[(f32, f32, f32, bool, bool, f32)],
    live: bool,
    window: &mut Window,
) {
    let (x0, y0, w) = (b.left().as_f32(), b.top().as_f32(), b.size.width.as_f32());
    let mid = y0 + GRAPH_HEIGHT / 2.;
    let server = (x0 + GRAPH_SERVER_W, mid);
    let file = (x0 + w - GRAPH_FILE_W + 10., mid);
    let cxm = x0 + w / 2. + GRAPH_SHIFT;
    if nodes.is_empty() {
        // Idle: one faint dashed line from server to file.
        let mut p = PathBuilder::stroke(px(1.)).dash_array(&[px(3.), px(5.)]);
        p.move_to(point(px(server.0), px(server.1)));
        p.line_to(point(px(file.0), px(file.1)));
        if let Ok(path) = p.build() {
            window.paint_path(path, ink::faint().opacity(0.5));
        }
        return;
    }
    let curve = |a: (f32, f32), z: (f32, f32)| {
        let dx = (z.0 - a.0) * 0.5;
        [a, (a.0 + dx, a.1), (z.0 - dx, z.1), z]
    };
    for (y, alpha, norm, active, fastest, phase) in nodes {
        let y = y0 + y;
        let left = curve(server, (cxm - GRAPH_NODE_W / 2., y));
        let right = curve((cxm + GRAPH_NODE_W / 2., y), file);
        let (width, color) = if !active {
            (1., ink::faint().opacity(0.35 * alpha))
        } else if *fastest {
            (1.2 + 2.3 * norm, ink::accent().opacity(0.9 * alpha))
        } else {
            (
                1. + 2. * norm,
                ink::white2().opacity((0.22 + 0.45 * norm) * alpha),
            )
        };
        for c in [left, right] {
            let mut p = PathBuilder::stroke(px(width));
            if !active {
                p = p.dash_array(&[px(2.), px(4.)]);
            }
            p.move_to(point(px(c[0].0), px(c[0].1)));
            p.cubic_bezier_to(
                point(px(c[3].0), px(c[3].1)),
                point(px(c[1].0), px(c[1].1)),
                point(px(c[2].0), px(c[2].1)),
            );
            if let Ok(path) = p.build() {
                window.paint_path(path, color);
            }
        }
        if *active && live && *alpha > 0.3 {
            // Particles travel server → part → file, faster on faster edges.
            for k in 0..3 {
                let t = (phase + k as f32 * 2. / 3.) % 2.;
                let (c, u) = if t < 1. { (left, t) } else { (right, t - 1.) };
                let (x, y) = bezier(c, u);
                let dot_color = if *fastest {
                    ink::accent2()
                } else {
                    ink::white2()
                };
                glow_dot(
                    window,
                    x,
                    y,
                    1.6 + norm,
                    dot_color.opacity(*alpha),
                    if *fastest { 0.7 } else { 0.35 },
                );
            }
        }
    }
}

fn bezier(c: [(f32, f32); 4], t: f32) -> (f32, f32) {
    let u = 1. - t;
    let (a, b, d) = (u * u * u, 3. * u * u * t, 3. * u * t * t);
    let e = t * t * t;
    (
        a * c[0].0 + b * c[1].0 + d * c[2].0 + e * c[3].0,
        a * c[0].1 + b * c[1].1 + d * c[2].1 + e * c[3].1,
    )
}

fn paint_day_bars(b: Bounds<Pixels>, days: &[u64], window: &mut Window, cx: &mut App) {
    let (x0, y0, w, h) = (
        b.left().as_f32(),
        b.top().as_f32(),
        b.size.width.as_f32(),
        b.size.height.as_f32(),
    );
    let base = y0 + h - 1.;
    let top = y0 + 16.;
    for i in 0..3 {
        hline(
            window,
            x0,
            x0 + w,
            top + (base - top) * i as f32 / 2.,
            hsla(0., 0., 1., if i == 2 { 0.08 } else { 0.035 }),
        );
    }
    let max = nice_max(days.iter().copied().max().unwrap_or(0));
    paint_text(
        window,
        cx,
        &format_bytes(max),
        x0 + w,
        y0 - 2.,
        9.5,
        ink::faint(),
        true,
    );
    if days.iter().all(|d| *d == 0) {
        paint_text(
            window,
            cx,
            "NO FINISHED DOWNLOADS YET",
            x0 + w / 2. - 70.,
            top + (base - top) / 2. - 6.,
            10.,
            ink::faint(),
            false,
        );
        return;
    }
    let slot = w / days.len() as f32;
    let bar = (slot * 0.56).min(26.);
    for (i, d) in days.iter().enumerate() {
        let x = x0 + slot * i as f32 + (slot - bar) / 2.;
        let hgt = (*d as f32 / max.max(1) as f32) * (base - top);
        if *d == 0 {
            window.paint_quad(
                fill(
                    Bounds {
                        origin: point(px(x), px(base - 2.)),
                        size: size(px(bar), px(2.)),
                    },
                    hsla(0., 0., 1., 0.06),
                )
                .corner_radii(px(1.)),
            );
            continue;
        }
        let r = Bounds {
            origin: point(px(x), px(base - hgt)),
            size: size(px(bar), px(hgt)),
        };
        let color = if i == days.len() - 1 {
            linear_gradient(
                180.,
                linear_color_stop(ink::accent2(), 0.),
                linear_color_stop(ink::accent().opacity(0.55), 1.),
            )
        } else {
            linear_gradient(
                180.,
                linear_color_stop(ink::accent().opacity(0.85), 0.),
                linear_color_stop(ink::accent().opacity(0.25), 1.),
            )
        };
        window.paint_quad(fill(r, color).corner_radii(Corners {
            top_left: px(4.),
            top_right: px(4.),
            bottom_left: px(1.),
            bottom_right: px(1.),
        }));
    }
}

fn paint_donut(b: Bounds<Pixels>, slices: &[(f32, Hsla)], window: &mut Window) {
    let c = b.center();
    let (cx_, cy_) = (c.x.as_f32(), c.y.as_f32());
    let r1 = b.size.width.as_f32().min(b.size.height.as_f32()) / 2. - 2.;
    let r0 = r1 - 14.;
    let ring = |from: f32, to: f32| -> Option<gpui_kit::Path<Pixels>> {
        let mut p = PathBuilder::fill();
        let steps = ((to - from) / 0.05).ceil().max(2.) as usize;
        for i in 0..=steps {
            let a = from + (to - from) * i as f32 / steps as f32;
            let pt = point(px(cx_ + r1 * a.cos()), px(cy_ + r1 * a.sin()));
            if i == 0 { p.move_to(pt) } else { p.line_to(pt) }
        }
        for i in (0..=steps).rev() {
            let a = from + (to - from) * i as f32 / steps as f32;
            p.line_to(point(px(cx_ + r0 * a.cos()), px(cy_ + r0 * a.sin())));
        }
        p.close();
        p.build().ok()
    };
    if slices.is_empty() {
        if let Some(path) = ring(0., std::f32::consts::TAU) {
            window.paint_path(path, ink::inset());
        }
        return;
    }
    let gap = if slices.len() > 1 { 0.025 } else { 0. };
    let mut a = -std::f32::consts::FRAC_PI_2;
    for (share, color) in slices {
        let sweep = share * std::f32::consts::TAU;
        if sweep > gap * 2.
            && let Some(path) = ring(a + gap, a + sweep - gap) {
                window.paint_path(path, *color);
            }
        a += sweep;
    }
}

/// Connections in use over time, as steps.
fn paint_steps(b: Bounds<Pixels>, values: &[u64], slots: usize, window: &mut Window, cx: &mut App) {
    let (x0, y0, w, h) = (
        b.left().as_f32(),
        b.top().as_f32(),
        b.size.width.as_f32(),
        b.size.height.as_f32(),
    );
    let base = y0 + h - 1.;
    let top = y0 + 12.;
    for i in 0..3 {
        hline(
            window,
            x0,
            x0 + w,
            top + (base - top) * i as f32 / 2.,
            hsla(0., 0., 1., if i == 2 { 0.08 } else { 0.035 }),
        );
    }
    let max = values.iter().copied().max().unwrap_or(0).max(4);
    paint_text(
        window,
        cx,
        &format!("{max}"),
        x0 + w,
        y0 - 3.,
        9.5,
        ink::faint(),
        true,
    );
    if values.iter().all(|v| *v == 0) {
        hline(window, x0, x0 + w, base - 1., ink::faint().opacity(0.5));
        paint_text(
            window,
            cx,
            "IDLE",
            x0 + w / 2. - 12.,
            top + (base - top) / 2. - 6.,
            10.,
            ink::faint(),
            false,
        );
        return;
    }
    let dx = w / (slots.max(2) - 1) as f32;
    let n = values.len();
    let mut pts = Vec::with_capacity(n * 2);
    for (i, v) in values.iter().enumerate() {
        let x = x0 + w - (n - 1 - i) as f32 * dx;
        let y = base - (*v as f32 / max as f32) * (base - top);
        if let Some(&(_, py)) = pts.last() {
            pts.push((x, py));
        }
        pts.push((x, y));
    }
    if pts.len() == 1 {
        let (x, y) = pts[0];
        pts.insert(0, (x - dx, y));
    }
    paint_area(window, &pts, base, ink::accent2(), ink::accent(), 1.5, 0.3);
}

// ---------------------------------------------------------------------------------------------
// Helpers

fn open_file(row: &Row, cx: &App) {
    if let Some(path) = &row.path {
        cx.open_with_system(path);
    }
}

/// Show the file in the file manager, or open the folder it goes to.
fn reveal(row: &Row, settings: &Settings, cx: &App) {
    match row.path.as_deref() {
        Some(path) if path.exists() => cx.reveal_path(path),
        Some(path) => cx.open_with_system(path.parent().unwrap_or(&settings.dir)),
        None => cx.open_with_system(&settings.dir),
    }
}

fn clipboard_link(cx: &App) -> Option<String> {
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .map(|text| text.trim().to_string())
        .filter(|text| looks_like_url(text))
}

fn fraction(row: &Row) -> f32 {
    if row.status == Status::Done {
        1.
    } else if row.total > 0 {
        (row.done as f64 / row.total as f64).clamp(0., 1.) as f32
    } else {
        0.
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

/// The file's name: from its path once the server has named it, else from the URL.
fn file_name(row: &Row) -> &str {
    row.path
        .as_deref()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .unwrap_or_else(|| {
            let name = row.name.split(['?', '#']).next().unwrap_or(&row.name);
            name.rsplit('/').next().unwrap_or(name)
        })
}

fn extension(row: &Row) -> String {
    file_name(row)
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

fn extension_label(row: &Row) -> String {
    let ext = extension(row).to_ascii_uppercase();
    if ext.is_empty() || ext.len() > 4 {
        "FILE".into()
    } else {
        ext
    }
}

fn display_name(row: &Row) -> String {
    if row.name.is_empty() {
        row.url.clone()
    } else {
        row.name.clone()
    }
}

fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .to_string()
}

fn looks_like_url(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://"))
        && text.len() > "https://".len()
        && !text.chars().any(char::is_whitespace)
}

fn expand_home(dir: &str) -> PathBuf {
    match dir.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest.trim_start_matches('/')),
            None => PathBuf::from(dir),
        },
        _ => PathBuf::from(dir),
    }
}

/// A path with the home folder as `~`.
fn tilde(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(rest) = path.strip_prefix(&home) {
            return if rest.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", rest.display())
            };
        }
    path.display().to_string()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A round axis maximum at or above `v`: 1, 2 or 5 times a power of ten of KB, MB or GB, so
/// the label reads as a round number.
fn nice_max(v: u64) -> u64 {
    if v == 0 {
        return MB;
    }
    let unit = if v < MB {
        1024
    } else if v < 1024 * MB {
        MB
    } else {
        1024 * MB
    };
    let x = v as f64 / unit as f64;
    let mut step = 1f64;
    while step * 10. <= x {
        step *= 10.;
    }
    let m = [1., 2., 5., 10.]
        .into_iter()
        .find(|m| step * m >= x)
        .unwrap_or(10.);
    (step * m * unit as f64) as u64
}

/// 1536 -> "1.50 KB", 52_428_800 -> "50.0 MB".
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.;
    let mut unit = 0;
    while value >= 1024. && unit + 1 < UNITS.len() {
        value /= 1024.;
        unit += 1;
    }
    let decimals = if value >= 100. {
        0
    } else if value >= 10. {
        1
    } else {
        2
    };
    format!("{value:.decimals$} {}", UNITS[unit])
}

fn format_speed(bytes_per_second: u64) -> String {
    let (value, unit) = speed_parts(bytes_per_second);
    format!("{value} {unit}")
}

/// A speed as a number and its unit, for the big readout.
fn speed_parts(bps: u64) -> (String, &'static str) {
    let v = bps as f64;
    if bps < 1024 * 1024 {
        (format!("{:.0}", v / 1024.), "KB/s")
    } else if bps < 1024 * 1024 * 1024 {
        let m = v / 1024. / 1024.;
        (
            if m >= 100. {
                format!("{m:.0}")
            } else {
                format!("{m:.1}")
            },
            "MB/s",
        )
    } else {
        (format!("{:.2}", v / 1024. / 1024. / 1024.), "GB/s")
    }
}

/// 192 -> "3m 12s".
fn format_duration(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m {:02}s", secs / 60, secs % 60),
        3600..86400 => format!("{}h {:02}m", secs / 3600, secs % 3600 / 60),
        _ => format!("{}d {}h", secs / 86400, secs % 86400 / 3600),
    }
}

/// "just now", "12 min ago", "5 h ago", "3 days ago", then the date (UTC).
fn format_added(added: i64, now: i64) -> String {
    let ago = (now - added).max(0);
    match ago {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", ago / 60),
        3600..86400 => format!("{} h ago", ago / 3600),
        86400..172800 => "yesterday".into(),
        172800..604800 => format!("{} days ago", ago / 86400),
        _ => {
            let (y, m, d) = civil_from_days(added.div_euclid(86400));
            format!("{y}-{m:02}-{d:02}")
        }
    }
}

/// Days since 1970-01-01 to (year, month, day), from Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

// ---------------------------------------------------------------------------------------------
// The shell: the main window coming and going, the tray, and the tray's panel

/// Which windows are open. The main window can close while Shard runs on in the tray.
struct Shell {
    manager: Arc<Manager>,
    main: Option<(AnyWindowHandle, WeakEntity<IdmView>)>,
    panel: Option<AnyWindowHandle>,
    /// A download the panel wants the main window to show.
    select: Option<Id>,
    has_tray: bool,
    /// In the Dock (macOS); apps start there.
    docked: bool,
    /// A sleep or shut down waiting to happen once the queue finished, and when.
    power: Option<(WhenDone, Instant)>,
    /// The copied link the last notification offered to download.
    clip: Option<String>,
    updater: Arc<crate::update::Updater>,
}

impl Global for Shell {}

/// Bring the main window forward, opening it again if it was closed.
fn open_main(focus: bool, cx: &mut App) {
    let main = cx.global::<Shell>().main.as_ref().map(|(handle, _)| *handle);
    dock(true, cx);
    if let Some(handle) = main
        && handle.update(cx, |_, window, _| window.activate_window()).is_ok()
    {
        if focus {
            cx.activate(true);
        }
        return;
    }
    let manager = cx.global::<Shell>().manager.clone();
    let options = main_window_options(focus, cx);
    match gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| IdmView::new(manager, window, cx))) {
        Ok((handle, view)) => {
            cx.global_mut::<Shell>().main = Some((handle, view.downgrade()));
            if focus {
                cx.activate(true);
            }
        }
        Err(err) => {
            eprintln!("shard: couldn't open the window: {err:#}");
            if !cx.global::<Shell>().has_tray {
                cx.quit();
            }
        }
    }
}

/// macOS: in the Dock while the main window is open, only in the menu bar otherwise.
fn dock(visible: bool, cx: &mut App) {
    if cx.global::<Shell>().docked == visible {
        return;
    }
    cx.global_mut::<Shell>().docked = visible;
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            let policy = if visible { NSApplicationActivationPolicy::Regular } else { NSApplicationActivationPolicy::Accessory };
            NSApplication::sharedApplication(mtm).setActivationPolicy(policy);
        }
    }
}

/// Ten times a second: the tray's clicks, a second launch asking for the window, the main window
/// closing; once a second, the tray's icon and text.
fn run_shell(mut tray: Option<crate::tray::Tray>, cx: &mut App) {
    let mut shows = cx.global::<Shell>().manager.show_requests();
    let mut drained = cx.global::<Shell>().manager.drained();
    let mut drawn_at: Option<Instant> = None;
    // Statuses at the last look, and the clipboard's link: None until the first look, so nothing
    // that was already there gets announced.
    let (mut seen, mut clip_seen): (Option<HashMap<Id, Status>>, Option<Option<String>>) = (None, None);
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            let events = tray.as_ref().map(crate::tray::Tray::events).unwrap_or_default();
            let due = drawn_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(1));
            let status = cx.update(|cx| {
                for event in events {
                    tray_event(event, cx);
                }
                let manager = cx.global::<Shell>().manager.clone();
                let asked = manager.show_requests();
                if asked != shows {
                    shows = asked;
                    open_main(true, cx);
                }
                let main = cx.global::<Shell>().main.as_ref().map(|(handle, _)| *handle);
                if let Some(handle) = main
                    && handle.update(cx, |_, _, _| ()).is_err()
                {
                    // Closed: Shard lives on in the tray, out of the Dock.
                    cx.global_mut::<Shell>().main = None;
                    if cx.global::<Shell>().has_tray {
                        dock(false, cx);
                    }
                }
                due.then(|| {
                    let (rows, settings) = (manager.rows(), manager.settings());
                    announce(&rows, &settings, &mut seen, cx);
                    offer_clipboard(&rows, &settings, &mut clip_seen, cx);
                    keep_power(&manager, &settings, &rows, &mut drained, cx);
                    tray_status(&rows, &settings, cx)
                })
            });
            if let (Some(tray), Some(status)) = (tray.as_mut(), status) {
                tray.update(&status);
                drawn_at = Some(Instant::now());
            }
        }
    })
    .detach();
}

fn tray_status(rows: &[Row], settings: &Settings, cx: &App) -> crate::tray::Status {
    let running: Vec<&Row> = rows.iter().filter(|r| r.status == Status::Downloading).collect();
    let queued = rows.iter().filter(|r| r.status == Status::Queued).count();
    let speed: u64 = running.iter().map(|r| r.speed).sum();
    let (done, total) = running.iter().filter(|r| r.total > 0).fold((0, 0), |(d, t), r| (d + r.done, t + r.total));
    let summary = match (running.len(), queued) {
        (0, 0) => "Idle".to_string(),
        (0, q) => format!("{q} queued"),
        (n, 0) => format!("↓ {} · {n} active", format_speed(speed)),
        (n, q) => format!("↓ {} · {n} active · {q} queued", format_speed(speed)),
    };
    crate::tray::Status {
        title: (!running.is_empty()).then(|| format_speed(speed)),
        summary,
        progress: (total > 0).then(|| done as f32 / total as f32),
        running: !running.is_empty() || queued > 0,
        paused: rows.iter().any(|r| matches!(r.status, Status::Paused | Status::Failed)),
        cap: settings.speed_limit,
        keep_awake: settings.keep_awake,
        launch_at_login: settings.launch_at_login,
        alert: power_alert(cx),
        update: match cx.global::<Shell>().updater.state() {
            crate::update::State::Ready { version } => Some(format!("Restart to update to {version}")),
            _ => None,
        },
    }
}

/// "Shutting down in 42 s", while one is counting down.
fn power_alert(cx: &App) -> Option<String> {
    let (when, at) = cx.global::<Shell>().power?;
    let secs = at.saturating_duration_since(Instant::now()).as_secs();
    let doing = if when == WhenDone::ShutDown { "Shutting down" } else { "Sleeping" };
    Some(format!("{doing} in {secs} s"))
}

/// A system notification for each download that finished or failed since the last look.
fn announce(rows: &[Row], settings: &Settings, seen: &mut Option<HashMap<Id, Status>>, cx: &mut App) {
    let now: HashMap<Id, Status> = rows.iter().map(|r| (r.id, r.status)).collect();
    let before = seen.replace(now);
    let Some(before) = before.filter(|_| settings.notify) else { return };
    for r in rows {
        if !matches!(before.get(&r.id), Some(Status::Downloading | Status::Queued)) {
            continue;
        }
        let (title, body) = match r.status {
            Status::Done => (format!("Downloaded {}", display_name(r)), format_bytes(r.total)),
            Status::Failed => (format!("Couldn't download {}", display_name(r)), r.error.clone().unwrap_or_default()),
            _ => continue,
        };
        cx.show_system_notification(SystemNotification {
            tag: format!("row:{}", r.id).into(),
            title: title.into(),
            body: body.into(),
            actions: Vec::new(),
        });
    }
}

/// When a link to a file is copied, a notification offering to download it.
fn offer_clipboard(rows: &[Row], settings: &Settings, seen: &mut Option<Option<String>>, cx: &mut App) {
    let link = clipboard_link(cx);
    let Some(last) = seen.replace(link.clone()) else { return }; // the first look only remembers
    let Some(url) = link.filter(|url| settings.watch_clipboard && last.as_ref() != Some(url)) else { return };
    let name = url::Url::parse(&url)
        .ok()
        .and_then(|u| Some(percent_encoding::percent_decode_str(u.path_segments()?.next_back()?).decode_utf8_lossy().into_owned()))
        .unwrap_or_default();
    // Only links that look like files, and not ones already in the list.
    if crate::queue::category(&name) == "Other" || rows.iter().any(|r| r.url == url) {
        return;
    }
    cx.global_mut::<Shell>().clip = Some(url.clone());
    cx.show_system_notification(SystemNotification {
        tag: "clip".into(),
        title: format!("Download {name}?").into(),
        body: host(&url).into(),
        actions: vec![SystemNotificationAction { id: "download".into(), label: "Download".into() }],
    });
}

/// Once the queue finishes with "when done" set: a minute's warning, then sleep or shut down.
fn keep_power(manager: &Manager, settings: &Settings, rows: &[Row], drained: &mut u64, cx: &mut App) {
    let now = manager.drained();
    if now != *drained {
        *drained = now;
        if settings.when_done != WhenDone::Nothing {
            cx.global_mut::<Shell>().power = Some((settings.when_done, Instant::now() + Duration::from_secs(60)));
            let title = if settings.when_done == WhenDone::ShutDown { "Shutting down in a minute" } else { "Sleeping in a minute" };
            cx.show_system_notification(SystemNotification {
                tag: "power".into(),
                title: title.into(),
                body: "The downloads finished.".into(),
                actions: vec![SystemNotificationAction { id: "cancel".into(), label: "Cancel".into() }],
            });
        }
    }
    let Some((when, at)) = cx.global::<Shell>().power else { return };
    if rows.iter().any(|r| matches!(r.status, Status::Downloading | Status::Queued)) {
        cancel_power(cx); // something new started: the queue isn't done after all
    } else if Instant::now() >= at {
        cx.global_mut::<Shell>().power = None;
        match when {
            WhenDone::Sleep => crate::system::sleep_now(),
            WhenDone::ShutDown => crate::system::shut_down(),
            WhenDone::Nothing => {}
        }
    }
}

/// Install the downloaded update and quit; the new version starts itself.
fn restart_to_update(cx: &mut App) {
    match cx.global::<Shell>().updater.install() {
        Ok(()) => cx.quit(),
        Err(err) => eprintln!("shard: {err}"),
    }
}

fn cancel_power(cx: &mut App) {
    cx.global_mut::<Shell>().power = None;
    cx.dismiss_system_notification("power");
}

fn notification_response(response: SystemNotificationResponse, cx: &mut App) {
    match (response.tag.as_ref(), response.action_id.as_deref()) {
        ("power", _) => cancel_power(cx),
        ("clip", Some("download")) => {
            if let Some(url) = cx.global_mut::<Shell>().clip.take()
                && let Err(err) = cx.global::<Shell>().manager.add(&url)
            {
                eprintln!("shard: {err}");
            }
        }
        // Clicking the offer itself opens the window, whose link bar picks up the copied link.
        ("clip", None) => open_main(true, cx),
        (tag, _) => {
            if let Some(id) = tag.strip_prefix("row:").and_then(|id| id.parse().ok()) {
                cx.global_mut::<Shell>().select = Some(id);
            }
            open_main(true, cx);
        }
    }
}

fn tray_event(event: crate::tray::Event, cx: &mut App) {
    use crate::tray::Event;
    let manager = cx.global::<Shell>().manager.clone();
    let change = |set: &dyn Fn(&mut Settings)| {
        let mut settings = manager.settings();
        set(&mut settings);
        if let Err(err) = manager.set_settings(settings) {
            eprintln!("shard: {err}");
        }
    };
    match event {
        // Linux trays don't say where their icon is; the window is the panel there.
        Event::Panel { .. } if cfg!(target_os = "linux") => open_main(true, cx),
        Event::Panel { x, y, w, h } => toggle_panel((x, y, w, h), cx),
        Event::Open => open_main(true, cx),
        Event::PauseAll => manager.pause_all(),
        Event::ResumeAll => manager.start_all(),
        Event::Cap(cap) => change(&|s| s.speed_limit = cap),
        Event::KeepAwake(on) => change(&|s| s.keep_awake = on),
        Event::LaunchAtLogin(on) => change(&|s| s.launch_at_login = on),
        Event::CancelPower => cancel_power(cx),
        Event::RestartToUpdate => restart_to_update(cx),
        Event::Quit => cx.quit(),
    }
}

const TRAY_PANEL: (f32, f32) = (360., 540.);

/// Open the panel by the tray icon (`icon` is its rectangle in physical pixels), or close it.
fn toggle_panel(icon: (f64, f64, f64, f64), cx: &mut App) {
    if let Some(handle) = cx.global_mut::<Shell>().panel.take()
        && handle.update(cx, |_, window, _| window.remove_window()).is_ok()
    {
        return;
    }
    let scale = screen_scale();
    let (x, y, w, h) = (icon.0 / scale, icon.1 / scale, icon.2 / scale, icon.3 / scale);
    let (pw, ph) = (TRAY_PANEL.0 as f64, TRAY_PANEL.1 as f64);
    let screen = cx.primary_display().map(|d| d.bounds());
    let (sx, sy, sw, sh) = screen.map_or((0., 0., 1440., 900.), |b| {
        (b.origin.x.as_f32() as f64, b.origin.y.as_f32() as f64, b.size.width.as_f32() as f64, b.size.height.as_f32() as f64)
    });
    // Centred under a menu-bar icon; above a taskbar icon at the bottom of the screen.
    let left = (x + w / 2. - pw / 2.).clamp(sx + 8., (sx + sw - pw - 8.).max(sx + 8.));
    let top = if y - sy < sh / 2. { y + h + 6. } else { y - ph - 8. };
    let options = WindowOptions {
        kind: WindowKind::PopUp,
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(left as f32), px(top as f32)),
            size: size(px(TRAY_PANEL.0), px(TRAY_PANEL.1)),
        })),
        focus: true,
        show: true,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let manager = cx.global::<Shell>().manager.clone();
    match cx.open_window(options, |window, cx| cx.new(|cx| TrayPanel::new(manager, window, cx))) {
        Ok(handle) => cx.global_mut::<Shell>().panel = Some(handle.into()),
        Err(err) => eprintln!("shard: couldn't open the tray panel: {err:#}"),
    }
}

/// The main screen's backing scale: tray-icon reports physical pixels, windows are placed in points.
#[cfg(target_os = "macos")]
fn screen_scale() -> f64 {
    objc2::MainThreadMarker::new()
        .and_then(objc2_app_kit::NSScreen::mainScreen)
        .map_or(2., |screen| screen.backingScaleFactor())
}

#[cfg(windows)]
fn screen_scale() -> f64 {
    // SAFETY: a plain query with no arguments.
    unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForSystem() as f64 / 96. }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn screen_scale() -> f64 {
    1.
}

/// The tray's panel: a small instrument panel by the tray icon. Closes when it loses focus.
struct TrayPanel {
    manager: Arc<Manager>,
    rows: Vec<Row>,
    throughput: Vec<u64>,
    settings: Settings,
    focus: FocusHandle,
    opened: Instant,
    added: Option<Result<String, String>>,
    _poll: Task<()>,
    _blur: Subscription,
}

impl TrayPanel {
    fn new(manager: Arc<Manager>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        });
        let blur = cx.observe_window_activation(window, |_, window, _| {
            if !window.is_window_active() {
                window.remove_window();
            }
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            rows: manager.rows(),
            throughput: manager.throughput(),
            settings: manager.settings(),
            manager,
            focus,
            opened: Instant::now(),
            added: None,
            _poll: poll,
            _blur: blur,
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.rows = self.manager.rows();
        self.throughput = self.manager.throughput();
        self.settings = self.manager.settings();
        cx.notify();
    }

    fn set_cap(&mut self, cap: u64, cx: &mut Context<Self>) {
        let mut settings = self.manager.settings();
        settings.speed_limit = cap;
        if let Err(err) = self.manager.set_settings(settings) {
            eprintln!("shard: {err}");
        }
        self.refresh(cx);
    }

    /// Close the panel and show a download, or just the window, in the main window.
    fn open(select: Option<Id>, window: &mut Window, cx: &mut Context<Self>) {
        cx.global_mut::<Shell>().select = select;
        window.remove_window();
        cx.defer(|cx| open_main(true, cx));
    }

    fn render_row(&self, row: &Row, cx: &mut Context<Self>) -> impl IntoElement {
        let id = row.id;
        let (parts, status, total) = (row.parts.clone(), row.status, row.total);
        let frac = fraction(row);
        let left = (row.speed > 0 && row.total > row.done).then(|| format_duration((row.total - row.done) / row.speed));
        let meta = [Some(format_speed(row.speed)), left.map(|l| format!("{l} left")), Some(format!("{} conn", row.connections))]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        let manager = self.manager.clone();
        h_flex()
            .id(("tray-row", id as usize))
            .gap(px(10.))
            .p(px(8.))
            .rounded(px(10.))
            .cursor_pointer()
            .hover(|s| s.bg(ink::inset()))
            .on_click(cx.listener(move |_, _, window, cx| Self::open(Some(id), window, cx)))
            .child(type_tile(&extension_label(row), 34., true))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(5.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).child(display_name(row)))
                            .child(
                                div()
                                    .font_family(MONO)
                                    .text_size(px(11.))
                                    .text_color(ink::accent2())
                                    .child(if total > 0 { format!("{:.0}%", frac * 100.) } else { format_bytes(row.done) }),
                            ),
                    )
                    .child(
                        div().h(px(4.)).w_full().child(
                            canvas(
                                |_, _, _| {},
                                move |b, _, window, _| paint_segments(b, &parts, status, total, frac, 0., true, window),
                            )
                            .size_full(),
                        ),
                    )
                    .child(div().font_family(MONO).text_size(px(10.)).text_color(ink::faint()).truncate().child(meta)),
            )
            .child(
                icon_button(("tray-pause", id as usize), Lucide::Pause, "Pause")
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        manager.pause(id);
                    }),
            )
    }

    fn render_finished(&self, row: &Row) -> impl IntoElement {
        let (open, folder) = (row.clone(), row.clone());
        let settings = self.settings.clone();
        h_flex()
            .gap(px(10.))
            .p(px(8.))
            .rounded(px(10.))
            .child(type_tile(&extension_label(row), 34., false))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(3.))
                    .child(div().truncate().text_size(px(12.5)).child(display_name(row)))
                    .child(div().font_family(MONO).text_size(px(10.)).text_color(ink::faint()).child(format_bytes(row.total))),
            )
            .child(icon_button(("tray-file", row.id as usize), Lucide::ExternalLink, "Open").on_click(move |_, _, cx| open_file(&open, cx)))
            .child(
                icon_button(("tray-folder", row.id as usize), Lucide::FolderOpen, "Show in folder")
                    .on_click(move |_, _, cx| reveal(&folder, &settings, cx)),
            )
    }
}

impl Render for TrayPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let running: Vec<Row> = self.rows.iter().filter(|r| r.status == Status::Downloading).cloned().collect();
        let queued = self.rows.iter().filter(|r| r.status == Status::Queued).count();
        let paused = self.rows.iter().any(|r| matches!(r.status, Status::Paused | Status::Failed));
        let speed: u64 = running.iter().map(|r| r.speed).sum();
        let (value, unit) = speed_parts(speed);
        let cap = self.settings.speed_limit;
        let throughput = self.throughput.clone();
        let idle = throughput.iter().all(|v| *v == 0);
        let link = clipboard_link(cx);
        // A short fade and rise as it opens.
        let t = ease_out((self.opened.elapsed().as_secs_f32() / 0.18).min(1.));
        if t < 1. {
            window.request_animation_frame();
        }
        let manager = self.manager.clone();

        v_flex()
            .key_context(TRAY)
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &Dismiss, window, _| window.remove_window()))
            .size_full()
            .mt(px(8. * (1. - t)))
            .opacity(t)
            .p(px(14.))
            .gap(px(12.))
            .rounded(px(16.))
            .bg(ink::panel())
            .border_1()
            .border_color(ink::line_strong())
            .font_family(SANS)
            .text_color(ink::text())
            // Header: the mark, and the speed cap.
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(mark(0.9))
                            .child(div().font_family(DISPLAY).font_weight(FontWeight::EXTRA_BOLD).text_size(px(12.)).child("SHARD")),
                    )
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(3.))
                            .rounded_full()
                            .border_1()
                            .border_color(if cap > 0 { ink::accent().opacity(0.5) } else { ink::line_strong() })
                            .child(caps(if cap > 0 { format!("CAP {}", format_speed(cap)) } else { "NO CAP".into() }, ink::faint())),
                    ),
            )
            // The speed, big.
            .child(
                h_flex()
                    .items_end()
                    .justify_between()
                    .child(
                        h_flex()
                            .items_end()
                            .gap(px(6.))
                            .child(
                                div()
                                    .font_family(MONO)
                                    .text_size(px(40.))
                                    .line_height(px(42.))
                                    .text_color(if running.is_empty() { ink::muted() } else { ink::text() })
                                    .child(value),
                            )
                            .child(div().pb(px(6.)).font_family(MONO).text_size(px(13.)).text_color(ink::muted()).child(unit)),
                    )
                    .child(
                        v_flex()
                            .items_end()
                            .gap(px(3.))
                            .pb(px(6.))
                            .child(caps(format!("{} ACTIVE", running.len()), if running.is_empty() { ink::faint() } else { ink::accent2() }))
                            .child(caps(format!("{queued} QUEUED"), ink::faint())),
                    ),
            )
            // The last minute of throughput.
            .child(
                div()
                    .relative()
                    .h(px(64.))
                    .w_full()
                    .rounded(px(10.))
                    .bg(ink::inset())
                    .overflow_hidden()
                    .child(
                        canvas(|_, _, _| {}, move |b, _, window, _| paint_sparkline(b, &throughput, true, window))
                            .absolute()
                            .inset_0(),
                    )
                    .when(idle, |this| {
                        this.child(div().absolute().inset_0().flex().items_center().justify_center().child(caps("IDLE", ink::faint())))
                    }),
            )
            // What's downloading, or what just finished.
            .child({
                let list = v_flex().flex_1().min_h_0().gap(px(2.));
                if !running.is_empty() {
                    let more = running.len().saturating_sub(5);
                    list.children(running.iter().take(5).map(|r| self.render_row(r, cx).into_any_element()))
                        .when(more > 0, |this| this.child(div().pl(px(8.)).child(caps(format!("+{more} MORE"), ink::faint()))))
                } else {
                    let finished: Vec<Row> = self.rows.iter().filter(|r| r.status == Status::Done).take(3).cloned().collect();
                    if finished.is_empty() {
                        list.items_center().justify_center().child(caps("NOTHING DOWNLOADING", ink::faint()))
                    } else {
                        list.child(div().pl(px(8.)).pb(px(2.)).child(caps("RECENTLY FINISHED", ink::faint())))
                            .children(finished.iter().map(|r| self.render_finished(r).into_any_element()))
                    }
                }
            })
            // Add the link on the clipboard.
            .child(match (&self.added, link) {
                (Some(Err(err)), _) => h_flex().h(px(36.)).px(px(10.)).rounded(px(10.)).border_1().border_color(ink::red().opacity(0.5)).child(
                    div().truncate().text_size(px(12.)).text_color(ink::red()).child(err.clone()),
                ),
                (_, Some(url)) => {
                    let manager = manager.clone();
                    h_flex()
                        .h(px(36.))
                        .pl(px(10.))
                        .pr(px(4.))
                        .gap(px(8.))
                        .rounded(px(10.))
                        .bg(ink::inset())
                        .child(Icon::new(Lucide::Link).with_size(px(13.)).text_color(ink::faint()))
                        .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(11.)).text_color(ink::muted()).child(host(&url)))
                        .child(accent_button("tray-add", "Add", Some(Lucide::Plus)).on_click(cx.listener(move |this, _, _, cx| {
                            this.added = Some(manager.add(&url).map(|_| url.clone()));
                            cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
                            this.refresh(cx);
                        })))
                }
                (_, None) => h_flex()
                    .h(px(36.))
                    .px(px(10.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(ink::line())
                    .child(caps("COPY A LINK TO ADD IT HERE", ink::faint())),
            })
            // Controls.
            .child(
                h_flex()
                    .gap(px(6.))
                    .when(!running.is_empty() || queued > 0, |this| {
                        let manager = manager.clone();
                        this.child(outline_button("tray-pause-all", "Pause all", Lucide::Pause).on_click(move |_, _, _| manager.pause_all()))
                    })
                    .when(running.is_empty() && queued == 0 && paused, |this| {
                        let manager = manager.clone();
                        this.child(outline_button("tray-resume-all", "Resume all", Lucide::Play).on_click(move |_, _, _| manager.start_all()))
                    })
                    .children(crate::tray::CAPS[..4].iter().enumerate().map(|(i, &mb)| {
                        const LABELS: [&str; 4] = ["OFF", "1", "5", "10"];
                        let on = cap.div_ceil(MB) == mb;
                        chip(("tray-cap", i), LABELS[i], None, on, true)
                            .on_click(cx.listener(move |this, _, _, cx| this.set_cap(mb * MB, cx)))
                    }))
                    .child(div().flex_1())
                    .child(square_button("tray-open", Lucide::AppWindow, "Open Shard").on_click(cx.listener(|_, _, window, cx| Self::open(None, window, cx))))
                    .child(square_button("tray-quit", Lucide::Power, "Quit Shard").on_click(|_, _, cx| cx.quit())),
            )
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's glob import brings its own `test` attribute along.
    use super::{
        MB, Segment, civil_from_days, format_added, format_bytes, format_duration, host, looks_like_url,
        nice_max, segment_span, speed_parts,
    };

    #[test]
    fn tiny_parts_never_cross() {
        // A 256 KB part of a 100 MB file on a 400px track is about 1px: narrower than the 2px gap.
        let part = |start: f32, end: f32| Segment { start, done: start, end, active: true, speed: 0 };
        for s in [part(0.5, 0.5025), part(0.5, 0.5), part(0.0, 0.001), part(0.999, 1.0)] {
            let (a, z, d) = segment_span(&s, 10., 400., 2.);
            assert!(a <= z && (a..=z).contains(&d), "{s:?} -> {a} {z} {d}");
        }
        let (a, z, _) = segment_span(&part(0.25, 0.5), 0., 400., 2.);
        assert_eq!((a, z), (101., 199.), "wide parts keep their gaps");
    }

    #[test]
    fn formats() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(50 * 1024 * 1024), "50.0 MB");
        assert_eq!(format_bytes(6_114_656_256), "5.69 GB");
        assert_eq!(format_duration(192), "3m 12s");
        assert_eq!(format_duration(7260), "2h 01m");
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_373), (2025, 10, 12));
        assert_eq!(format_added(1000, 1030), "just now");
        assert_eq!(speed_parts(49 * MB), ("49.0".to_string(), "MB/s"));
        assert_eq!(nice_max(13 * MB), 20 * MB);
        assert_eq!(
            host("https://releases.ubuntu.com/24.04/x.iso"),
            "releases.ubuntu.com"
        );
    }

    #[test]
    fn graph_nodes_settle_in_their_slots() {
        use crate::queue::{Row, Segment, Status};
        let parts: Vec<Segment> = (0..6)
            .map(|i| Segment {
                start: i as f32 / 6.,
                done: i as f32 / 6. + 0.05,
                end: (i + 1) as f32 / 6.,
                active: true,
                speed: 1000,
            })
            .collect();
        let row = Row {
            id: 1,
            name: "a.iso".into(),
            url: "https://h/a.iso".into(),
            path: None,
            status: Status::Downloading,
            total: 100,
            done: 30,
            speed: 6000,
            connections: 6,
            resumable: Some(true),
            error: None,
            added: 0,
            parts,
            history: vec![],
            position: 0,
        };
        let mut g = super::GraphAnim::default();
        for _ in 0..200 {
            g.step(Some(&row), 0.016);
        }
        let ys: Vec<f32> = g.nodes.iter().map(|n| n.y).collect();
        // Spread over the graph's height, evenly, at least NODE_PITCH apart.
        let gaps: Vec<f32> = ys.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.iter().all(|g| *g >= super::NODE_PITCH - 0.5 && (g - gaps[0]).abs() < 0.5), "{ys:?}");
    }

    #[test]
    fn urls() {
        assert!(looks_like_url("https://example.com/a.zip"));
        assert!(!looks_like_url("ftp://example.com/a.zip"));
        assert!(!looks_like_url("https://"));
        assert!(!looks_like_url("https://a b"));
    }
}
