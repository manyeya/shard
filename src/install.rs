//! Installing Shard: where it goes on each system, the steps that put it there, and the ones that
//! take it out again. The setup window (setup.rs) runs the steps one at a time and draws them;
//! `--silent` runs them with no window, for package managers.
//!
//! - macOS: Shard.app into /Applications, or ~/Applications when that isn't writable.
//! - Windows: Shard.exe into %LOCALAPPDATA%\Programs\Shard, a Start menu shortcut, and an entry in
//!   Settings › Apps whose Uninstall runs `Shard.exe --uninstall`. Nothing needs administrator rights.
//! - Linux: the binary into ~/.local/bin, a desktop entry and icons under ~/.local/share.
//!
//! Everywhere: optionally start at login. The browser extension comes from the browsers' own
//! stores; setup opens Shard's page there in each browser it finds. `SHARD_INSTALL_DIR` puts the
//! app somewhere else.

use crate::engine::sibling;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The app icon at the sizes Linux desktops look for.
const ICONS: &[(u32, &[u8])] = &[
    (32, include_bytes!("../assets/icons/png/shard-32.png")),
    (48, include_bytes!("../assets/icons/png/shard-48.png")),
    (64, include_bytes!("../assets/icons/png/shard-64.png")),
    (128, include_bytes!("../assets/icons/png/shard-128.png")),
    (256, include_bytes!("../assets/icons/png/shard-256.png")),
    (512, include_bytes!("../assets/icons/png/shard-512.png")),
];

const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Shard";
/// Where GPUI registers Shard for notifications on Windows; uninstalling takes it out too.
const NOTIFY_KEY: &str = r"HKCU\Software\Classes\AppUserModelId\dev.shard.app";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Install,
    Uninstall,
}

/// Some when this launch runs setup instead of the download manager: what, and whether silently.
/// `--setup` and `--uninstall` ask for it; otherwise a copy that isn't installed offers it.
pub fn mode(args: &[&str]) -> Option<(Mode, bool)> {
    let silent = args.contains(&"--silent");
    let rest: Vec<&str> = args.iter().copied().filter(|a| *a != "--silent").collect();
    match rest[..] {
        ["--setup"] => Some((Mode::Install, silent)),
        ["--uninstall"] => Some((Mode::Uninstall, silent)),
        [] if std::env::var_os("SHARD_NO_SETUP").is_none() => {
            let exe = std::env::current_exe().ok()?;
            offers_setup(&exe, &home(), &crate::data_dir()).then_some((Mode::Install, silent))
        }
        _ => None,
    }
}

/// Whether a copy at `exe` should offer to install itself when opened.
fn offers_setup(exe: &Path, home: &Path, data: &Path) -> bool {
    let named_setup = exe.file_stem().is_some_and(|s| s.to_string_lossy().to_lowercase().contains("setup"));
    let path = exe.to_string_lossy();
    if named_setup {
        return true;
    }
    if path.contains("/target/") || path.contains(r"\target\") {
        return false; // a development build
    }
    if cfg!(target_os = "macos") {
        // A Shard.app outside Applications: opened from the disk image, or from Downloads.
        bundle(exe).is_some_and(|app| !app.ancestors().any(|p| p.file_name().is_some_and(|n| n == "Applications")))
    } else if cfg!(windows) {
        false // the portable zip runs where it's unpacked; Shard-Setup.exe is named for setup
    } else {
        let managed = ["/usr/", "/opt/", "/nix/", "/snap/"].iter().any(|p| path.starts_with(p));
        !managed && !exe.starts_with(home.join(".local/bin")) && !data.join(RUN_IN_PLACE).exists()
    }
}

/// Left in the data folder when someone picks "Run without installing" on Linux, so a portable
/// copy stops asking.
const RUN_IN_PLACE: &str = "run-in-place";

/// The Shard.app around `exe`, when it's in one.
fn bundle(exe: &Path) -> Option<PathBuf> {
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

/// Where an installed Shard lives.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// What gets copied in: Shard.app on macOS, the binary elsewhere.
    pub app: PathBuf,
    /// The binary to run.
    pub exe: PathBuf,
    pub data: PathBuf,
    home: PathBuf,
}

impl Layout {
    pub fn here() -> Layout {
        let home = home();
        let dir = std::env::var_os("SHARD_INSTALL_DIR").map(PathBuf::from).unwrap_or_else(|| default_dir(&home));
        Layout::in_dir(&dir, &home, &crate::data_dir())
    }

    fn in_dir(dir: &Path, home: &Path, data: &Path) -> Layout {
        let (app, exe) = if cfg!(target_os = "macos") {
            let app = dir.join("Shard.app");
            (app.clone(), app.join("Contents/MacOS/shard"))
        } else if cfg!(windows) {
            (dir.join("Shard.exe"), dir.join("Shard.exe"))
        } else {
            (dir.join("shard"), dir.join("shard"))
        };
        Layout { app, exe, data: data.to_path_buf(), home: home.to_path_buf() }
    }

    /// The version of the Shard installed here, if there is one that says.
    pub fn installed_version(&self) -> Option<String> {
        if !self.exe.exists() {
            return None;
        }
        let out = hidden(&mut std::process::Command::new(&self.exe)).arg("--version").output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        out.status.success().then(|| text.trim().trim_start_matches("shard ").to_string()).filter(|v| !v.is_empty())
    }

    /// Whether this very copy is the installed one.
    pub fn is_running_copy(&self) -> bool {
        source().is_ok_and(|s| same_file(&s, &self.app))
    }

    /// Desktop entries and icons on Linux: under $XDG_DATA_HOME, else ~/.local/share.
    fn share(&self) -> PathBuf {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| self.home.join(".local/share"))
    }
}

fn default_dir(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        if writable(Path::new("/Applications")) { PathBuf::from("/Applications") } else { home.join("Applications") }
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home.join("AppData/Local")).join("Programs").join("Shard")
    } else {
        home.join(".local/bin")
    }
}

/// What this copy would install: the Shard.app it's in on macOS, else the binary itself.
pub fn source() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(if cfg!(target_os = "macos") { bundle(&exe).unwrap_or(exe) } else { exe })
}

/// What the person picked in setup.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Choices {
    pub login: bool,
    /// A desktop shortcut too (Windows).
    pub desktop: bool,
    /// Walk through adding the extension from the browsers' stores.
    pub extension: bool,
    /// Uninstalling: also delete the download history and settings.
    pub forget: bool,
}

impl Default for Choices {
    fn default() -> Self {
        Choices { login: true, desktop: false, extension: true, forget: false }
    }
}

type Work = Box<dyn FnOnce(&dyn Fn(f32)) -> Result<(), String> + Send>;

/// One step of installing or removing, for setup to run and draw.
pub struct Task {
    pub label: &'static str,
    pub detail: String,
    work: Work,
}

impl Task {
    pub(crate) fn new(label: &'static str, detail: impl Into<String>, work: impl FnOnce(&dyn Fn(f32)) -> Result<(), String> + Send + 'static) -> Task {
        Task { label, detail: detail.into(), work: Box::new(work) }
    }

    /// Do it, reporting progress from 0 to 1.
    pub fn run(self, progress: &dyn Fn(f32)) -> Result<(), String> {
        (self.work)(progress)
    }
}

pub fn install_tasks(layout: &Layout, choices: Choices) -> Result<Vec<Task>, String> {
    let source = source()?;
    if cfg!(target_os = "macos") && !source.is_dir() {
        return Err("Setup installs Shard.app, and this copy of Shard isn't inside one.".into());
    }
    let mut tasks = Vec::new();
    if crate::browser::running() {
        tasks.push(Task::new("Closing Shard", "Downloads carry on where they left off", |_| close_running()));
    }
    if !same_file(&source, &layout.app) {
        let (app, dir) = (layout.app.clone(), layout.app.parent().map(Path::to_path_buf).unwrap_or_default());
        tasks.push(Task::new("Copying Shard", tilde(&dir, &layout.home), move |p| copy_app(&source, &app, p)));
    }
    let exe = layout.exe.clone();
    if cfg!(windows) {
        let desktop = choices.desktop;
        let e = exe.clone();
        let detail = if desktop { "Start menu and desktop" } else { "Start menu" };
        tasks.push(Task::new("Adding shortcuts", detail, move |_| {
            shortcut("Programs", &e, true)?;
            shortcut("Desktop", &e, desktop)
        }));
        let e = exe.clone();
        tasks.push(Task::new("Adding to Settings › Apps", "So it can be uninstalled there", move |_| register(&e)));
    } else if cfg!(target_os = "linux") {
        let (e, share) = (exe.clone(), layout.share());
        tasks.push(Task::new("Adding to your apps", tilde(&share.join("applications"), &layout.home), move |_| desktop_entry(&share, &e)));
    }
    let (e, home, on) = (exe.clone(), layout.home.clone(), choices.login);
    let label = if on { "Starting at login" } else { "Not starting at login" };
    tasks.push(Task::new(label, if on { "Waits in the tray" } else { "Open it when you need it" }, move |_| {
        crate::system::set_launch_at_login_for(&home, &e, on)
    }));
    Ok(tasks)
}

pub fn uninstall_tasks(layout: &Layout, choices: Choices) -> Vec<Task> {
    let mut tasks = Vec::new();
    if crate::browser::running() {
        tasks.push(Task::new("Closing Shard", "", |_| close_running()));
    }
    let (exe, home) = (layout.exe.clone(), layout.home.clone());
    tasks.push(Task::new("Not starting at login", "", move |_| crate::system::set_launch_at_login_for(&home, &exe, false)));
    if cfg!(windows) {
        let exe = layout.exe.clone();
        tasks.push(Task::new("Removing shortcuts", "Start menu and desktop", move |_| {
            shortcut("Programs", &exe, false)?;
            shortcut("Desktop", &exe, false)
        }));
        tasks.push(Task::new("Removing from Settings › Apps", "", |_| unregister()));
    } else if cfg!(target_os = "linux") {
        let share = layout.share();
        tasks.push(Task::new("Removing from your apps", "", move |_| remove_desktop_entry(&share)));
    }
    if choices.forget {
        let data = layout.data.clone();
        tasks.push(Task::new("Deleting history and settings", tilde(&data, &layout.home), move |_| remove_dir(&data)));
    }
    let app = layout.app.clone();
    tasks.push(Task::new("Removing Shard", tilde(&app, &layout.home), move |_| remove_app(&app)));
    tasks
}

/// Ask the Shard that's running to quit, and wait until it has.
fn close_running() -> Result<(), String> {
    let _ = crate::browser::quit();
    for _ in 0..75 {
        if !crate::browser::running() {
            std::thread::sleep(Duration::from_millis(300)); // let the old binary close
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err("Shard is still running. Quit it from its tray menu, then try again.".into())
}

/// Copy the app in beside the old one, then swap them, so a failed copy leaves the old one working.
fn copy_app(source: &Path, app: &Path, progress: &dyn Fn(f32)) -> Result<(), String> {
    let dir = app.parent().ok_or("nowhere to install to")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
    let new = sibling(app, ".new");
    crate::update::remove_any(&new);
    if source.is_dir() {
        let ok = std::process::Command::new("ditto").arg(source).arg(&new).status().is_ok_and(|s| s.success());
        if !ok {
            return Err(format!("Couldn't copy Shard into {}", dir.display()));
        }
        // The copy is the app the person just opened and allowed, so it needn't ask again (and
        // macOS won't run it from a randomised read-only path, which would stop updates).
        let _ = std::process::Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(&new).status();
        progress(1.);
    } else {
        copy_file(source, &new, progress).map_err(|e| format!("Couldn't copy Shard into {}: {e}", dir.display()))?;
    }
    let placed = if app.exists() { crate::update::swap(app, &new) } else { std::fs::rename(&new, app) };
    placed.map_err(|e| format!("Couldn't put Shard in {}: {e}", dir.display()))?;
    crate::update::remove_any(&sibling(app, ".old")); // can fail on Windows; the next start cleans up
    Ok(())
}

fn copy_file(from: &Path, to: &Path, progress: &dyn Fn(f32)) -> std::io::Result<()> {
    let mut src = std::fs::File::open(from)?;
    let total = src.metadata()?.len().max(1);
    let mut dst = std::fs::File::create(to)?;
    let mut buf = vec![0; 1 << 20];
    let mut done = 0;
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n])?;
        done += n as u64;
        progress(done as f32 / total as f32);
    }
    dst.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(to, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn desktop_entry(share: &Path, exe: &Path) -> Result<(), String> {
    for (size, png) in ICONS {
        let path = share.join(format!("icons/hicolor/{size}x{size}/apps/shard.png"));
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&path, png).map_err(|e| e.to_string())?;
    }
    let entry = include_str!("../packaging/linux/shard.desktop").replace("Exec=shard", &format!("Exec=\"{}\"", exe.display()));
    let apps = share.join("applications");
    std::fs::create_dir_all(&apps).map_err(|e| e.to_string())?;
    std::fs::write(apps.join("shard.desktop"), entry).map_err(|e| e.to_string())?;
    refresh_desktop(share);
    Ok(())
}

fn remove_desktop_entry(share: &Path) -> Result<(), String> {
    remove_file(&share.join("applications/shard.desktop"))?;
    for (size, _) in ICONS {
        remove_file(&share.join(format!("icons/hicolor/{size}x{size}/apps/shard.png")))?;
    }
    refresh_desktop(share);
    Ok(())
}

/// Tell the desktop its app list changed; it notices on its own soon enough if these are missing.
fn refresh_desktop(share: &Path) {
    if cfg!(target_os = "linux") {
        let _ = std::process::Command::new("update-desktop-database").arg(share.join("applications")).output();
        let _ = std::process::Command::new("gtk-update-icon-cache").args(["-q", "-t"]).arg(share.join("icons/hicolor")).output();
    }
}

fn remove_app(app: &Path) -> Result<(), String> {
    if cfg!(windows) {
        return remove_after_exit(app);
    }
    if app.is_dir() {
        std::fs::remove_dir_all(app).map_err(|e| format!("Couldn't remove {}: {e}", app.display()))
    } else {
        remove_file(app)
    }
}

/// A running Windows program can't delete itself: a hidden shell waits for it to exit, then
/// deletes Shard.exe and its folder (only if nothing else is in it).
#[cfg(windows)]
fn remove_after_exit(exe: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let dir = exe.parent().ok_or("Shard is in an odd place")?;
    let script = format!(
        r#"/c ping 127.0.0.1 -n 3 >nul & del /f /q "{exe}" "{exe}.old" & rmdir "{dir}""#,
        exe = exe.display(),
        dir = dir.display()
    );
    std::process::Command::new("cmd")
        .raw_arg(script)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn remove_after_exit(app: &Path) -> Result<(), String> {
    remove_file(app)
}

fn remove_dir(dir: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Couldn't remove {}: {e}", dir.display())),
        _ => Ok(()),
    }
}

fn remove_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Couldn't remove {}: {e}", path.display())),
        _ => Ok(()),
    }
}

/// A Shard.lnk in the Start menu ("Programs") or on the desktop, made through the shell's own
/// scripting object; paths travel in environment variables, so no quoting can go wrong.
#[cfg(windows)]
fn shortcut(folder: &str, exe: &Path, on: bool) -> Result<(), String> {
    let script = "$l = Join-Path ([Environment]::GetFolderPath($env:SHARD_FOLDER)) 'Shard.lnk'; \
        if ($env:SHARD_ON -eq '1') { $s = (New-Object -ComObject WScript.Shell).CreateShortcut($l); \
        $s.TargetPath = $env:SHARD_EXE; $s.WorkingDirectory = Split-Path $env:SHARD_EXE; \
        $s.Description = 'Download files over parallel connections'; $s.Save() } \
        elseif (Test-Path $l) { Remove-Item $l }";
    let ok = hidden(&mut std::process::Command::new("powershell"))
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("SHARD_FOLDER", folder)
        .env("SHARD_EXE", exe)
        .env("SHARD_ON", if on { "1" } else { "0" })
        .output()
        .is_ok_and(|o| o.status.success());
    ok.then_some(()).ok_or_else(|| format!("Couldn't change the {folder} shortcut"))
}

#[cfg(not(windows))]
fn shortcut(_: &str, _: &Path, _: bool) -> Result<(), String> {
    Ok(())
}

/// The entry in Settings › Apps (and Control Panel's Programs).
#[cfg(windows)]
fn register(exe: &Path) -> Result<(), String> {
    use crate::system::reg;
    let exe_s = exe.display().to_string();
    let dir = exe.parent().map(|d| d.display().to_string()).unwrap_or_default();
    let kb = (std::fs::metadata(exe).map_or(0, |m| m.len()) / 1024).to_string();
    let values = [
        ("DisplayName", "REG_SZ", "Shard".to_string()),
        ("DisplayVersion", "REG_SZ", crate::update::VERSION.to_string()),
        ("Publisher", "REG_SZ", "The Shard Authors".to_string()),
        ("DisplayIcon", "REG_SZ", format!("{exe_s},0")),
        ("InstallLocation", "REG_SZ", dir),
        ("UninstallString", "REG_SZ", format!("\"{exe_s}\" --uninstall")),
        ("QuietUninstallString", "REG_SZ", format!("\"{exe_s}\" --uninstall --silent")),
        ("URLInfoAbout", "REG_SZ", "https://manyeya.github.io/shard/".to_string()),
        ("HelpLink", "REG_SZ", "https://github.com/manyeya/shard/issues".to_string()),
        ("NoModify", "REG_DWORD", "1".to_string()),
        ("NoRepair", "REG_DWORD", "1".to_string()),
        ("EstimatedSize", "REG_DWORD", kb),
    ];
    for (name, kind, value) in values {
        if !reg(&["add", UNINSTALL_KEY, "/v", name, "/t", kind, "/d", &value, "/f"]) {
            return Err(format!("Couldn't write {UNINSTALL_KEY}"));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn unregister() -> Result<(), String> {
    use crate::system::reg;
    for key in [UNINSTALL_KEY, NOTIFY_KEY] {
        if reg(&["query", key]) && !reg(&["delete", key, "/f"]) {
            return Err(format!("Couldn't remove {key}"));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn register(_: &Path) -> Result<(), String> {
    let _ = UNINSTALL_KEY;
    Ok(())
}

#[cfg(not(windows))]
fn unregister() -> Result<(), String> {
    let _ = NOTIFY_KEY;
    Ok(())
}

/// Open the installed Shard.
pub fn launch(layout: &Layout) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(&layout.app);
        c
    } else {
        std::process::Command::new(&layout.exe)
    };
    // Not the AppImage's own variables: the installed copy isn't running from it.
    cmd.env("SHARD_NO_SETUP", "1").env_remove("APPIMAGE").env_remove("APPDIR");
    cmd.spawn().map(drop).map_err(|e| format!("Couldn't open Shard: {e}"))
}

/// Run this copy as the download manager, where it is.
pub fn run_in_place(data: &Path) -> Result<(), String> {
    if cfg!(target_os = "linux") {
        let _ = std::fs::create_dir_all(data);
        let _ = std::fs::write(data.join(RUN_IN_PLACE), b"");
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe).env("SHARD_NO_SETUP", "1").spawn().map(drop).map_err(|e| format!("Couldn't start Shard: {e}"))
}

/// `--silent`: the same steps with defaults, reported on stdout. The exit code says how it went.
pub fn silent(mode: Mode) -> i32 {
    let layout = Layout::here();
    let tasks = match mode {
        Mode::Install => install_tasks(&layout, Choices::default()),
        Mode::Uninstall => Ok(uninstall_tasks(&layout, Choices::default())),
    };
    let tasks = match tasks {
        Ok(t) => t,
        Err(e) => {
            eprintln!("shard: {e}");
            return 1;
        }
    };
    for task in tasks {
        println!("{}…", task.label);
        if let Err(e) = task.run(&|_| {}) {
            eprintln!("shard: {e}");
            return 1;
        }
    }
    match mode {
        Mode::Install => println!("Shard {} is installed: {}", crate::update::VERSION, layout.app.display()),
        Mode::Uninstall => println!("Shard is removed."),
    }
    0
}

/// A store the extension is listed in. Their ids come in at build time (the release workflow
/// passes the repository's variables), or from the same variables at run time for trying it out;
/// a store without one isn't offered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Store {
    Chrome,
    Edge,
    Firefox,
}

impl Store {
    pub fn name(self) -> &'static str {
        match self {
            Store::Chrome => "Chrome Web Store",
            Store::Edge => "Edge Add-ons",
            Store::Firefox => "Firefox Add-ons",
        }
    }

    /// The listing's button, then the one in the browser's "add it?" prompt.
    pub fn buttons(self) -> (&'static str, &'static str) {
        match self {
            Store::Chrome => ("Add to Chrome", "Add extension"),
            Store::Edge => ("Get", "Add extension"),
            Store::Firefox => ("Add to Firefox", "Add"),
        }
    }

    /// Shard's page in this store, once it's listed there.
    pub fn listing(self) -> Option<String> {
        let (var, built, url) = match self {
            Store::Chrome => ("SHARD_CHROME_STORE_ID", option_env!("SHARD_CHROME_STORE_ID"), "https://chromewebstore.google.com/detail/"),
            Store::Edge => ("SHARD_EDGE_STORE_ID", option_env!("SHARD_EDGE_STORE_ID"), "https://microsoftedge.microsoft.com/addons/detail/"),
            Store::Firefox => ("SHARD_FIREFOX_STORE_SLUG", option_env!("SHARD_FIREFOX_STORE_SLUG"), "https://addons.mozilla.org/firefox/addon/"),
        };
        let id = std::env::var(var).ok().or(built.map(String::from)).filter(|id| !id.trim().is_empty())?;
        Some(format!("{url}{}", id.trim()))
    }
}

/// A browser on this computer, and the store its extensions come from.
#[derive(Clone, Debug, PartialEq)]
pub struct Browser {
    pub name: &'static str,
    pub store: Store,
    /// The app bundle (macOS) or binary that opens it.
    path: PathBuf,
}

impl Browser {
    /// Open Shard's page in the store, in this browser.
    pub fn open_listing(&self) -> Result<(), String> {
        let url = self.store.listing().ok_or("Shard isn't in this browser's store yet")?;
        let mut cmd = if cfg!(target_os = "macos") {
            let mut c = std::process::Command::new("open");
            c.arg("-a").arg(&self.path);
            c
        } else {
            std::process::Command::new(&self.path)
        };
        cmd.arg(url).spawn().map(drop).map_err(|e| format!("Couldn't open {}: {e}", self.name))
    }
}

/// (name, its store, where to find it). macOS: app bundles in /Applications or ~/Applications.
/// Windows: paths under %ProgramFiles%, %ProgramFiles(x86)% and %LOCALAPPDATA%. Linux: commands
/// on PATH. Chromium browsers without a store of their own take Chrome Web Store extensions.
const BROWSERS: &[(&str, Store, &[&str])] = if cfg!(target_os = "macos") {
    &[
        ("Chrome", Store::Chrome, &["Google Chrome.app"]),
        ("Arc", Store::Chrome, &["Arc.app"]),
        ("Edge", Store::Edge, &["Microsoft Edge.app"]),
        ("Firefox", Store::Firefox, &["Firefox.app"]),
        ("Brave", Store::Chrome, &["Brave Browser.app"]),
        ("Vivaldi", Store::Chrome, &["Vivaldi.app"]),
        ("Opera", Store::Chrome, &["Opera.app"]),
        ("Chromium", Store::Chrome, &["Chromium.app"]),
    ]
} else if cfg!(windows) {
    &[
        ("Chrome", Store::Chrome, &[r"Google\Chrome\Application\chrome.exe"]),
        ("Edge", Store::Edge, &[r"Microsoft\Edge\Application\msedge.exe"]),
        ("Firefox", Store::Firefox, &[r"Mozilla Firefox\firefox.exe"]),
        ("Brave", Store::Chrome, &[r"BraveSoftware\Brave-Browser\Application\brave.exe"]),
        ("Vivaldi", Store::Chrome, &[r"Vivaldi\Application\vivaldi.exe"]),
        ("Opera", Store::Chrome, &[r"Programs\Opera\opera.exe"]),
        ("Chromium", Store::Chrome, &[r"Chromium\Application\chrome.exe"]),
    ]
} else {
    &[
        ("Chrome", Store::Chrome, &["google-chrome-stable", "google-chrome"]),
        ("Firefox", Store::Firefox, &["firefox"]),
        ("Chromium", Store::Chrome, &["chromium", "chromium-browser"]),
        ("Edge", Store::Edge, &["microsoft-edge-stable", "microsoft-edge"]),
        ("Brave", Store::Chrome, &["brave-browser", "brave"]),
        ("Vivaldi", Store::Chrome, &["vivaldi-stable", "vivaldi"]),
        ("Opera", Store::Chrome, &["opera"]),
    ]
};

/// The browsers on this computer.
pub fn browsers() -> Vec<Browser> {
    let home = home();
    let roots: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec!["/Applications".into(), home.join("Applications")]
    } else if cfg!(windows) {
        ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"].iter().filter_map(std::env::var_os).map(PathBuf::from).collect()
    } else {
        std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default()
    };
    find_browsers(&roots)
}

fn find_browsers(roots: &[PathBuf]) -> Vec<Browser> {
    BROWSERS
        .iter()
        .filter_map(|&(name, store, places)| {
            let path = places.iter().flat_map(|p| roots.iter().map(move |r| r.join(p))).find(|p| p.exists())?;
            // Edge takes Chrome Web Store extensions too, until it has a listing of its own.
            let store = if store == Store::Edge && Store::Edge.listing().is_none() { Store::Chrome } else { store };
            Some(Browser { name, store, path })
        })
        .collect()
}

/// `~/…` for paths in the home folder: what setup shows.
fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        _ => path.display().to_string(),
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".shard-setup-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(probe);
    ok
}

fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_default()
}

/// No console window flashing up when a windowed Windows app runs a command.
fn hidden(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_copies_that_arent_installed_offer_setup() {
        let (home, data) = (Path::new("/Users/ann"), Path::new("/Users/ann/data"));
        assert!(offers_setup(Path::new("/Users/ann/Downloads/Shard-Setup-0.2.0-windows-x64.exe"), home, data));
        assert!(!offers_setup(Path::new("/Users/ann/code/shard/target/release/shard"), home, data));
        if cfg!(target_os = "macos") {
            assert!(offers_setup(Path::new("/Volumes/Shard/Shard.app/Contents/MacOS/shard"), home, data));
            assert!(offers_setup(Path::new("/Users/ann/Downloads/Shard.app/Contents/MacOS/shard"), home, data));
            assert!(!offers_setup(Path::new("/Applications/Shard.app/Contents/MacOS/shard"), home, data));
            assert!(!offers_setup(Path::new("/Users/ann/Applications/Shard.app/Contents/MacOS/shard"), home, data));
            assert!(!offers_setup(Path::new("/Users/ann/bin/shard"), home, data), "a bare binary has no bundle to install");
        } else if cfg!(target_os = "linux") {
            assert!(offers_setup(Path::new("/Users/ann/Downloads/shard-0.2.0-linux-x64/shard"), home, data));
            assert!(!offers_setup(Path::new("/usr/bin/shard"), home, data));
            assert!(!offers_setup(Path::new("/Users/ann/.local/bin/shard"), home, data));
        }
    }

    #[test]
    fn setup_is_asked_for_by_flags() {
        assert_eq!(mode(&["--setup"]), Some((Mode::Install, false)));
        assert_eq!(mode(&["--setup", "--silent"]), Some((Mode::Install, true)));
        assert_eq!(mode(&["--uninstall", "--silent"]), Some((Mode::Uninstall, true)));
        assert_eq!(mode(&["https://example.com/a.zip"]), None);
        assert_eq!(mode(&["serve"]), None);
    }

    #[test]
    fn installing_and_removing_leave_nothing_behind() {
        let root = crate::engine::tests::scratch("install");
        let (home, data) = (root.join("home"), root.join("data"));
        let layout = Layout::in_dir(&root.join("apps"), &home, &data);
        // What gets copied in: a pretend app made of one file (a bundle on macOS).
        let source = root.join(if cfg!(target_os = "macos") { "Shard.app" } else { "shard-setup" });
        if cfg!(target_os = "macos") {
            std::fs::create_dir_all(source.join("Contents/MacOS")).unwrap();
            std::fs::write(source.join("Contents/MacOS/shard"), b"new").unwrap();
        } else {
            std::fs::write(&source, b"new").unwrap();
        }
        let seen = std::sync::Mutex::new(Vec::new());
        copy_app(&source, &layout.app, &|p| seen.lock().unwrap().push(p)).unwrap();
        assert_eq!(std::fs::read(&layout.exe).unwrap(), b"new");
        assert_eq!(seen.lock().unwrap().last(), Some(&1.));
        // Again over the top: the old copy is swapped out, and nothing's left beside it.
        copy_app(&source, &layout.app, &|_| {}).unwrap();
        assert!(!sibling(&layout.app, ".old").exists() && !sibling(&layout.app, ".new").exists());

        let share = home.join(".local/share");
        desktop_entry(&share, &layout.exe).unwrap();
        let entry = std::fs::read_to_string(share.join("applications/shard.desktop")).unwrap();
        assert!(entry.contains(&format!("Exec=\"{}\" %u", layout.exe.display())), "{entry}");
        assert!(share.join("icons/hicolor/512x512/apps/shard.png").exists());
        remove_desktop_entry(&share).unwrap();
        assert!(!share.join("applications/shard.desktop").exists());

        if cfg!(windows) {
            remove_file(&layout.app).unwrap(); // remove_app waits for Shard to exit there
        } else {
            remove_app(&layout.app).unwrap();
            assert!(!layout.app.exists());
            remove_app(&layout.app).unwrap(); // already gone is fine
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn browsers_are_found_where_they_install() {
        let root = crate::engine::tests::scratch("browsers");
        let (name, place) = (BROWSERS[0].0, BROWSERS[0].2[0]);
        let path = root.join(place);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"").unwrap();
        let found = find_browsers(&[root.join("nothing-here"), root.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].name, found[0].path.clone()), (name, path));
        assert_eq!(found[0].store, BROWSERS[0].1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stores_link_to_shards_page_once_listed() {
        // SAFETY: tests that read this variable are only this one.
        unsafe { std::env::set_var("SHARD_FIREFOX_STORE_SLUG", "shard-integration") };
        assert_eq!(Store::Firefox.listing().as_deref(), Some("https://addons.mozilla.org/firefox/addon/shard-integration"));
        unsafe { std::env::set_var("SHARD_FIREFOX_STORE_SLUG", " ") };
        assert_eq!(Store::Firefox.listing(), None, "a blank id means not listed");
        unsafe { std::env::remove_var("SHARD_FIREFOX_STORE_SLUG") };
    }

    #[test]
    fn paths_in_the_home_folder_are_shortened() {
        let home = Path::new("/home/ann");
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(tilde(&home.join(".local").join("bin"), home), format!("~{sep}.local{sep}bin"));
        assert_eq!(tilde(Path::new("/Applications"), home), "/Applications");
    }
}
