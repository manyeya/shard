//! Updates. Each release carries `update.txt`, a manifest: the version, then a line per platform
//! with the build's size, SHA-256 and URL. Next to it is `update.txt.sig`, an Ed25519 signature
//! made with the release key, whose public half is `PUBLIC_KEY`. Every 6 hours Shard reads the
//! latest release's manifest. If it's newer, this platform's build comes down quietly through
//! Shard's own engine (so it resumes), is checked against the manifest, and waits until the user
//! picks "Restart to update".

use crate::engine::{self, Net, Outcome, Progress, Source, sibling};
use crate::queue::Manager;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::{Notify, watch};

const MANIFEST: &str = "https://github.com/manyeya/shard/releases/latest/download/update.txt";
/// The release key's public half; the private half signs `update.txt` in the release workflow.
const PUBLIC_KEY: [u8; 32] = [
    0xef, 0xe3, 0x4c, 0xef, 0x72, 0x23, 0x72, 0xec, 0x7a, 0x8c, 0x74, 0xaa, 0x42, 0x47, 0xce, 0x18, 0x2b, 0x46, 0x00, 0x36, 0x16,
    0x6c, 0x1b, 0x00, 0x4b, 0x67, 0x8b, 0x10, 0x3e, 0xa5, 0xc2, 0x7b,
];
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Up to date, or not checked yet.
    Idle,
    Checking,
    Downloading { version: String, done: u64, total: u64 },
    /// Downloaded and checked: restarting installs it.
    Ready { version: String },
    Failed(String),
    /// This install can't update itself: why, for the user.
    Manual(String),
}

/// One platform's build in a manifest.
#[derive(Clone, Debug, PartialEq)]
struct Asset {
    size: u64,
    sha256: String,
    url: String,
}

#[derive(Debug, PartialEq)]
struct Manifest {
    version: semver::Version,
    assets: Vec<(String, Asset)>,
}

pub struct Updater {
    state: Mutex<State>,
    /// The new Shard.app (macOS) or binary, once it's ready.
    staged: Mutex<Option<PathBuf>>,
    progress: Mutex<Option<Arc<Progress>>>,
    wake: Notify,
    /// Where builds come down to: `<data>/updates`.
    dir: PathBuf,
    net: Arc<Net>,
}

impl Updater {
    /// Start checking: now, then every 6 hours while the settings allow it (and whenever
    /// `check_now` asks).
    pub fn start(manager: Arc<Manager>, data: &Path, rt: &Handle) -> Arc<Updater> {
        let me = Arc::new(Updater {
            state: Mutex::new(State::Idle),
            staged: Mutex::new(None),
            progress: Mutex::new(None),
            wake: Notify::new(),
            dir: data.join("updates"),
            net: Net::new(),
        });
        let updater = me.clone();
        rt.spawn(async move {
            let mut asked = false;
            loop {
                if (asked || manager.settings().auto_update) && std::env::var_os("SHARD_NO_UPDATE").is_none() {
                    updater.check().await;
                }
                tokio::select! {
                    () = tokio::time::sleep(EVERY) => asked = false,
                    () = updater.wake.notified() => asked = true,
                }
            }
        });
        me
    }

    pub fn state(&self) -> State {
        let state = self.state.lock().unwrap().clone();
        match (state, self.progress.lock().unwrap().as_ref()) {
            (State::Downloading { version, .. }, Some(p)) => {
                State::Downloading { version, done: p.done.load(Relaxed), total: p.total.load(Relaxed) }
            }
            (state, _) => state,
        }
    }

    pub fn check_now(&self) {
        self.wake.notify_one();
    }

    fn set(&self, state: State) {
        *self.state.lock().unwrap() = state;
    }

    async fn check(&self) {
        if matches!(self.state(), State::Ready { .. } | State::Downloading { .. }) {
            return;
        }
        if let Err(why) = target() {
            return self.set(State::Manual(why));
        }
        self.set(State::Checking);
        let state = match self.fetch().await {
            Ok(Some(version)) => State::Ready { version },
            Ok(None) => State::Idle,
            Err(e) => State::Failed(e),
        };
        self.set(state);
    }

    /// Read the manifest; when it's newer, download this platform's build and stage it.
    /// Ok(None): already up to date.
    async fn fetch(&self) -> Result<Option<String>, String> {
        let get = |url: String| async move {
            let resp = self.net.client.get(&url).send().await.map_err(|e| e.to_string())?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None); // no release yet
            }
            let resp = resp.error_for_status().map_err(|e| e.to_string())?;
            resp.bytes().await.map(Some).map_err(|e| e.to_string())
        };
        let (Some(text), Some(sig)) = (get(MANIFEST.into()).await?, get(format!("{MANIFEST}.sig")).await?) else { return Ok(None) };
        let manifest = verified(&text, &sig, &PUBLIC_KEY)?;
        let current = semver::Version::parse(VERSION).map_err(|e| e.to_string())?;
        let Some(asset) = (manifest.version > current).then(|| manifest.asset(platform()?)).flatten() else { return Ok(None) };
        let version = manifest.version.to_string();

        let here = self.dir.join(&version);
        std::fs::create_dir_all(&here).map_err(|e| e.to_string())?;
        let name = asset.url.rsplit('/').next().unwrap_or("shard-update");
        let file = here.join(engine::sanitize(name));
        if sha256(&file).as_deref() != Some(&asset.sha256) {
            let progress = Arc::new(Progress::default());
            *self.progress.lock().unwrap() = Some(progress.clone());
            self.set(State::Downloading { version: version.clone(), done: 0, total: asset.size });
            let src = Source { url: asset.url.clone(), ..Default::default() };
            let probe = engine::probe(&self.net, &src).await.map_err(|e| e.to_string())?;
            let (_stop, never) = watch::channel(false);
            let outcome = engine::fetch(&self.net, &src, probe, &file, 4, &progress, never).await;
            *self.progress.lock().unwrap() = None;
            if outcome.map_err(|e| e.to_string())? != Outcome::Done {
                return Err("the update's download stopped".into());
            }
            if sha256(&file).as_deref() != Some(&asset.sha256) {
                let _ = std::fs::remove_file(&file);
                return Err("the update didn't match its checksum".into());
            }
        }
        *self.staged.lock().unwrap() = Some(stage(&file, &here)?);
        Ok(Some(version))
    }

    /// Swap the new build in for this one and start it. The caller quits right after.
    pub fn install(&self) -> Result<(), String> {
        let staged = self.staged.lock().unwrap().clone().ok_or("No update is ready yet.")?;
        let target = target()?;
        swap(&target, &staged).map_err(|e| format!("Couldn't install the update: {e}"))?;
        let exe = if cfg!(target_os = "macos") { target.join("Contents/MacOS/shard") } else { target };
        // The new one waits for this one to let go of the browser port before starting.
        std::process::Command::new(exe)
            .env("SHARD_RESTARTED", "1")
            .spawn()
            .map_err(|e| format!("Installed, but couldn't start it: {e}"))?;
        Ok(())
    }
}

/// The build ready to swap in: the Shard.app unpacked from the macOS zip, else the binary itself.
fn stage(file: &Path, here: &Path) -> Result<PathBuf, String> {
    if cfg!(target_os = "macos") {
        let unpacked = here.join("unpacked");
        let _ = std::fs::remove_dir_all(&unpacked);
        let ok = std::process::Command::new("ditto").args(["-x", "-k"]).arg(file).arg(&unpacked).status().is_ok_and(|s| s.success());
        let app = unpacked.join("Shard.app");
        return if ok && app.join("Contents/MacOS/shard").exists() { Ok(app) } else { Err("the update didn't unpack".into()) };
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    Ok(file.to_path_buf())
}

/// What an update replaces: the Shard.app around this binary on macOS, the binary elsewhere.
/// Err says why this install can't update itself.
fn target() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = if cfg!(target_os = "macos") {
        exe.ancestors()
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .map(Path::to_path_buf)
            .ok_or("Updates install into Shard.app, and this copy isn't one.")?
    } else if cfg!(target_os = "linux") && exe.starts_with("/usr") {
        return Err("Installed by a package manager: update Shard through it.".into());
    } else {
        exe
    };
    let dir = target.parent().ok_or("Shard is in an odd place.")?;
    let probe = dir.join(".shard-update-check");
    std::fs::write(&probe, b"").map_err(|_| format!("Move Shard somewhere you can write to (not {}) to update it.", dir.display()))?;
    let _ = std::fs::remove_file(probe);
    Ok(target)
}

/// Put `new` where `target` is, keeping the old one beside it as `.old` until the next start (a
/// running Windows binary can be renamed but not deleted). If moving the new one in fails, the
/// old one goes back.
pub fn swap(target: &Path, new: &Path) -> std::io::Result<()> {
    let old = sibling(target, ".old");
    remove_any(&old);
    std::fs::rename(target, &old)?;
    let moved = std::fs::rename(new, target).or_else(|_| copy_any(new, target));
    if let Err(e) = moved {
        remove_any(target);
        std::fs::rename(&old, target)?;
        return Err(e);
    }
    Ok(())
}

/// Delete what the last update left behind.
pub fn clean_up() {
    if let Ok(target) = target() {
        remove_any(&sibling(&target, ".old"));
    }
}

pub fn remove_any(path: &Path) {
    let _ = std::fs::remove_dir_all(path).or_else(|_| std::fs::remove_file(path));
}

/// Copy a file or (on macOS, an app bundle) a folder, for when a rename can't cross volumes.
fn copy_any(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        let ok = std::process::Command::new("ditto").arg(from).arg(to).status()?.success();
        return ok.then_some(()).ok_or_else(|| std::io::Error::other("ditto failed"));
    }
    std::fs::copy(from, to).map(drop)
}

fn sha256(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// This platform's key in the manifest.
fn platform() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("macos-universal")
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        Some("windows-x64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux-x64")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("linux-arm64")
    } else {
        None
    }
}

impl Manifest {
    fn asset(&self, platform: &str) -> Option<Asset> {
        self.assets.iter().find(|(p, _)| p == platform).map(|(_, a)| a.clone())
    }
}

/// The manifest, if `sig` is the release key's signature of it.
fn verified(text: &[u8], sig: &[u8], key: &[u8; 32]) -> Result<Manifest, String> {
    let key = VerifyingKey::from_bytes(key).map_err(|e| e.to_string())?;
    let sig = Signature::from_slice(sig).map_err(|_| "the update's signature is malformed".to_string())?;
    key.verify_strict(text, &sig).map_err(|_| "the update's signature doesn't match".to_string())?;
    parse(std::str::from_utf8(text).map_err(|e| e.to_string())?)
}

/// `shard-update 1`, `version X.Y.Z`, then `<platform> <size> <sha256> <url>` lines.
fn parse(text: &str) -> Result<Manifest, String> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some("shard-update 1") {
        return Err("the update's manifest is in a format this version doesn't know".into());
    }
    let version = lines
        .next()
        .and_then(|l| l.strip_prefix("version "))
        .and_then(|v| semver::Version::parse(v.trim()).ok())
        .ok_or("the update's manifest has no version")?;
    let assets = lines
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (platform, size, sha256, url) = (f.next()?, f.next()?.parse().ok()?, f.next()?, f.next()?);
            Some((platform.to_string(), Asset { size, sha256: sha256.to_lowercase(), url: url.to_string() }))
        })
        .collect();
    Ok(Manifest { version, assets })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    const MANIFEST_TEXT: &str = "shard-update 1\nversion 9.1.0\nmacos-universal 1024 ABCDEF https://example.com/Shard-9.1.0-macos-universal.zip\nlinux-x64 2048 0123 https://example.com/shard-9.1.0-linux-x64\n";

    #[test]
    fn a_signed_manifest_is_read_and_a_tampered_one_refused() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = key.verifying_key().to_bytes();
        let sig = key.sign(MANIFEST_TEXT.as_bytes()).to_bytes();
        let m = verified(MANIFEST_TEXT.as_bytes(), &sig, &public).unwrap();
        assert_eq!(m.version, semver::Version::new(9, 1, 0));
        assert_eq!(m.asset("macos-universal").unwrap().sha256, "abcdef");
        assert_eq!(m.asset("linux-x64").unwrap().size, 2048);
        assert!(m.asset("windows-x64").is_none());

        let tampered = MANIFEST_TEXT.replace("example.com", "evil.example");
        assert!(verified(tampered.as_bytes(), &sig, &public).is_err(), "a changed manifest fails its signature");
        let other = SigningKey::from_bytes(&[8; 32]).sign(MANIFEST_TEXT.as_bytes()).to_bytes();
        assert!(verified(MANIFEST_TEXT.as_bytes(), &other, &public).is_err(), "another key's signature fails");
        assert!(verified(MANIFEST_TEXT.as_bytes(), &sig, &PUBLIC_KEY).is_err(), "the release key didn't sign the test one");
    }

    #[test]
    fn newer_means_newer_by_semver() {
        let v = |s| semver::Version::parse(s).unwrap();
        assert!(v("0.1.10") > v("0.1.9") && v("0.2.0") > v("0.1.99") && v("1.0.0") > v("1.0.0-rc.1"));
        assert!(semver::Version::parse(VERSION).is_ok(), "this build's version parses");
    }

    #[test]
    fn swapping_keeps_the_old_one_until_cleaned_up() {
        let dir = crate::engine::tests::scratch("swap");
        let (target, new) = (dir.join("Shard.app"), dir.join("staged/Shard.app"));
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(target.join("version"), "old").unwrap();
        std::fs::write(new.join("version"), "new").unwrap();
        swap(&target, &new).unwrap();
        assert_eq!(std::fs::read_to_string(target.join("version")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(sibling(&target, ".old").join("version")).unwrap(), "old");
        // A second update replaces the leftover .old.
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("version"), "newer").unwrap();
        swap(&target, &new).unwrap();
        assert_eq!(std::fs::read_to_string(sibling(&target, ".old").join("version")).unwrap(), "new");
        // A failed move puts the old one back.
        assert!(swap(&target, &dir.join("missing")).is_err());
        assert_eq!(std::fs::read_to_string(target.join("version")).unwrap(), "newer");
        let _ = std::fs::remove_dir_all(dir);
    }
}
