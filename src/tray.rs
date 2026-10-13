//! The tray (menu-bar) icon: the sliced S, filling from the bottom with the downloads' progress, the
//! speed beside it on macOS, a menu on right click, and a left click that asks for the panel.
//! tray-icon drives it on every platform (on Linux through its StatusNotifierItem backend).

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const MB: u64 = 1024 * 1024;
/// The speed-cap presets the menu offers, in MB/s; 0 is no cap.
pub const CAPS: [u64; 5] = [0, 1, 5, 10, 25];

/// What the tray shows; the UI fills it in from the queue.
#[derive(Clone, PartialEq)]
pub struct Status {
    /// Beside the icon on macOS ("12.3 MB/s"); None when idle.
    pub title: Option<String>,
    /// Hover text, and the menu's first line.
    pub summary: String,
    /// How far the running downloads have got, 0 to 1; None when idle or sizes are unknown.
    pub progress: Option<f32>,
    pub running: bool,
    pub paused: bool,
    pub cap: u64,
    pub keep_awake: bool,
    pub launch_at_login: bool,
}

/// What the user asked of the tray.
pub enum Event {
    /// Toggle the panel; the icon's rectangle in physical screen pixels, top-left origin.
    Panel { x: f64, y: f64, w: f64, h: f64 },
    Open,
    PauseAll,
    ResumeAll,
    /// Bytes per second; 0 is no cap.
    Cap(u64),
    KeepAwake(bool),
    LaunchAtLogin(bool),
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    summary: MenuItem,
    pause: MenuItem,
    resume: MenuItem,
    caps: Vec<CheckMenuItem>,
    awake: CheckMenuItem,
    login: CheckMenuItem,
    shown: Option<Status>,
    /// The progress drawn in the icon, so it's only redrawn when that changes.
    drawn: Option<Option<u8>>,
}

impl Tray {
    /// None when there's no tray to put an icon in (some Linux desktops).
    pub fn new() -> Option<Tray> {
        let summary = MenuItem::with_id("summary", "Shard", false, None);
        let pause = MenuItem::with_id("pause", "Pause all", true, None);
        let resume = MenuItem::with_id("resume", "Resume all", true, None);
        let caps: Vec<CheckMenuItem> = CAPS
            .iter()
            .map(|&c| {
                let label = if c == 0 { "No cap".to_string() } else { format!("{c} MB/s") };
                CheckMenuItem::with_id(format!("cap:{c}"), label, true, c == 0, None)
            })
            .collect();
        let cap_menu = Submenu::with_id("caps", "Speed cap", true);
        for item in &caps {
            cap_menu.append(item).ok()?;
        }
        let awake = CheckMenuItem::with_id("awake", "Keep computer awake while downloading", true, true, None);
        let login = CheckMenuItem::with_id("login", "Launch at login", true, false, None);
        let menu = Menu::new();
        menu.append_items(&[
            &summary,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("open", "Open Shard", true, None),
            &pause,
            &resume,
            &cap_menu,
            &PredefinedMenuItem::separator(),
            &awake,
            &login,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("quit", "Quit Shard", true, None),
        ])
        .ok()?;
        let builder = TrayIconBuilder::new().with_id("shard").with_menu(Box::new(menu)).with_menu_on_left_click(false);
        // A template on macOS: only alpha counts, and the system colours it for the menu bar.
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(icon(None));
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(icon(None));
        let icon = builder
            .with_tooltip("Shard")
            .build()
            .map_err(|e| eprintln!("shard: no tray icon: {e}"))
            .ok()?;
        Some(Tray { icon, summary, pause, resume, caps, awake, login, shown: None, drawn: None })
    }

    pub fn update(&mut self, status: &Status) {
        // Redraw only on a visible change: a percent step, or idle to busy.
        let step = status.progress.map(|p| (p.clamp(0., 1.) * 100.) as u8);
        if self.drawn != Some(step) {
            let drawn = Some(icon(status.progress));
            #[cfg(target_os = "macos")]
            let _ = self.icon.set_icon_templated(drawn);
            #[cfg(not(target_os = "macos"))]
            let _ = self.icon.set_icon(drawn);
            self.drawn = Some(step);
        }
        if self.shown.as_ref() == Some(status) {
            return;
        }
        if cfg!(target_os = "macos") {
            self.icon.set_title(status.title.as_deref());
        }
        let _ = self.icon.set_tooltip(Some(format!("Shard — {}", status.summary)));
        self.summary.set_text(&status.summary);
        self.pause.set_enabled(status.running);
        self.resume.set_enabled(status.paused);
        let mb = status.cap.div_ceil(MB);
        for (item, &c) in self.caps.iter().zip(&CAPS) {
            item.set_checked(c == mb);
        }
        self.awake.set_checked(status.keep_awake);
        self.login.set_checked(status.launch_at_login);
        self.shown = Some(status.clone());
    }

    /// Everything clicked since the last call; never blocks.
    pub fn events(&self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(e) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click { rect, button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                out.push(Event::Panel {
                    x: rect.position.x,
                    y: rect.position.y,
                    w: rect.size.width as f64,
                    h: rect.size.height as f64,
                });
            }
        }
        while let Ok(e) = MenuEvent::receiver().try_recv() {
            let id = e.id.0.as_str();
            out.push(match id {
                "open" => Event::Open,
                "pause" => Event::PauseAll,
                "resume" => Event::ResumeAll,
                "awake" => Event::KeepAwake(self.awake.is_checked()),
                "login" => Event::LaunchAtLogin(self.login.is_checked()),
                "quit" => Event::Quit,
                _ => match id.strip_prefix("cap:").and_then(|c| c.parse::<u64>().ok()) {
                    Some(c) => Event::Cap(c * MB),
                    None => continue,
                },
            });
        }
        out
    }
}

/// The icon: 44px (22pt at 2×). On macOS it's a template, so only alpha matters and the system
/// colours it for the menu bar; elsewhere it's drawn in the accent.
fn icon(progress: Option<f32>) -> Icon {
    let rgba = render(44, progress, !cfg!(target_os = "macos"));
    Icon::from_rgba(rgba, 44, 44).expect("a 44×44 RGBA buffer")
}

/// The S's top and bottom, for filling it bottom-up.
const TOP: f32 = crate::logo::BANDS[0].0;
const BOTTOM: f32 = crate::logo::BANDS[2].1;

/// Whether (x, y) is on the sliced S: in one of its bands, the middle one slid right.
fn on_mark(x: f32, y: f32) -> bool {
    use crate::logo::{BANDS, S, SHIFT};
    BANDS.iter().enumerate().any(|(i, &(top, bottom))| {
        let x = if i == 1 { x - SHIFT } else { x };
        (top..bottom).contains(&y) && S.iter().filter(|c| inside(c, x, y)).count() % 2 == 1
    })
}

/// RGBA pixels, 4×4 supersampled. The S is solid up to `progress`, from the bottom, and faint
/// above it; with no progress it's solid all over.
fn render(size: u32, progress: Option<f32>, colour: bool) -> Vec<u8> {
    let fill_from = progress.map_or(TOP, |p| BOTTOM - (BOTTOM - TOP) * p.clamp(0., 1.));
    let k = 100. / size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (mut lit, mut dim) = (0f32, 0f32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = (px as f32 + (sx as f32 + 0.5) / 4.) * k;
                    let y = (py as f32 + (sy as f32 + 0.5) / 4.) * k;
                    if on_mark(x, y) {
                        if y >= fill_from { lit += 1. } else { dim += 1. }
                    }
                }
            }
            let alpha = (lit + dim * 0.32) / 16.;
            let (r, g, b) = if colour { (0xa3, 0xe6, 0x35) } else { (0, 0, 0) };
            out.extend_from_slice(&[r, g, b, (alpha * 255.).round() as u8]);
        }
    }
    out
}

/// Even-odd point-in-polygon.
fn inside(poly: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut hit = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let ((xi, yi), (xj, yj)) = (poly[i], poly[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            hit = !hit;
        }
        j = i;
    }
    hit
}

#[cfg(test)]
mod tests {
    use super::render;

    fn alpha_at(px: &[u8], size: usize, x: usize, y: usize) -> u8 {
        px[(y * size + x) * 4 + 3]
    }

    #[test]
    fn the_mark_fills_from_the_bottom() {
        let size = 44;
        let strongest = |px: &[u8], rows: std::ops::Range<usize>| rows.flat_map(|y| (0..size).map(move |x| (x, y))).map(|(x, y)| alpha_at(px, size, x, y)).max().unwrap();
        let (upper, lower) = (0..size / 2 - 4, size / 2 + 4..size);
        let half = render(size as u32, Some(0.5), false);
        assert_eq!(strongest(&half, lower.clone()), 255, "half done: solid at the bottom");
        assert!(strongest(&half, upper.clone()) < 100, "half done: faint at the top");
        let idle = render(size as u32, None, false);
        assert_eq!((strongest(&idle, upper.clone()), strongest(&idle, lower)), (255, 255), "idle: the whole S");
        assert!(strongest(&render(size as u32, Some(0.), false), 0..size) < 100, "nothing done: all faint");
        assert_eq!(alpha_at(&idle, size, 0, 0), 0, "transparent around it");
    }
}
