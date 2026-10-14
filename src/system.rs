//! What Shard asks of the operating system: staying awake while downloading, and starting at login.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Keeps the computer from idle-sleeping while switched on. One thread owns the request, because
/// on Windows it belongs to the thread that made it, and the downloads' threads come and go.
pub struct Awake(mpsc::Sender<bool>);

impl Awake {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<bool>();
        std::thread::spawn(move || {
            let mut held = None;
            for on in rx {
                if on && held.is_none() {
                    held = keepawake::Builder::default()
                        .idle(true)
                        .reason("Downloading")
                        .app_name("Shard")
                        .app_reverse_domain("dev.shard.app")
                        .create()
                        .map_err(|e| eprintln!("shard: can't keep the computer awake: {e}"))
                        .ok();
                } else if !on {
                    held = None;
                }
            }
        });
        Awake(tx)
    }

    pub fn set(&self, on: bool) {
        let _ = self.0.send(on);
    }
}

/// Put the computer to sleep (the queue finished, and the settings say so).
pub fn sleep_now() {
    let (program, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("pmset", &["sleepnow"])
    } else if cfg!(windows) {
        ("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"])
    } else {
        ("systemctl", &["suspend"])
    };
    run_power(program, args);
}

/// Shut the computer down (the queue finished, and the settings say so).
pub fn shut_down() {
    let (program, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("osascript", &["-e", "tell application \"System Events\" to shut down"])
    } else if cfg!(windows) {
        ("shutdown", &["/s", "/t", "0"])
    } else {
        ("systemctl", &["poweroff"])
    };
    run_power(program, args);
}

fn run_power(program: &str, args: &[&str]) {
    if let Err(e) = std::process::Command::new(program).args(args).status() {
        eprintln!("shard: couldn't run {program}: {e}");
    }
}

/// Whether Shard starts, hidden in the tray, when the user logs in.
pub fn launch_at_login() -> bool {
    login_entry(&home(), xdg_config()).is_some_and(|p| p.exists()) || registry_entry()
}

pub fn set_launch_at_login(on: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    set_login(&home(), xdg_config(), &exe, on)
}

fn xdg_config() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
}

fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_default()
}

/// The file that starts Shard at login, on the systems that use one.
fn login_entry(home: &Path, xdg_config: Option<PathBuf>) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(home.join("Library/LaunchAgents/dev.shard.app.plist"))
    } else if cfg!(windows) {
        None // the registry's Run key
    } else {
        Some(xdg_config.unwrap_or_else(|| home.join(".config")).join("autostart/shard.desktop"))
    }
}

fn set_login(home: &Path, xdg_config: Option<PathBuf>, exe: &Path, on: bool) -> Result<(), String> {
    let Some(entry) = login_entry(home, xdg_config) else { return set_registry_entry(exe, on) };
    if !on {
        return match std::fs::remove_file(&entry) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let text = if cfg!(target_os = "macos") {
        // Inside Shard.app, start the bundle through Launch Services, so it comes up as an app.
        let bundle = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app"));
        let args = match bundle {
            Some(app) => vec!["/usr/bin/open".into(), "-a".into(), app.display().to_string(), "--args".into(), "--hidden".into()],
            None => vec![exe.display().to_string(), "--hidden".into()],
        };
        let args: String = args.iter().map(|a| format!("\n    <string>{}</string>", xml_escape(a))).collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.shard.app</string>
  <key>ProgramArguments</key>
  <array>{args}
  </array>
  <key>RunAtLoad</key><true/>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#
        )
    } else {
        format!(
            "[Desktop Entry]\nType=Application\nName=Shard\nExec=\"{}\" --hidden\nIcon=shard\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        )
    };
    if let Some(dir) = entry.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&entry, text).map_err(|e| e.to_string())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
fn reg(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000; // no console flashing up from a windowed app
    std::process::Command::new("reg")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(windows)]
fn registry_entry() -> bool {
    reg(&["query", RUN_KEY, "/v", "Shard"])
}

#[cfg(windows)]
fn set_registry_entry(exe: &Path, on: bool) -> Result<(), String> {
    let ok = if on {
        let command = format!("\"{}\" --hidden", exe.display());
        reg(&["add", RUN_KEY, "/v", "Shard", "/t", "REG_SZ", "/d", &command, "/f"])
    } else {
        !registry_entry() || reg(&["delete", RUN_KEY, "/v", "Shard", "/f"])
    };
    ok.then_some(()).ok_or_else(|| format!("couldn't change {RUN_KEY}"))
}

#[cfg(not(windows))]
fn registry_entry() -> bool {
    false
}

#[cfg(not(windows))]
fn set_registry_entry(_: &Path, _: bool) -> Result<(), String> {
    let _ = RUN_KEY;
    Err("there's no registry here".into())
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn login_entry_comes_and_goes() {
        let home = crate::engine::tests::scratch("login");
        let exe = Path::new("/Applications/Shard.app/Contents/MacOS/shard");
        let entry = login_entry(&home, None).unwrap();
        set_login(&home, None, exe, true).unwrap();
        let text = std::fs::read_to_string(&entry).unwrap();
        if cfg!(target_os = "macos") {
            assert!(text.contains("<string>/Applications/Shard.app</string>") && text.contains("<string>--hidden</string>"), "{text}");
        } else {
            assert!(text.contains("--hidden"), "{text}");
        }
        set_login(&home, None, exe, false).unwrap();
        assert!(!entry.exists());
        set_login(&home, None, exe, false).unwrap(); // already off is fine
        std::fs::remove_dir_all(home).unwrap();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod awake_tests {
    use super::Awake;
    use std::time::Duration;

    /// Whether this process holds the assertion; a running Shard may hold its own.
    fn asserted() -> bool {
        let out = std::process::Command::new("pmset").args(["-g", "assertions"]).output().unwrap();
        let me = format!("pid {}(", std::process::id());
        String::from_utf8_lossy(&out.stdout).lines().any(|l| l.contains(&me) && l.contains("Downloading"))
    }

    #[test]
    fn holds_the_mac_awake_only_while_on() {
        let awake = Awake::new();
        awake.set(true);
        std::thread::sleep(Duration::from_millis(300));
        assert!(asserted(), "pmset shows no assertion from Shard");
        awake.set(false);
        std::thread::sleep(Duration::from_millis(300));
        assert!(!asserted(), "the assertion outlived set(false)");
    }
}
