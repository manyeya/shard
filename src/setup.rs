//! The setup window. It installs Shard (or removes it), then walks through adding the browser
//! extension, in the app's instrument-panel look. Everything moves off one clock: the sliced S cuts
//! itself together as the window opens and a ripple runs through the dot field, shards of light
//! stream into it, the install runs as connections fanning out from it to each step while a
//! segmented bar fills, and the extension step plays the browser's clicks on a loop until the
//! extension says hello. Finished, the S closes into one piece for a moment.
//!
//! `SHARD_SETUP_DEMO=1` runs it with pretend steps that change nothing, and moves on by itself.
//! `SHARD_REDUCE_MOTION=1` (or the system's reduce-motion setting) keeps it still.

use std::f32::consts::TAU;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{Icon, Sizable as _, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::browser::Hello;
use crate::install::{self, Browser, Choices, Layout, Mode, Store, Task};
use crate::ui::{DISPLAY, FONTS, MONO, SANS, bezier, glow, ink, switch};

actions!(setup, [Primary, Close]);

const W: f32 = 880.;
const H: f32 = 580.;
const CONTEXT: &str = "Setup";
/// The shortest a step shows as running, so one that takes a millisecond can still be read.
const MIN_STEP: f32 = 0.55;
/// Which way each band of the S flies in from.
const FROM: [f32; 3] = [-1., 1., -1.];

/// Open the setup window; returns when it closes (on macOS the process ends with it).
pub fn run(mode: Mode) -> i32 {
    let app = gpui_kit::application().with_assets(crate::ui::IdmAssets).with_quit_mode(QuitMode::LastWindowClosed);
    app.run(move |cx: &mut App| {
        gpui_kit::init(cx);
        if let Err(err) = cx.text_system().add_fonts(FONTS.map(std::borrow::Cow::Borrowed).to_vec()) {
            eprintln!("shard: couldn't load the bundled fonts: {err:#}");
        }
        crate::ui::apply_theme(cx);
        cx.on_action(|_: &Close, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new("secondary-q", Close, None),
            KeyBinding::new("secondary-w", Close, None),
            KeyBinding::new("enter", Primary, Some(CONTEXT)),
        ]);
        cx.set_menus(vec![Menu::new("Shard Setup").items([MenuItem::action("Quit Setup", Close)])]);
        let focus = std::env::var_os("SHARD_NO_FOCUS").is_none_or(|v| v != "1");
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("Shard Setup".into()),
                traffic_light_position: Some(point(px(16.), px(16.))),
                ..TitleBar::title_bar_options()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(W), px(H)), cx))),
            is_resizable: false,
            focus,
            window_background: WindowBackgroundAppearance::Opaque,
            ..TitleBar::window_options()
        };
        match gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| SetupView::new(mode, window, cx))) {
            Ok(_) if focus => cx.activate(true),
            Ok(_) => {}
            Err(err) => {
                eprintln!("shard: couldn't open the setup window: {err:#}");
                cx.quit();
            }
        }
    });
    0
}

#[derive(Clone, Debug, PartialEq)]
enum Scene {
    Welcome,
    Working,
    Extension,
    Done,
    Failed(String),
}

/// The worker thread's side of a run of steps.
#[derive(Default)]
struct Job {
    current: usize,
    progress: f32,
    error: Option<String>,
    finished: bool,
}

/// A step as drawn: its fill eases toward the worker's progress.
struct Step {
    label: &'static str,
    detail: String,
    shown: f32,
    done_at: Option<Instant>,
}

struct SetupView {
    mode: Mode,
    demo: bool,
    layout: Layout,
    /// The version already installed, if any.
    installed: Option<String>,
    running_copy: bool,
    choices: Choices,
    browsers: Vec<Browser>,
    picked: Vec<bool>,
    scene: Scene,
    born: Instant,
    scene_at: Instant,
    last_frame: Instant,
    /// The streams' clock: it runs faster while installing, and backwards for the finish.
    flow: f32,
    speed: f32,
    /// The logo's centre and size: where it was when the scene changed, and where it is now.
    logo_from: (f32, f32, f32),
    logo_now: (f32, f32, f32),
    /// When ripples went out through the dot field, in seconds since the window opened.
    ripples: Vec<f32>,
    steps: Vec<Step>,
    job: Arc<Mutex<Job>>,
    /// The step drawn as running, and since when.
    at_step: usize,
    step_since: Instant,
    /// The browsers to walk through (indexes into `browsers`), and which one is up.
    guide: Vec<usize>,
    at_browser: usize,
    opened: bool,
    hello: Option<Hello>,
    heard: usize,
    connected: Vec<&'static str>,
    connected_at: Option<Instant>,
    still: bool,
    focus: FocusHandle,
}

impl SetupView {
    fn new(mode: Mode, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let demo = std::env::var_os("SHARD_SETUP_DEMO").is_some();
        let layout = Layout::here();
        let installed = (mode == Mode::Install).then(|| layout.installed_version()).flatten();
        // Only browsers whose store has Shard (any, in the demo).
        let browsers: Vec<Browser> = install::browsers().into_iter().filter(|b| demo || b.store.listing().is_some()).collect();
        let picked = vec![true; browsers.len()];
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let still = reduce_motion();
        let start = logo_place(&Scene::Welcome, mode);
        SetupView {
            mode,
            demo,
            running_copy: layout.is_running_copy(),
            layout,
            installed,
            choices: Choices { extension: mode == Mode::Install && !browsers.is_empty(), ..Choices::default() },
            browsers,
            picked,
            scene: Scene::Welcome,
            born: Instant::now(),
            scene_at: Instant::now(),
            last_frame: Instant::now(),
            flow: 0.,
            speed: 1.,
            logo_from: start,
            logo_now: start,
            ripples: if still { vec![] } else { vec![0.78] },
            steps: Vec::new(),
            job: Arc::default(),
            at_step: 0,
            step_since: Instant::now(),
            guide: Vec::new(),
            at_browser: 0,
            opened: false,
            hello: None,
            heard: 0,
            connected: Vec::new(),
            connected_at: None,
            still,
            focus,
        }
    }

    fn t(&self) -> f32 {
        if self.still { 60. } else { self.born.elapsed().as_secs_f32() }
    }

    fn st(&self) -> f32 {
        if self.still { 60. } else { self.scene_at.elapsed().as_secs_f32() }
    }

    fn go(&mut self, scene: Scene) {
        self.logo_from = self.logo_now;
        self.scene_at = Instant::now();
        if scene == Scene::Done && self.mode == Mode::Install && !self.still {
            self.ripples.push(self.born.elapsed().as_secs_f32() + 0.7); // as the S closes
        }
        self.scene = scene;
    }

    /// Advance everything one frame.
    fn tick(&mut self, dt: f32) {
        let st = self.st();
        let target = match self.scene {
            _ if self.still => 0.,
            Scene::Welcome => 1.,
            Scene::Working => 2.8,
            Scene::Extension => 0.6,
            Scene::Done if self.mode == Mode::Install && st < 1.1 => -2.6,
            Scene::Done => 0.35,
            Scene::Failed(_) => 0.25,
        };
        self.speed += (target - self.speed) * (dt * 3.).min(1.);
        self.flow += self.speed * dt;
        let to = logo_place(&self.scene, self.mode);
        let k = ease_out_cubic((st / 0.8).min(1.));
        self.logo_now = (lerp(self.logo_from.0, to.0, k), lerp(self.logo_from.1, to.1, k), lerp(self.logo_from.2, to.2, k));
        let now = self.born.elapsed().as_secs_f32();
        self.ripples.retain(|r| now - r < 2.);

        match self.scene {
            Scene::Welcome if self.demo && st > 3.2 => self.start(),
            Scene::Working => self.follow_job(dt),
            Scene::Extension => self.listen(),
            _ => {}
        }
    }

    fn follow_job(&mut self, dt: f32) {
        let (current, progress, error, finished) = {
            let j = self.job.lock().unwrap();
            (j.current, j.progress, j.error.clone(), j.finished)
        };
        let at = self.at_step;
        if let Some(step) = self.steps.get_mut(at) {
            let real = if current > at || finished { 1. } else if current == at { progress } else { 0. };
            step.shown += (real.max(step.shown) - step.shown) * (dt * 7.).min(1.);
            let ready = real >= 1. && step.shown > 0.96 && self.step_since.elapsed().as_secs_f32() >= MIN_STEP;
            if ready {
                step.shown = 1.;
                step.done_at = Some(Instant::now());
                self.at_step += 1;
                self.step_since = Instant::now();
            } else if let Some(e) = error.filter(|_| current == at) {
                self.go(Scene::Failed(e));
            }
            return;
        }
        // Every step drawn as done: move on once the last one has had its moment.
        let settled = self.steps.last().and_then(|s| s.done_at).is_none_or(|d| d.elapsed() > Duration::from_millis(650));
        if finished && settled {
            self.finish_job();
        }
    }

    fn finish_job(&mut self) {
        if self.mode == Mode::Uninstall {
            return self.go(Scene::Done);
        }
        self.guide = (0..self.browsers.len()).filter(|&i| self.picked[i]).collect();
        if !self.choices.extension || self.guide.is_empty() {
            return self.go(Scene::Done);
        }
        if !self.demo {
            self.hello = Hello::listen();
        }
        self.at_browser = 0;
        self.opened = false;
        self.go(Scene::Extension);
    }

    /// The extension says hello when it's loaded; in the demo, it does a few seconds after opening.
    fn listen(&mut self) {
        let heard = self.hello.as_ref().map(Hello::heard).unwrap_or(0);
        let pretend = self.demo && self.opened && self.connected_at.is_none() && self.st() > 4.5;
        if (heard > self.heard || pretend) && self.connected_at.is_none() {
            self.heard = heard;
            if let Some(b) = self.current_browser() {
                self.connected.push(b.name);
            }
            self.connected_at = Some(Instant::now());
            if !self.still {
                self.ripples.push(self.born.elapsed().as_secs_f32());
            }
        }
        if self.connected_at.is_some_and(|c| c.elapsed() > Duration::from_millis(1900)) {
            self.next_browser();
        }
        if self.demo && !self.opened && self.st() > 2.5 {
            self.opened = true;
        }
    }

    fn current_browser(&self) -> Option<&Browser> {
        self.guide.get(self.at_browser).map(|&i| &self.browsers[i])
    }

    fn next_browser(&mut self) {
        self.at_browser += 1;
        self.opened = false;
        self.connected_at = None;
        if self.at_browser >= self.guide.len() {
            self.go(Scene::Done);
        } else {
            self.scene_at = Instant::now();
        }
    }

    /// Enter, or the scene's main button.
    fn primary(&mut self, cx: &mut Context<Self>) {
        match self.scene {
            Scene::Welcome | Scene::Failed(_) => self.start(),
            Scene::Working => {}
            Scene::Extension if !self.opened => self.open_browser(),
            Scene::Extension => self.next_browser(),
            Scene::Done if self.mode == Mode::Install => self.open_shard(cx),
            Scene::Done => cx.quit(),
        }
        cx.notify();
    }

    fn start(&mut self) {
        let tasks = if self.demo {
            Ok(demo_tasks(self.mode))
        } else if self.mode == Mode::Install {
            install::install_tasks(&self.layout, self.choices)
        } else {
            Ok(install::uninstall_tasks(&self.layout, self.choices))
        };
        let tasks = match tasks {
            Ok(t) => t,
            Err(e) => return self.go(Scene::Failed(e)),
        };
        self.steps = tasks.iter().map(|t| Step { label: t.label, detail: t.detail.clone(), shown: 0., done_at: None }).collect();
        let job = Arc::new(Mutex::new(Job::default()));
        self.job = job.clone();
        std::thread::spawn(move || {
            let count = tasks.len();
            for (i, task) in tasks.into_iter().enumerate() {
                {
                    let mut j = job.lock().unwrap();
                    (j.current, j.progress) = (i, 0.);
                }
                if let Err(e) = task.run(&|p| job.lock().unwrap().progress = p) {
                    job.lock().unwrap().error = Some(e);
                    return;
                }
            }
            let mut j = job.lock().unwrap();
            (j.current, j.finished) = (count, true);
        });
        self.at_step = 0;
        self.step_since = Instant::now();
        self.go(Scene::Working);
    }

    fn open_browser(&mut self) {
        if let Some(b) = self.current_browser().cloned()
            && !self.demo
            && let Err(e) = b.open_listing()
        {
            eprintln!("shard: {e}");
        }
        self.opened = true;
    }

    fn open_shard(&mut self, cx: &mut Context<Self>) {
        if !self.demo {
            drop(self.hello.take()); // the installed Shard wants the port
            std::thread::sleep(Duration::from_millis(150));
            if let Err(e) = install::launch(&self.layout) {
                eprintln!("shard: {e}");
            }
        }
        cx.quit();
    }

    /// What the logo's bands do this frame: sideways offsets, each band's opacity, the middle
    /// band's shift, and how bright the lock-in flash is.
    fn bands(&self) -> ([f32; 3], [f32; 3], f32, f32) {
        let (t, st) = (self.t(), self.st());
        let shift = crate::logo::SHIFT;
        let first = self.scene == Scene::Welcome && self.born.elapsed().as_secs_f32() < 1.6 && !self.still;
        if first {
            let mut off = [0.; 3];
            let mut alpha = [1.; 3];
            for i in 0..3 {
                let p = ((t - 0.1 - 0.09 * i as f32) / 0.62).clamp(0., 1.);
                off[i] = FROM[i] * 320. * (1. - ease_out_back(p));
                alpha[i] = (p * 3.).min(1.);
            }
            return (off, alpha, shift, flash(t, 0.78));
        }
        let breathe = if self.still { 0. } else { (1.3 * t).sin() };
        match (&self.scene, self.mode) {
            (Scene::Working, Mode::Install) => {
                let off = [0, 1, 2].map(|i| if self.still { 0. } else { 4.5 * (3.2 * t - i as f32 * 1.1).sin() });
                (off, [1.; 3], shift, 0.)
            }
            (Scene::Working | Scene::Done, Mode::Uninstall) => {
                let p = if self.scene == Scene::Done { 1. } else { self.overall() };
                let apart = ease_out_cubic(p) * 46.;
                (FROM.map(|d| d * apart), [1. - 0.6 * p; 3], shift, 0.)
            }
            (Scene::Welcome, Mode::Uninstall) => (FROM.map(|d| d * (3. + 2. * breathe)), [1.; 3], shift, 0.),
            (Scene::Done, Mode::Install) => {
                // The middle band slides home and the S is whole for a moment, then it's cut again.
                let close = ease_in_out(((st - 0.25) / 0.45).clamp(0., 1.));
                let open = ease_out_back(((st - 1.35) / 0.6).clamp(0., 1.));
                let s = if self.still { shift } else { shift * (1. - close + open).min(1.) };
                ([0.; 3], [1.; 3], s, if self.still { 0. } else { flash(st, 0.7) })
            }
            _ => ([0.; 3], [1.; 3], shift + breathe, 0.),
        }
    }

    /// How far through the steps, 0 to 1.
    fn overall(&self) -> f32 {
        if self.steps.is_empty() {
            return 0.;
        }
        self.steps.iter().map(|s| s.shown).sum::<f32>() / self.steps.len() as f32
    }

    fn frame(&self) -> Frame {
        let (offsets, alphas, shift, flash) = self.bands();
        let now = self.born.elapsed().as_secs_f32();
        let t = self.t();
        let scan = (self.scene == Scene::Welcome && !self.still && t > 2.4).then(|| ((t - 2.4) % 5.5) / 1.2).filter(|p| *p < 1.);
        let graph = (self.scene == Scene::Working).then(|| {
            self.steps
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let state = match s.done_at {
                        Some(at) => Edge::Done(at.elapsed().as_secs_f32()),
                        None if i == self.at_step => Edge::Active,
                        None => Edge::Waiting,
                    };
                    (node_y(i, self.steps.len()), state, s.shown)
                })
                .collect()
        });
        Frame {
            t,
            flow: self.flow,
            speed: self.speed,
            logo: self.logo_now,
            offsets,
            alphas,
            shift,
            flash,
            ripples: self.ripples.iter().map(|r| now - r).filter(|a| *a >= 0.).collect(),
            scan,
            graph,
            still: self.still,
            uninstall: self.mode == Mode::Uninstall,
            quiet: match self.scene {
                Scene::Welcome | Scene::Failed(_) => Some((400., 70., W, 230.)),
                Scene::Done => Some((250., 290., W - 250., H - 40.)),
                Scene::Working => Some((30., 50., 330., 140.)),
                _ => None,
            },
        }
    }
}

/// Where the logo sits in each scene: centre x, centre y, size.
fn logo_place(scene: &Scene, mode: Mode) -> (f32, f32, f32) {
    match scene {
        Scene::Welcome | Scene::Failed(_) => (212., 262., 150.),
        Scene::Working if mode == Mode::Install => (150., 262., 112.),
        Scene::Working => (212., 262., 150.),
        Scene::Extension => (68., 72., 0.),
        Scene::Done => (W / 2., 196., 150.),
    }
}

/// The install graph's nodes: their centres' height.
fn node_y(i: usize, n: usize) -> f32 {
    let pitch = 56.;
    262. - (n as f32 - 1.) * pitch / 2. + i as f32 * pitch
}
const NODE_X: f32 = 520.;
const NODE_W: f32 = 312.;

#[derive(Clone, Copy, PartialEq)]
enum Edge {
    Waiting,
    Active,
    /// Done this many seconds ago.
    Done(f32),
}

/// What the backdrop canvas paints this frame.
#[derive(Clone)]
struct Frame {
    t: f32,
    flow: f32,
    speed: f32,
    logo: (f32, f32, f32),
    offsets: [f32; 3],
    alphas: [f32; 3],
    shift: f32,
    flash: f32,
    ripples: Vec<f32>,
    scan: Option<f32>,
    graph: Option<Vec<(f32, Edge, f32)>>,
    still: bool,
    uninstall: bool,
    /// Where text sits: the streams fade out over it.
    quiet: Option<(f32, f32, f32, f32)>,
}

impl Render for SetupView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dt = self.last_frame.elapsed().as_secs_f32().min(0.05);
        self.last_frame = Instant::now();
        self.tick(dt);
        let waiting = matches!(self.scene, Scene::Working | Scene::Extension);
        if !self.still || waiting {
            window.request_animation_frame();
        }
        let frame = self.frame();
        let content = match self.scene.clone() {
            Scene::Welcome => self.render_welcome(cx).into_any_element(),
            Scene::Working => self.render_working().into_any_element(),
            Scene::Extension => self.render_extension(cx).into_any_element(),
            Scene::Done => self.render_done(cx).into_any_element(),
            Scene::Failed(e) => self.render_failed(e, cx).into_any_element(),
        };
        div()
            .id("setup")
            .size_full()
            .relative()
            .overflow_hidden()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Primary, _, cx| this.primary(cx)))
            .bg(ink::bg())
            .text_color(ink::text())
            .font_family(SANS)
            .text_size(px(13.))
            .child(canvas(|_, _, _| {}, move |_, _, window, _| paint_backdrop(&frame, window)).absolute().size_full())
            .child(content)
            .child(self.render_rail())
            .child(TitleBar::new().absolute().top_0().left_0().w_full().h(px(40.)).bg(transparent_black()).border_color(transparent_black()))
    }
}

impl SetupView {
    /// The intro clock for the welcome scene's content: it waits for the logo the first time.
    fn enter_clock(&self) -> f32 {
        if self.still {
            return 60.;
        }
        let first = self.born.elapsed().as_secs_f32() < 3. && self.scene_at.duration_since(self.born) < Duration::from_millis(50);
        if first { self.t() - 0.95 } else { self.st() }
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let e = self.enter_clock();
        let install = self.mode == Mode::Install;
        let (title, button): (String, &'static str) = match (&self.installed, install) {
            (_, false) => ("Remove Shard".into(), "Remove"),
            (None, _) => ("Install Shard".into(), "Install"),
            (Some(v), _) if version_newer(crate::update::VERSION, v) => (format!("Update to {}", crate::update::VERSION), "Update"),
            (Some(v), _) if v == crate::update::VERSION => ("Reinstall Shard".into(), "Reinstall"),
            (Some(_), _) => (format!("Install {}", crate::update::VERSION), "Install"),
        };
        let lead = if install {
            match &self.installed {
                Some(v) if v != crate::update::VERSION => format!("Shard {v} is installed. This puts {} in its place; downloads pick up where they left off.", crate::update::VERSION),
                _ => "Every download in parallel parts, resumed after anything, waiting in the tray when the window's closed.".into(),
            }
        } else {
            "This takes Shard off this computer. Files you downloaded stay where they are.".into()
        };
        let dir = self.layout.app.parent().map(tilde).unwrap_or_default();
        let rows: Vec<AnyElement> = if install {
            let mut rows = vec![
                option_row("Install to", None, div().min_w_0().max_w(px(240.)).truncate().font_family(MONO).text_size(px(12.)).text_color(ink::muted()).child(dir)).into_any_element(),
                option_row(
                    "Start at login",
                    Some("Waits in the tray, out of the way".into()),
                    switch("login", self.choices.login).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.choices.login = !this.choices.login;
                        cx.notify();
                    })),
                )
                .into_any_element(),
            ];
            if cfg!(windows) {
                rows.push(
                    option_row(
                        "Desktop shortcut",
                        None,
                        switch("desktop", self.choices.desktop).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.choices.desktop = !this.choices.desktop;
                            cx.notify();
                        })),
                    )
                    .into_any_element(),
                );
            }
            if !self.browsers.is_empty() {
            rows.push(
                option_row(
                    "Browser extension",
                    Some("Hands your browser's downloads to Shard".into()),
                    switch("extension", self.choices.extension).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.choices.extension = !this.choices.extension;
                        cx.notify();
                    })),
                )
                .into_any_element(),
            );
            }
            if self.choices.extension && !self.browsers.is_empty() {
                rows.push(self.render_browser_chips(cx).into_any_element());
            }
            rows
        } else {
            vec![
                option_row(
                    "Also delete history and settings",
                    Some(tilde(&self.layout.data)),
                    switch("forget", self.choices.forget).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.choices.forget = !this.choices.forget;
                        cx.notify();
                    })),
                )
                .into_any_element(),
            ]
        };
        let row_count = rows.len();

        div()
            .absolute()
            .size_full()
            .child(self.render_wordmark(e))
            .child(
                v_flex()
                    .absolute()
                    .left(px(420.))
                    .top(px(92.))
                    .w(px(412.))
                    .gap(px(18.))
                    .child(enter(e, 0., div().font_family(DISPLAY).font_weight(FontWeight::EXTRA_BOLD).text_size(px(30.)).line_height(px(36.)).child(title)))
                    .child(enter(e, 0.06, div().text_size(px(14.)).line_height(px(21.)).text_color(ink::muted()).child(lead)))
                    .child(enter(
                        e,
                        0.12,
                        v_flex()
                            .w_full()
                            .rounded(px(14.))
                            .border_1()
                            .border_color(ink::line_strong())
                            .bg(ink::panel().opacity(0.86))
                            .children(rows.into_iter().enumerate().map(|(i, r)| {
                                div().when(i + 1 < row_count, |d| d.border_b_1().border_color(ink::line())).child(r)
                            })),
                    ))
                    .child(enter(
                        e,
                        0.18,
                        h_flex()
                            .gap(px(10.))
                            .child(primary_button("go", button, install).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.primary(cx))))
                            .when(install && !self.running_copy, |row| {
                                row.child(ghost_button("in-place", "Run without installing").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    if let Err(e) = install::run_in_place(&this.layout.data) {
                                        eprintln!("shard: {e}");
                                    }
                                    cx.quit();
                                })))
                            })
                            .when(!install, |row| row.child(ghost_button("cancel", "Cancel").on_click(|_, _, cx| cx.quit()))),
                    ))
                    .when(cfg!(not(target_os = "macos")) && install, |col| {
                        col.child(enter(e, 0.24, div().text_size(px(12.)).text_color(ink::faint()).child("Installs for you alone, so no administrator password is needed.")))
                    }),
            )
    }

    fn render_browser_chips(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().flex_wrap().gap(px(6.)).px(px(16.)).pb(px(14.)).children(self.browsers.iter().enumerate().map(|(i, b)| {
            let on = self.picked[i];
            h_flex()
                .id(("browser", i))
                .h(px(28.))
                .px(px(10.))
                .gap(px(7.))
                .rounded(px(8.))
                .border_1()
                .cursor_pointer()
                .border_color(if on { ink::accent().opacity(0.45) } else { ink::line_strong() })
                .bg(if on { ink::accent().opacity(0.08) } else { ink::bg().opacity(0.) })
                .hover(|s| s.bg(ink::inset()))
                .child(div().size(px(7.)).rounded_full().bg(if on { ink::accent() } else { ink::faint() }).when(on, |d| d.shadow(glow(ink::accent(), 0.7, 6.))))
                .child(div().text_size(px(12.5)).text_color(if on { ink::text() } else { ink::muted() }).child(b.name))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.picked[i] = !this.picked[i];
                    cx.notify();
                }))
        }))
    }

    /// SHARD under the logo, each letter rising into place, and the version.
    fn render_wordmark(&self, e: f32) -> impl IntoElement {
        let (cx_, cy, size) = self.logo_now;
        let letters = "SHARD".chars().enumerate().map(move |(i, c)| {
            let p = ((e + 0.25 - i as f32 * 0.05) / 0.42).clamp(0., 1.);
            div().relative().top(px(26. * (1. - ease_out_cubic(p)))).opacity(p).child(c.to_string())
        });
        let under = cy + size / 2. + 26.;
        v_flex()
            .absolute()
            .left(px(cx_ - 120.))
            .top(px(under))
            .w(px(240.))
            .items_center()
            .gap(px(10.))
            .when(self.scene == Scene::Welcome, |col| {
                col.child(
                    h_flex()
                        .h(px(30.))
                        .overflow_hidden()
                        .gap(px(3.))
                        .font_family(DISPLAY)
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_size(px(24.))
                        .children(letters),
                )
                .child(div().opacity(((e - 0.2) / 0.4).clamp(0., 1.)).font_family(MONO).text_size(px(11.5)).text_color(ink::faint()).child(format!(
                    "{} · {}",
                    crate::update::VERSION,
                    platform_name()
                )))
            })
    }

    fn render_working(&self) -> impl IntoElement {
        let n = self.steps.len();
        let pct = (self.overall() * 100.).round() as u32;
        let current = self.steps.get(self.at_step);
        let install = self.mode == Mode::Install;
        let title = if install { "Installing Shard" } else { "Removing Shard" };
        div()
            .absolute()
            .size_full()
            .child(
                v_flex()
                    .absolute()
                    .left(px(48.))
                    .top(px(58.))
                    .gap(px(4.))
                    .child(div().font_family(DISPLAY).font_weight(FontWeight::EXTRA_BOLD).text_size(px(20.)).child(title))
                    .child(div().text_size(px(13.)).text_color(ink::muted()).child(current.map_or("Finishing up".to_string(), |s| format!("{}…", s.label)))),
            )
            .when(install, |d| {
                d.children(self.steps.iter().enumerate().map(|(i, s)| {
                    let y = node_y(i, n);
                    let state = if s.done_at.is_some() { 2 } else if i == self.at_step { 1 } else { 0 };
                    let appear = ((self.st() - 0.15 - i as f32 * 0.07) / 0.35).clamp(0., 1.);
                    step_node(s, state, self.t()).absolute().left(px(NODE_X + 16. * (1. - ease_out_cubic(appear)))).top(px(y - 24.)).opacity(appear)
                }))
            })
            .when(!install, |d| {
                d.child(v_flex().absolute().left(px(450.)).top(px(120.)).w(px(380.)).gap(px(8.)).children(self.steps.iter().enumerate().map(|(i, s)| {
                    let state = if s.done_at.is_some() { 2 } else if i == self.at_step { 1 } else { 0 };
                    step_node(s, state, self.t()).w_full()
                })))
            })
            .child(
                h_flex()
                    .absolute()
                    .left(px(48.))
                    .top(px(430.))
                    .w(px(W - 96.))
                    .items_end()
                    .justify_between()
                    .child(
                        h_flex()
                            .items_end()
                            .gap(px(4.))
                            .child(div().font_family(MONO).text_size(px(44.)).line_height(px(46.)).text_color(ink::accent2()).child(pct.to_string()))
                            .child(div().font_family(MONO).text_size(px(16.)).pb(px(6.)).text_color(ink::muted()).child("%")),
                    )
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(11.5))
                            .pb(px(8.))
                            .text_color(ink::faint())
                            .child(format!("{} of {} steps", self.steps.iter().filter(|s| s.done_at.is_some()).count(), n)),
                    ),
            )
            .child(segment_bar(self.steps.iter().map(|s| s.shown).collect(), self.at_step, self.t()).absolute().left(px(48.)).top(px(488.)).w(px(W - 96.)).h(px(8.)))
    }

    fn render_extension(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(browser) = self.current_browser().cloned() else { return div().into_any_element() };
        let st = self.st();
        let connected = self.connected_at.map(|c| c.elapsed().as_secs_f32());
        let (add, confirm) = browser.store.buttons();
        let store = browser.store.name();
        let many = self.guide.len() > 1;
        let last = self.at_browser + 1 >= self.guide.len();
        let next_label = match (connected.is_some(), last) {
            (_, true) => "Finish",
            (true, false) => "Next browser",
            (false, false) => "Skip this one",
        };
        div()
            .absolute()
            .size_full()
            .child(
                h_flex()
                    .absolute()
                    .left(px(58.))
                    .top(px(62.))
                    .gap(px(10.))
                    .child(mini_mark(20.))
                    .child(div().font_family(MONO).text_size(px(11.5)).text_color(ink::faint()).child(if many {
                        format!("Browser {} of {}", self.at_browser + 1, self.guide.len())
                    } else {
                        "Browser extension".to_string()
                    })),
            )
            .child(mock_store(&browser, st, connected).absolute().left(px(40.)).top(px(104.)))
            .child(
                v_flex()
                    .absolute()
                    .left(px(488.))
                    .top(px(104.))
                    .w(px(352.))
                    .gap(px(18.))
                    .child(enter(st, 0., div().font_family(DISPLAY).font_weight(FontWeight::EXTRA_BOLD).text_size(px(24.)).line_height(px(30.)).child(format!("Add Shard to {}", browser.name))))
                    .child(enter(
                        st,
                        0.05,
                        div().text_size(px(13.5)).line_height(px(20.)).text_color(ink::muted()).child(format!(
                            "The extension hands {}'s downloads to Shard. It comes from the {store}, like any other extension.",
                            browser.name
                        )),
                    ))
                    .child(enter(
                        st,
                        0.1,
                        guide_step(
                            1,
                            format!("Open Shard's page in the {store}"),
                            None,
                            self.opened,
                            h_flex().when(self.opened, |d| {
                                d.child(pill_button("open-again", Lucide::ExternalLink, "Open it again").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.open_browser();
                                    cx.notify();
                                })))
                            }),
                        ),
                    ))
                    .child(enter(st, 0.15, guide_step(2, format!("Click {add}, then {confirm}"), None, connected.is_some(), div())))
                    .child(enter(st, 0.2, status_line(&browser, connected, self.opened, self.hello.is_some() || self.demo, self.t())))
                    .child(enter(
                        st,
                        0.25,
                        h_flex()
                            .gap(px(10.))
                            .child(if self.opened {
                                primary_button("next", next_label, connected.is_some() || last).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.next_browser();
                                    cx.notify();
                                }))
                            } else {
                                primary_button("open", format!("Open {store}"), true).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.open_browser();
                                    cx.notify();
                                }))
                            })
                            .when(!self.opened, |row| {
                                row.child(ghost_button("skip", if last { "Skip" } else { "Skip this one" }).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.next_browser();
                                    cx.notify();
                                })))
                            }),
                    )),
            )
            .into_any_element()
    }

    fn render_done(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.st();
        let install = self.mode == Mode::Install;
        let e = st - 0.9;
        let dir = self.layout.app.parent().map(tilde).unwrap_or_default();
        let mut lines: Vec<(bool, String)> = Vec::new();
        if install {
            lines.push((true, format!("In {dir}")));
            lines.push((self.choices.login, if self.choices.login { "Starts at login, in the tray".into() } else { "Doesn't start at login".into() }));
            if !self.connected.is_empty() {
                lines.push((true, format!("Extension added to {}", self.connected.join(", "))));
            }
        } else {
            lines.push((true, "Files you downloaded are still where you saved them".into()));
            if !self.choices.forget {
                lines.push((true, format!("History and settings kept in {}", tilde(&self.layout.data))));
            }
        }
        v_flex()
            .absolute()
            .left_0()
            .top(px(300.))
            .w_full()
            .items_center()
            .gap(px(14.))
            .child(enter(
                e,
                0.,
                div().font_family(DISPLAY).font_weight(FontWeight::EXTRA_BOLD).text_size(px(30.)).child(if install { "Shard is installed" } else { "Shard is removed" }),
            ))
            .child(enter(
                e,
                0.08,
                v_flex().gap(px(6.)).items_center().children(lines.into_iter().map(|(ok, text)| {
                    h_flex()
                        .gap(px(8.))
                        .child(if ok {
                            Icon::new(Lucide::Check).with_size(px(13.)).text_color(ink::accent()).into_any_element()
                        } else {
                            div().size(px(5.)).mx(px(4.)).rounded_full().bg(ink::faint()).into_any_element()
                        })
                        .child(div().text_size(px(13.5)).text_color(ink::muted()).child(text))
                })),
            ))
            .child(enter(
                e,
                0.16,
                h_flex()
                    .pt(px(10.))
                    .gap(px(10.))
                    .when(install, |row| {
                        row.child(primary_button("launch", "Open Shard", true).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_shard(cx))))
                            .child(ghost_button("close", "Close").on_click(|_, _, cx| cx.quit()))
                    })
                    .when(!install, |row| row.child(primary_button("close", "Close", true).on_click(|_, _, cx| cx.quit()))),
            ))
    }

    fn render_failed(&self, error: String, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.st();
        v_flex()
            .absolute()
            .left(px(420.))
            .top(px(120.))
            .w(px(412.))
            .gap(px(16.))
            .child(enter(st, 0., div().font_family(DISPLAY).font_weight(FontWeight::EXTRA_BOLD).text_size(px(26.)).child("Setup stopped")))
            .child(enter(
                st,
                0.06,
                h_flex()
                    .items_start()
                    .gap(px(10.))
                    .p(px(14.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(ink::red().opacity(0.45))
                    .bg(ink::red().opacity(0.07))
                    .child(Icon::new(Lucide::CircleAlert).with_size(px(15.)).text_color(ink::red()))
                    .child(div().flex_1().text_size(px(13.)).line_height(px(19.)).child(error)),
            ))
            .child(enter(
                st,
                0.12,
                div().text_size(px(13.)).text_color(ink::muted()).child("Nothing that was already there has been lost. Fix what it says, then try again."),
            ))
            .child(enter(
                st,
                0.18,
                h_flex()
                    .gap(px(10.))
                    .child(primary_button("retry", "Try again", true).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.primary(cx))))
                    .child(ghost_button("close", "Close").on_click(|_, _, cx| cx.quit())),
            ))
    }

    /// The steps of setup along the bottom.
    fn render_rail(&self) -> impl IntoElement {
        let names: Vec<&str> = match self.mode {
            Mode::Install if self.choices.extension && !self.browsers.is_empty() => vec!["Choose", "Install", "Browser", "Done"],
            Mode::Install => vec!["Choose", "Install", "Done"],
            Mode::Uninstall => vec!["Confirm", "Remove", "Done"],
        };
        let at = match self.scene {
            Scene::Welcome | Scene::Failed(_) => 0,
            Scene::Working => 1,
            Scene::Extension => 2,
            Scene::Done => names.len() - 1,
        };
        h_flex().absolute().bottom(px(18.)).left_0().w_full().justify_center().gap(px(10.)).children(names.into_iter().enumerate().flat_map(move |(i, name)| {
            let color = if i < at { ink::muted() } else if i == at { ink::text() } else { ink::faint() };
            let mut parts = vec![
                h_flex()
                    .gap(px(7.))
                    .child(
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .bg(if i <= at { ink::accent() } else { ink::line_strong() })
                            .when(i == at, |d| d.shadow(glow(ink::accent(), 0.8, 8.))),
                    )
                    .child(div().text_size(px(11.5)).text_color(color).child(name))
                    .into_any_element(),
            ];
            if i > 0 {
                parts.insert(0, div().w(px(26.)).h(px(1.)).bg(if i <= at { ink::accent().opacity(0.5) } else { ink::line_strong() }).into_any_element());
            }
            parts
        }))
    }
}

/// Fade and slide content in, `delay` seconds into its scene's clock `e`.
fn enter<E: IntoElement>(e: f32, delay: f32, el: E) -> Div {
    let p = ((e - delay) / 0.38).clamp(0., 1.);
    div().relative().left(px(18. * (1. - ease_out_cubic(p)))).opacity(p).child(el)
}

fn option_row(label: &'static str, sub: Option<String>, control: impl IntoElement) -> impl IntoElement {
    h_flex()
        .w_full()
        .min_h(px(54.))
        .px(px(16.))
        .py(px(10.))
        .gap(px(12.))
        .justify_between()
        .child(
            v_flex()
                .min_w(px(110.))
                .flex_1()
                .gap(px(2.))
                .child(div().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child(label))
                .when_some(sub, |col, sub| col.child(div().text_size(px(12.)).text_color(ink::faint()).child(sub))),
        )
        .child(control)
}

fn primary_button(id: &'static str, label: impl Into<SharedString>, lit: bool) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(40.))
        .px(px(22.))
        .gap(px(8.))
        .rounded(px(10.))
        .flex_shrink_0()
        .cursor_pointer()
        .text_size(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .map(|b| {
            if lit {
                b.bg(linear_gradient(180., linear_color_stop(ink::accent2(), 0.), linear_color_stop(ink::accent(), 1.)))
                    .shadow(glow(ink::accent(), 0.4, 18.))
                    .text_color(ink::bg())
            } else {
                b.bg(ink::inset()).border_1().border_color(ink::line_strong()).text_color(ink::text())
            }
        })
        .hover(|s| s.opacity(0.92))
        .active(|s| s.opacity(0.8))
        .child(label.into())
}

fn ghost_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(40.))
        .px(px(16.))
        .rounded(px(10.))
        .flex_shrink_0()
        .cursor_pointer()
        .text_size(px(13.5))
        .text_color(ink::muted())
        .hover(|s| s.bg(ink::inset()).text_color(ink::text()))
        .child(label)
}

fn pill_button(id: &'static str, icon: Lucide, label: &'static str) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(28.))
        .px(px(10.))
        .gap(px(6.))
        .rounded(px(8.))
        .flex_shrink_0()
        .border_1()
        .border_color(ink::line_strong())
        .cursor_pointer()
        .text_size(px(12.))
        .text_color(ink::text())
        .hover(|s| s.bg(ink::inset()))
        .child(Icon::new(icon).with_size(px(12.)).text_color(ink::muted()))
        .child(label)
}

/// One step of the install, as a node the graph's connection runs into. `state`: 0 waiting,
/// 1 running, 2 done.
fn step_node(step: &Step, state: u8, t: f32) -> Div {
    let just = step.done_at.map(|d| d.elapsed().as_secs_f32()).filter(|a| *a < 0.6);
    let border = match state {
        1 => ink::accent().opacity(0.55),
        2 => ink::accent().opacity(0.22 + just.map_or(0., |a| 0.5 * (1. - a / 0.6))),
        _ => ink::line_strong(),
    };
    let marker = match state {
        2 => div().size(px(18.)).flex().items_center().justify_center().rounded_full().bg(ink::accent()).child(Icon::new(Lucide::Check).with_size(px(11.)).text_color(ink::bg())),
        1 => {
            let pulse = 0.5 + 0.5 * (t * 5.).sin();
            div()
                .size(px(18.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(ink::accent().opacity(0.6))
                .child(div().size(px(8.)).rounded_full().bg(ink::accent2()).shadow(glow(ink::accent(), 0.5 + 0.4 * pulse, 10.)))
        }
        _ => div().size(px(18.)).rounded_full().border_1().border_color(ink::line_strong()),
    };
    h_flex()
        .w(px(NODE_W))
        .h(px(48.))
        .px(px(14.))
        .gap(px(12.))
        .rounded(px(12.))
        .border_1()
        .border_color(border)
        .bg(if state == 1 { ink::card() } else { ink::panel() })
        .when(state == 1, |d| d.shadow(glow(ink::accent(), 0.12, 22.)))
        .child(marker)
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(if state == 0 { ink::muted() } else { ink::text() }).child(step.label))
                .when(!step.detail.is_empty(), |col| {
                    col.child(div().font_family(MONO).text_size(px(10.5)).text_color(ink::faint()).truncate().child(step.detail.clone()))
                }),
        )
}

/// The steps' bar: a part per step, filling as it runs, with a bright cursor on the running one.
fn segment_bar(fills: Vec<f32>, at: usize, t: f32) -> Div {
    div().child(
        canvas(
            |_, _, _| {},
            move |b, _, window, _| {
                let (x0, y0, w, h) = (b.left().as_f32(), b.top().as_f32(), b.size.width.as_f32(), b.size.height.as_f32());
                let n = fills.len().max(1) as f32;
                let gap = 6.;
                let part = (w - gap * (n - 1.)) / n;
                for (i, f) in fills.iter().enumerate() {
                    let a = x0 + i as f32 * (part + gap);
                    let rect = |z: f32| Bounds { origin: point(px(a), px(y0)), size: size(px(z), px(h)) };
                    window.paint_quad(fill(rect(part), ink::inset()).corner_radii(px(3.)));
                    if *f > 0.01 {
                        window.paint_quad(
                            fill(rect(part * f), linear_gradient(90., linear_color_stop(ink::accent(), 0.), linear_color_stop(ink::accent2(), 1.))).corner_radii(px(3.)),
                        );
                    }
                    if i == at && *f < 0.999 {
                        let x = a + part * f;
                        let cursor = Bounds { origin: point(px(x - 1.), px(y0 - 3.)), size: size(px(2.), px(h + 6.)) };
                        window.paint_drop_shadows(cursor, Corners::all(px(1.)), &glow(ink::accent2(), 0.7 + 0.3 * (t * 6.).sin().abs(), 10.));
                        window.paint_quad(fill(cursor, hsla(0., 0., 1., 0.95)).corner_radii(px(1.)));
                    }
                }
            },
        )
        .size_full(),
    )
}

fn guide_step(n: u8, title: String, sub: Option<String>, done: bool, actions: impl IntoElement) -> impl IntoElement {
    h_flex()
        .items_start()
        .gap(px(12.))
        .child(
            div()
                .size(px(22.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .font_family(MONO)
                .text_size(px(11.))
                .map(|d| {
                    if done {
                        d.bg(ink::accent()).text_color(ink::bg()).child(Icon::new(Lucide::Check).with_size(px(12.)))
                    } else {
                        d.border_1().border_color(ink::line_strong()).text_color(ink::muted()).child(n.to_string())
                    }
                }),
        )
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .gap(px(6.))
                .child(div().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).line_height(px(20.)).child(title))
                .when_some(sub, |col, sub| col.child(div().font_family(MONO).text_size(px(11.)).text_color(ink::faint()).child(sub)))
                .child(actions),
        )
}

fn status_line(browser: &Browser, connected: Option<f32>, opened: bool, listening: bool, t: f32) -> impl IntoElement {
    let (dot, text, color) = match connected {
        Some(_) => (ink::accent(), format!("Shard is in {}", browser.name), ink::accent2()),
        None if !opened => (ink::faint(), format!("{} opens on Shard's page", browser.name), ink::faint()),
        None if listening => (ink::accent().opacity(0.35 + 0.35 * (t * 3.).sin().abs()), format!("Waiting for {} to add it…", browser.name), ink::muted()),
        None => (ink::faint(), "Setup can't hear the browser from here; move on once it's added".to_string(), ink::faint()),
    };
    h_flex()
        .gap(px(9.))
        .px(px(12.))
        .h(px(36.))
        .rounded(px(10.))
        .bg(if connected.is_some() { ink::accent().opacity(0.08) } else { ink::panel() })
        .border_1()
        .border_color(if connected.is_some() { ink::accent().opacity(0.4) } else { ink::line() })
        .child(div().size(px(8.)).rounded_full().bg(dot).when(connected.is_some(), |d| d.shadow(glow(ink::accent(), 0.8, 10.))))
        .child(div().text_size(px(12.5)).text_color(color).child(text))
}

/// Shard's store page in miniature, acting out the two clicks on a 6-second loop: the pointer
/// clicks the store's button, the browser asks, the pointer confirms, and the browser says it's
/// added. Once the extension really says hello, the loop stops there, lit.
fn mock_store(browser: &Browser, st: f32, connected: Option<f32>) -> Div {
    const MW: f32 = 410.;
    const MH: f32 = 320.;
    let lit = connected.is_some();
    let u = if lit { 4. } else { (st + 0.2) % 6. };
    let (add, confirm) = browser.store.buttons();
    let pressed = |at: f32| (u > at && u < at + 0.22) as u8 as f32;
    let prompt = smooth(1.55, 1.75, u) * (1. - smooth(2.7, 2.85, u));
    let added = smooth(2.85, 3.1, u);
    let fade = if lit { 1. } else { 1. - smooth(5.5, 5.95, u) };
    // The pointer: rests, goes to the button, clicks; goes to the prompt's button, clicks; leaves.
    let keys = [(0., (352., 292.)), (0.55, (352., 292.)), (1.25, (330., 122.)), (1.6, (330., 122.)), (2.3, (296., 221.)), (2.75, (296., 221.)), (3.4, (340., 286.)), (6.0, (340., 286.))];
    let pointer = keyframes(&keys, u);
    let clicks = [1.35, 2.6];
    let address = match browser.store {
        Store::Chrome => "chromewebstore.google.com/detail/shard-integration",
        Store::Edge => "microsoftedge.microsoft.com/addons/detail/shard",
        Store::Firefox => "addons.mozilla.org/firefox/addon/shard-integration",
    };
    let name = browser.name;
    let skeleton = |w: f32| div().w(px(w)).h(px(7.)).rounded(px(4.)).bg(ink::line_strong().opacity(0.7));

    div()
        .w(px(MW))
        .h(px(MH))
        .rounded(px(14.))
        .border_1()
        .border_color(ink::line_strong())
        .bg(ink::panel())
        .overflow_hidden()
        .shadow(vec![BoxShadow { color: hsla(0., 0., 0., 0.5), offset: point(px(0.), px(18.)), blur_radius: px(40.), spread_radius: px(0.), inset: false }])
        .relative()
        // Toolbar: three dots and the address.
        .child(
            h_flex()
                .h(px(38.))
                .px(px(14.))
                .gap(px(14.))
                .border_b_1()
                .border_color(ink::line())
                .child(h_flex().gap(px(6.)).children([0, 1, 2].map(|_| div().size(px(9.)).rounded_full().bg(ink::line_strong()))))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .h(px(24.))
                        .px(px(10.))
                        .gap(px(6.))
                        .rounded(px(7.))
                        .bg(ink::inset())
                        .child(Icon::new(Lucide::Globe).with_size(px(11.)).text_color(ink::faint()))
                        .child(div().min_w_0().truncate().font_family(MONO).text_size(px(10.5)).text_color(ink::muted()).child(address)),
                ),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(38.))
                .size_full()
                .opacity(fade)
                .child(div().absolute().left(px(18.)).top(px(14.)).text_size(px(11.5)).text_color(ink::faint()).child(browser.store.name()))
                // The listing.
                .child(
                    div()
                        .absolute()
                        .left(px(18.))
                        .top(px(40.))
                        .w(px(MW - 38.))
                        .h(px(84.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(if lit { ink::accent().opacity(0.55) } else { ink::line_strong() })
                        .bg(ink::card())
                        .when(lit, |d| d.shadow(glow(ink::accent(), 0.18 + 0.4 * (1. - connected.unwrap_or(9.) / 1.2).max(0.), 24.)))
                        .child(
                            div()
                                .absolute()
                                .left(px(14.))
                                .top(px(16.))
                                .size(px(52.))
                                .rounded(px(12.))
                                .bg(ink::bg())
                                .border_1()
                                .border_color(ink::line())
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(mini_mark(30.)),
                        )
                        .child(
                            v_flex()
                                .absolute()
                                .left(px(80.))
                                .top(px(20.))
                                .gap(px(4.))
                                .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("Shard integration"))
                                .child(div().text_size(px(11.)).text_color(ink::faint()).child("The Shard Authors")),
                        )
                        .child(
                            h_flex()
                                .absolute()
                                .left(px(244.))
                                .top(px(26.))
                                .w(px(114.))
                                .h(px(32.))
                                .gap(px(6.))
                                .justify_center()
                                .rounded(px(16.))
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .map(|b| {
                                    if added > 0.5 {
                                        b.border_1().border_color(ink::line_strong()).text_color(ink::muted()).child(Icon::new(Lucide::Check).with_size(px(11.))).child("Added")
                                    } else {
                                        b.bg(mix(ink::white2(), ink::accent2(), pressed(1.35))).text_color(ink::bg()).child(add)
                                    }
                                }),
                        ),
                )
                // The rest of the page, as placeholder lines.
                .child(v_flex().absolute().left(px(18.)).top(px(146.)).gap(px(10.)).child(skeleton(300.)).child(skeleton(352.)).child(skeleton(240.)).child(skeleton(320.)))
                // "Added" toast.
                .when(added > 0., |d| {
                    d.child(
                        h_flex()
                            .absolute()
                            .right(px(12.))
                            .top(px(8. - 8. * (1. - added)))
                            .opacity(added)
                            .gap(px(8.))
                            .px(px(12.))
                            .h(px(32.))
                            .rounded(px(9.))
                            .bg(ink::inset())
                            .border_1()
                            .border_color(if lit { ink::accent().opacity(0.6) } else { ink::line_strong() })
                            .child(Icon::new(Lucide::Check).with_size(px(12.)).text_color(ink::accent()))
                            .child(div().text_size(px(11.)).child(if lit { format!("Connected to Shard in {name}") } else { "Shard integration was added".into() })),
                    )
                }),
        )
        // The browser's "add it?" prompt.
        .when(prompt > 0., |d| {
            d.child(
                v_flex()
                    .absolute()
                    .left(px(60.))
                    .top(px(96. + 10. * (1. - prompt)))
                    .w(px(290.))
                    .opacity(prompt)
                    .p(px(16.))
                    .gap(px(8.))
                    .rounded(px(12.))
                    .bg(ink::card())
                    .border_1()
                    .border_color(ink::line_strong())
                    .shadow(vec![BoxShadow { color: hsla(0., 0., 0., 0.6), offset: point(px(0.), px(12.)), blur_radius: px(30.), spread_radius: px(0.), inset: false }])
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Add \u{201c}Shard integration\u{201d}?"))
                    .child(div().text_size(px(11.)).text_color(ink::faint()).child("It can manage your downloads, and read the cookies a download needs"))
                    .child(
                        h_flex()
                            .pt(px(6.))
                            .justify_end()
                            .gap(px(8.))
                            .child(div().h(px(26.)).px(px(12.)).flex().items_center().rounded(px(13.)).border_1().border_color(ink::line_strong()).text_size(px(11.)).text_color(ink::muted()).child("Cancel"))
                            .child(
                                div()
                                    .h(px(26.))
                                    .px(px(12.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(13.))
                                    .bg(mix(ink::white2(), ink::accent2(), pressed(2.6)))
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(ink::bg())
                                    .child(confirm),
                            ),
                    ),
            )
        })
        // The pointer, with a ripple where it clicks.
        .when(!lit, |d| {
            d.child(
                canvas(
                    |_, _, _| {},
                    move |b, _, window, _| {
                        let (ox, oy) = (b.left().as_f32(), b.top().as_f32());
                        for c in clicks {
                            let age = u - c;
                            if (0. ..0.45).contains(&age) {
                                let at = keyframes(&keys, c);
                                ring(window, ox + at.0, oy + at.1, 4. + age * 40., ink::accent().opacity(0.8 * (1. - age / 0.45)), 1.5);
                            }
                        }
                        paint_pointer(window, ox + pointer.0, oy + pointer.1);
                    },
                )
                .absolute()
                .size_full(),
            )
        })
}

/// An arrow pointer with its tip at (x, y).
fn paint_pointer(window: &mut Window, x: f32, y: f32) {
    let arrow = [(0., 0.), (0., 16.), (4.2, 12.4), (7., 18.6), (9.6, 17.4), (6.9, 11.4), (12., 11.2)];
    for (stroke, color) in [(false, hsla(0., 0., 1., 0.96)), (true, ink::bg())] {
        let mut path = if stroke { PathBuilder::stroke(px(1.)) } else { PathBuilder::fill() };
        path.move_to(point(px(x), px(y)));
        for &(ax, ay) in &arrow[1..] {
            path.line_to(point(px(x + ax), px(y + ay)));
        }
        path.close();
        if let Ok(p) = path.build() {
            window.paint_path(p, color);
        }
    }
}

/// The sliced S, small, for headers and the mock store listing.
fn mini_mark(size: f32) -> impl IntoElement {
    canvas(|_, _, _| {}, |b, _, window, _| {
        let s = b.size.width.as_f32();
        paint_logo(window, (b.left().as_f32() + s / 2., b.top().as_f32() + s / 2.), s, [0.; 3], [1.; 3], crate::logo::SHIFT, None);
    })
    .size(px(size))
    .flex_shrink_0()
}

/// Everything behind the content: the dot field and its ripples, the glow and streams around the
/// logo, the logo, and while installing, the graph's connections.
fn paint_backdrop(f: &Frame, window: &mut Window) {
    let (lx, ly, ls) = f.logo;
    // The dot field: brighter near the logo; a ripple runs through it when the S locks together.
    let pitch = 22.;
    let (cols, rows) = ((W / pitch) as i32 + 1, (H / pitch) as i32 + 1);
    for gy in 0..rows {
        for gx in 0..cols {
            let (x, y) = (gx as f32 * pitch + 11., gy as f32 * pitch + 11.);
            let d = ((x - lx).powi(2) + (y - ly).powi(2)).sqrt();
            let mut lime = 0.;
            for age in &f.ripples {
                let r = age * 470.;
                lime += (-((d - r) / 30.).powi(2)).exp() * (1. - age / 2.).max(0.);
            }
            let near = if ls > 0. { (-d / 230.).exp() } else { 0. };
            let alpha = 0.04 + 0.08 * near + 0.6 * lime.min(1.);
            let color = mix(hsla(0., 0., 1., 1.), ink::accent2(), (lime * 2.).min(1.));
            let s = 1.4 + lime.min(1.) * 1.2;
            window.paint_quad(fill(Bounds { origin: point(px(x - s / 2.), px(y - s / 2.)), size: size(px(s), px(s)) }, color.opacity(alpha)));
        }
    }
    if ls < 1. {
        return;
    }
    // A soft light behind the logo.
    let r = ls * 0.36;
    let breathe = if f.still { 0. } else { 0.03 * (f.t * 1.3).sin() };
    let halo = Bounds { origin: point(px(lx - r), px(ly - r)), size: size(px(2. * r), px(2. * r)) };
    window.paint_drop_shadows(halo, Corners::all(px(r)), &glow(ink::accent(), (0.13 + breathe + 0.35 * f.flash).min(0.7), ls * 0.95));

    // Shards streaming in toward the S (or out, when the clock runs backwards).
    let count = if f.uninstall { 18 } else if f.speed > 1.5 { 70 } else { 44 };
    for i in 0..count {
        let seed = |k: u32| hash(i * 7 + k);
        let theta0 = seed(1) * TAU;
        let r0 = 250. + seed(2) * 330.;
        let v = 46. + seed(3) * 70.;
        let travel = r0 - ls * 0.55;
        let tau = (f.flow * v + seed(4) * travel).rem_euclid(travel);
        let r = r0 - tau;
        let theta = theta0 + 0.5 * (1. - r / r0);
        let len = (8. + 22. * seed(5)) * (0.6 + 0.4 * f.speed.abs().min(3.));
        let head = (lx + r * theta.cos(), ly + r * theta.sin());
        let back = r + len * f.speed.signum();
        let tail = (lx + back * theta.cos(), ly + back * theta.sin());
        let mut alpha = smooth(r0, r0 - 70., r) * smooth(ls * 0.55, ls * 0.95, r) * (0.28 + 0.4 * seed(6));
        if let Some((qx0, qy0, qx1, qy1)) = f.quiet {
            let inside = |(x, y): (f32, f32)| x > qx0 - 30. && x < qx1 + 30. && y > qy0 - 30. && y < qy1 + 30.;
            if inside(head) {
                alpha *= 0.15;
            }
        }
        if alpha < 0.01 {
            continue;
        }
        let color = if seed(7) < 0.72 { ink::accent2() } else { ink::white2() };
        let mut path = PathBuilder::stroke(px(1.2));
        path.move_to(point(px(tail.0), px(tail.1)));
        path.line_to(point(px(head.0), px(head.1)));
        if let Ok(p) = path.build() {
            let angle = (head.0 - tail.0).atan2(tail.1 - head.1).to_degrees();
            window.paint_path(p, linear_gradient(angle, linear_color_stop(color.opacity(0.), 0.), linear_color_stop(color.opacity(alpha), 1.)));
        }
    }

    // The install's connections: logo → each step.
    if let Some(graph) = &f.graph {
        let (sx, sy) = (lx + ls * 0.5 + 10., ly);
        for &(ny, state, shown) in graph {
            let c = [(sx, sy), (sx + 140., sy), (NODE_X - 140., ny), (NODE_X, ny)];
            let mut path = match state {
                Edge::Waiting => PathBuilder::stroke(px(1.)).dash_array(&[px(3.), px(5.)]),
                _ => PathBuilder::stroke(px(if state == Edge::Active { 1.6 } else { 1.3 })),
            };
            path.move_to(point(px(c[0].0), px(c[0].1)));
            path.cubic_bezier_to(point(px(c[3].0), px(c[3].1)), point(px(c[1].0), px(c[1].1)), point(px(c[2].0), px(c[2].1)));
            if let Ok(p) = path.build() {
                let color: Background = match state {
                    Edge::Waiting => ink::line_strong().into(),
                    Edge::Active => linear_gradient(90., linear_color_stop(ink::accent().opacity(0.35), 0.), linear_color_stop(ink::accent2(), 1.)),
                    Edge::Done(age) => ink::accent().opacity(0.35 + 0.5 * (1. - age / 0.6).max(0.)).into(),
                };
                window.paint_path(p, color);
            }
            if state == Edge::Active && !f.still {
                // Packets running along it, faster as the step gets going.
                for k in 0..5 {
                    let u = (f.t * (0.55 + shown * 0.6) + k as f32 / 5.).fract();
                    let (x, y) = bezier(c, u);
                    glow_dot(window, x, y, 2.2, ink::accent2(), 0.9 * (1. - (u - 0.5).abs()));
                }
            }
            let end = match state {
                Edge::Waiting => ink::line_strong(),
                _ => ink::accent(),
            };
            glow_dot(window, NODE_X, ny, 2.5, end, if state == Edge::Waiting { 0. } else { 0.5 });
        }
        glow_dot(window, sx, sy, 3., ink::accent2(), 0.8);
    }

    // The S itself, with ghost trails while its bands fly in.
    for k in (1..=3).rev() {
        let ghost = f.offsets.map(|o| o * (1. + 0.16 * k as f32));
        if f.offsets.iter().any(|o| o.abs() > 3.) && !f.uninstall {
            paint_logo(window, (lx, ly), ls, ghost, f.alphas.map(|a| a * 0.12 / k as f32), f.shift, None);
        }
    }
    paint_logo(window, (lx, ly), ls, f.offsets, f.alphas, f.shift, f.scan);
}

/// The sliced S centred on `c`, `s` wide: each band moved sideways by `offsets` and faded by
/// `alphas`, the middle one shifted `shift` (in the logo's 100-unit square). `scan`, 0 to 1
/// across, runs a line of light down it.
fn paint_logo(window: &mut Window, c: (f32, f32), s: f32, offsets: [f32; 3], alphas: [f32; 3], shift: f32, scan: Option<f32>) {
    use crate::logo::{BANDS, S};
    let k = s / 100.;
    let (x0, y0) = (c.0 - s / 2., c.1 - s / 2.);
    let shape = |dx: f32| {
        let mut paths = Vec::new();
        for contour in S {
            let mut path = PathBuilder::fill();
            path.move_to(point(px(x0 + contour[0].0 * k + dx), px(y0 + contour[0].1 * k)));
            for &(x, y) in &contour[1..] {
                path.line_to(point(px(x0 + x * k + dx), px(y0 + y * k)));
            }
            path.close();
            paths.extend(path.build().ok());
        }
        paths
    };
    for (i, &(top, bottom)) in BANDS.iter().enumerate() {
        if alphas[i] <= 0.01 {
            continue;
        }
        let dx = offsets[i] + if i == 1 { shift * k } else { 0. };
        let a = alphas[i];
        let fill_with: Background = if i == 1 {
            linear_gradient(90., linear_color_stop(ink::accent().opacity(a), 0.), linear_color_stop(ink::accent2().opacity(a), 1.))
        } else {
            linear_gradient(180., linear_color_stop(ink::accent2().opacity(a), 0.), linear_color_stop(ink::accent().opacity(a), 1.))
        };
        let band = Bounds { origin: point(px(x0 - 400.), px(y0 + top * k)), size: size(px(s + 800.), px((bottom - top) * k)) };
        window.with_content_mask(Some(ContentMask { bounds: band }), |window| {
            for p in shape(dx) {
                window.paint_path(p, fill_with);
            }
            if let Some(u) = scan {
                let x = x0 - 0.1 * s + u * 1.2 * s;
                let strip = Bounds { origin: point(px(x), px(y0)), size: size(px(s * 0.07), px(s)) };
                window.with_content_mask(Some(ContentMask { bounds: strip }), |window| {
                    for p in shape(dx) {
                        window.paint_path(p, hsla(0., 0., 1., 0.55 * a));
                    }
                });
            }
        });
    }
}

fn glow_dot(window: &mut Window, x: f32, y: f32, r: f32, color: Hsla, halo: f32) {
    let b = Bounds { origin: point(px(x - r), px(y - r)), size: size(px(2. * r), px(2. * r)) };
    if halo > 0. {
        window.paint_drop_shadows(b, Corners::all(px(r)), &glow(color, halo, r * 4.));
    }
    window.paint_quad(fill(b, color).corner_radii(px(r)));
}

fn ring(window: &mut Window, x: f32, y: f32, r: f32, color: Hsla, width: f32) {
    let mut path = PathBuilder::stroke(px(width));
    for i in 0..=40 {
        let a = i as f32 / 40. * TAU;
        let p = point(px(x + r * a.cos()), px(y + r * a.sin()));
        if i == 0 { path.move_to(p) } else { path.line_to(p) }
    }
    if let Ok(p) = path.build() {
        window.paint_path(p, color);
    }
}

/// Steps that only pretend, for `SHARD_SETUP_DEMO`.
fn demo_tasks(mode: Mode) -> Vec<Task> {
    let names: &[(&'static str, &str, f32)] = match mode {
        Mode::Install => &[
            ("Copying Shard", "/Applications", 1.4),
            ("Starting at login", "Waits in the tray", 0.5),
        ],
        Mode::Uninstall => &[("Closing Shard", "", 0.3), ("Not starting at login", "", 0.3), ("Removing Shard", "/Applications/Shard.app", 0.8)],
    };
    names
        .iter()
        .map(|&(label, detail, secs)| {
            Task::new(label, detail, move |p: &dyn Fn(f32)| {
                for i in 1..=20 {
                    std::thread::sleep(Duration::from_secs_f32(secs / 20.));
                    p(i as f32 / 20.);
                }
                Ok(())
            })
        })
        .collect()
}

/// Whether the system asks apps to keep motion down (or `SHARD_REDUCE_MOTION=1` does).
fn reduce_motion() -> bool {
    if let Some(v) = std::env::var_os("SHARD_REDUCE_MOTION") {
        return v == "1";
    }
    #[cfg(target_os = "macos")]
    {
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW};
        let mut on: i32 = 1;
        // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the pointer.
        let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&mut on as *mut i32).cast(), 0) };
        ok != 0 && on == 0
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "enable-animations"])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "false")
    }
}

fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(windows) {
        "Windows"
    } else {
        "Linux"
    }
}

fn version_newer(a: &str, b: &str) -> bool {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

fn tilde(path: &std::path::Path) -> String {
    let home = std::env::home_dir().unwrap_or_default();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        _ => path.display().to_string(),
    }
}

/// Position along keyframes `(time, (x, y))` at time `u`, eased between each pair.
fn keyframes(keys: &[(f32, (f32, f32))], u: f32) -> (f32, f32) {
    for w in keys.windows(2) {
        let ((t0, a), (t1, b)) = (w[0], w[1]);
        if u <= t1 {
            let k = ease_in_out(((u - t0) / (t1 - t0).max(1e-3)).clamp(0., 1.));
            return (lerp(a.0, b.0, k), lerp(a.1, b.1, k));
        }
    }
    keys.last().map_or((0., 0.), |k| k.1)
}

/// A bright flash at `at` seconds that fades over half a second.
fn flash(t: f32, at: f32) -> f32 {
    if t < at { 0. } else { (-(t - at) * 5.).exp() }
}

fn mix(a: Hsla, b: Hsla, k: f32) -> Hsla {
    let (ra, rb) = (a.to_rgb(), b.to_rgb());
    Rgba { r: lerp(ra.r, rb.r, k), g: lerp(ra.g, rb.g, k), b: lerp(ra.b, rb.b, k), a: lerp(ra.a, rb.a, k) }.into()
}

fn hash(i: u32) -> f32 {
    ((i as f32 * 12.9898).sin() * 43758.545).fract().abs()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn ease_out_cubic(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 { 4. * t * t * t } else { 1. - (-2. * t + 2.).powi(3) / 2. }
}

fn ease_out_back(t: f32) -> f32 {
    let (c1, c3) = (1.70158, 2.70158);
    1. + c3 * (t - 1.).powi(3) + c1 * (t - 1.).powi(2)
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's glob import brings its own `test` attribute along.
    use super::{ease_in_out, ease_out_back, ease_out_cubic, keyframes, node_y};

    #[test]
    fn easing_starts_and_ends_where_it_should() {
        for f in [ease_out_cubic, ease_in_out, ease_out_back] {
            assert!(f(0.).abs() < 1e-4 && (f(1.) - 1.).abs() < 1e-4);
        }
        assert!(ease_out_back(0.7) > 1., "it overshoots on the way in");
        assert_eq!(keyframes(&[(0., (0., 0.)), (1., (10., 20.))], 0.5), (5., 10.));
        assert_eq!(keyframes(&[(0., (0., 0.)), (1., (10., 20.))], 5.), (10., 20.));
    }

    #[test]
    fn the_install_graph_is_centred_on_the_logo() {
        for n in 1..7 {
            let (first, last) = (node_y(0, n), node_y(n - 1, n));
            assert!(((first + last) / 2. - 262.).abs() < 0.01);
            assert!(first > 80. && last < 440., "{n} steps fit between the title and the bar");
        }
    }
}
