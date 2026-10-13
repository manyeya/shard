// A windowed app on Windows, not a console one; `shard get` reattaches to the console it was run from.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod browser;
mod db;
mod engine;
mod logo;
mod queue;
mod system;
mod tray;
mod ui;

use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;

const USAGE: &str = "usage:
  shard              open the download manager
  shard <url>        queue a link (in the Shard that's already open, if there is one)
  shard --hidden     start in the tray, with no window (how launch at login starts it)
  shard serve        run the queue and the browser extension's endpoint without a window
  shard get <url> [output]
                   download in the terminal; run it again to resume";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if !args.is_empty() {
        // SAFETY: plain Win32 call; failing (no parent console) just leaves output unseen.
        unsafe { windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS) };
    }
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["get", url, ref rest @ ..] if rest.len() <= 1 => {
            let rt = tokio::runtime::Runtime::new().expect("start the tokio runtime");
            if let Err(e) = rt.block_on(get(url.to_string(), rest.first().map(PathBuf::from))) {
                eprintln!("\nidm: {e}");
                std::process::exit(1);
            }
        }
        [] => app(None, Window::Shown),
        ["--hidden"] => app(None, Window::Hidden),
        ["serve"] => app(None, Window::None),
        [url] if !url.starts_with('-') => app(Some(url), Window::Shown),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

enum Window {
    Shown,
    /// Only the tray, until it's asked for the window.
    Hidden,
    /// No UI at all: `shard serve`.
    None,
}

fn app(url: Option<&str>, window: Window) {
    // One Shard per user: the browser extension's port doubles as the lock.
    let listener = match std::net::TcpListener::bind(("127.0.0.1", browser::port())) {
        Ok(l) => Some(l),
        Err(_) if browser::running() => {
            // Already running: hand the link over, or bring its window back.
            let result = match url {
                Some(url) => browser::hand_over(url),
                None if matches!(window, Window::Shown) => browser::show(),
                None => Ok(()),
            };
            if let Err(e) = result {
                eprintln!("shard: {e}");
            }
            return;
        }
        Err(e) => {
            eprintln!("shard: port {} is taken ({e}); the browser extension won't reach this Shard", browser::port());
            None
        }
    };

    let data = data_dir();
    if let Err(e) = std::fs::create_dir_all(&data) {
        eprintln!("shard: can't create {}: {e}", data.display());
        std::process::exit(1);
    }
    #[cfg(unix)]
    {
        // The database holds the cookies browser downloads need.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700));
    }
    let rt = tokio::runtime::Runtime::new().expect("start the tokio runtime");
    let manager = match queue::Manager::open(&data.join("shard.db"), downloads_dir(), rt.handle().clone()) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("shard: can't open {}: {e}", data.join("shard.db").display());
            std::process::exit(1);
        }
    };
    if let Some(l) = listener {
        let _guard = rt.enter();
        match l.set_nonblocking(true).and_then(|()| tokio::net::TcpListener::from_std(l)) {
            Ok(l) => drop(rt.spawn(browser::serve(l, manager.clone()))),
            Err(e) => eprintln!("shard: browser integration is off: {e}"),
        }
    }
    if let Some(Err(e)) = url.map(|u| manager.add(u)) {
        eprintln!("shard: {e}");
    }
    match window {
        Window::Shown => ui::run(manager, false),
        Window::Hidden => ui::run(manager, true),
        Window::None => rt.block_on(std::future::pending::<()>()),
    }
}

/// Where the database lives; `SHARD_DATA` overrides it.
fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SHARD_DATA") {
        return d.into();
    }
    let home = std::env::home_dir().unwrap_or_default();
    let env = |k| std::env::var_os(k).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/shard")
    } else if cfg!(windows) {
        env("APPDATA").unwrap_or(home).join("shard")
    } else {
        env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")).join("shard")
    }
}

fn downloads_dir() -> PathBuf {
    std::env::home_dir().unwrap_or_default().join("Downloads")
}

/// Download one URL in the terminal.
async fn get(url: String, out: Option<PathBuf>) -> engine::Result<()> {
    let net = engine::Net::new();
    let src = engine::Source { url, headers: Default::default() };
    let probe = engine::probe(&net, &src).await?;
    let out = out.unwrap_or_else(|| probe.name.clone().into());
    let progress = engine::Progress::default();
    let (_stop, rx) = tokio::sync::watch::channel(false);
    let report = async {
        let mut was = progress.done.load(Relaxed);
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let (done, total) = (progress.done.load(Relaxed), progress.total.load(Relaxed));
            let pct = if total > 0 { format!("{:5.1}%  ", done as f64 * 100.0 / total as f64) } else { String::new() };
            let speed = done.saturating_sub(was) as f64 / 1e6;
            eprint!("\r{pct}{:.1} MB  {speed:.1} MB/s  {} connections   ", done as f64 / 1e6, progress.connections.load(Relaxed));
            was = done;
        }
    };
    tokio::select! {
        r = engine::fetch(&net, &src, probe, &out, 8, &progress, rx) => r?,
        () = report => unreachable!(),
    };
    eprintln!("\rsaved {}                                        ", out.display());
    Ok(())
}
