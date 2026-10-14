//! The queue manager: every download, which ones run, and what they're called on disk.
//!
//! All methods are sync and cheap, so the UI calls them straight from its thread; downloads run
//! as tasks on the tokio runtime. A download's task is never started twice: a paused download
//! that's resumed while its last task is still stopping waits as Queued until that task ends.

use crate::db::{Db, Saved};
use crate::engine::{self, Net, Outcome, Probe, Progress, Source, sibling};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Handle;
use tokio::sync::watch;

pub type Id = i64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Queued,
    Downloading,
    Paused,
    Done,
    Failed,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Downloading => "downloading",
            Status::Paused => "paused",
            Status::Done => "done",
            Status::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Status {
        match s {
            "downloading" => Status::Downloading,
            "paused" => Status::Paused,
            "done" => Status::Done,
            "failed" => Status::Failed,
            _ => Status::Queued,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Row {
    pub id: Id,
    /// File name, or the URL until the first request picks one.
    pub name: String,
    pub url: String,
    /// Where the finished file is (or will be), once named.
    pub path: Option<PathBuf>,
    pub status: Status,
    /// Bytes; 0 when the server didn't say.
    pub total: u64,
    pub done: u64,
    /// Bytes per second, averaged over the last couple of seconds; 0 unless downloading.
    pub speed: u64,
    pub connections: usize,
    /// Whether the server supports resuming; None until the first request.
    pub resumable: Option<bool>,
    pub error: Option<String>,
    /// Unix seconds.
    pub added: i64,
    /// The segment map, sorted by start. Empty when the size is unknown.
    pub parts: Vec<Segment>,
    /// Speed once a second, oldest first, for the last minute of its latest run.
    pub history: Vec<u64>,
    /// Place among the downloads waiting to run (1 is next); 0 for the rest.
    pub position: usize,
}

/// One part of a download. Positions are fractions of the file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub start: f32,
    pub done: f32,
    pub end: f32,
    /// A connection is fetching it right now.
    pub active: bool,
    /// Bytes per second into this part, averaged over the last couple of seconds.
    pub speed: u64,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub dir: PathBuf,
    /// Downloads running at once; the rest wait as Queued.
    pub max_active: usize,
    /// Connections per download.
    pub connections: usize,
    /// Bytes per second for all downloads together; 0 for no limit.
    pub speed_limit: u64,
    /// Keep the computer from idle-sleeping while anything downloads.
    pub keep_awake: bool,
    /// Start Shard, hidden in the tray, at login. Read from (and written to) the system itself.
    pub launch_at_login: bool,
    /// A system notification when a download finishes or fails.
    pub notify: bool,
    /// Ask for a folder each time a link is added in the window.
    pub ask_where: bool,
    /// Put each file in a folder for its kind: Video, Music, Archives, Documents, Programs, Other.
    pub sort_into_folders: bool,
    /// Offer to download links copied to the clipboard.
    pub watch_clipboard: bool,
    /// Resume paused downloads at this local time, (hour, minute).
    pub start_at: Option<(u8, u8)>,
    /// Pause the queue at this local time.
    pub stop_at: Option<(u8, u8)>,
    /// What to do once the queue has finished.
    pub when_done: WhenDone,
    /// Check for, and download, new versions of Shard.
    pub auto_update: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WhenDone {
    Nothing,
    Sleep,
    ShutDown,
}

impl WhenDone {
    fn as_str(self) -> &'static str {
        match self {
            WhenDone::Nothing => "nothing",
            WhenDone::Sleep => "sleep",
            WhenDone::ShutDown => "shut down",
        }
    }

    fn parse(s: &str) -> WhenDone {
        match s {
            "sleep" => WhenDone::Sleep,
            "shut down" => WhenDone::ShutDown,
            _ => WhenDone::Nothing,
        }
    }
}

/// "HH:MM" to (hour, minute); None for anything else, including an empty string.
pub fn parse_clock(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

pub fn format_clock(t: Option<(u8, u8)>) -> String {
    t.map_or_else(String::new, |(h, m)| format!("{h:02}:{m:02}"))
}

/// The kind of file a name is, by its extension: the folder it goes in with `sort_into_folders`.
pub fn category(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "xz" | "bz2" | "zst" | "iso" | "img" | "dmg" => "Archives",
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" | "wmv" | "flv" | "ts" => "Video",
        "mp3" | "flac" | "wav" | "m4a" | "aac" | "ogg" | "opus" => "Music",
        "pdf" | "epub" | "mobi" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "csv" | "odt" => "Documents",
        "exe" | "msi" | "pkg" | "deb" | "rpm" | "appimage" | "apk" | "app" => "Programs",
        _ => "Other",
    }
}

/// Where a download moves to in the queue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Move {
    Top,
    Up,
    Down,
    Bottom,
}

pub struct Manager {
    me: Weak<Manager>,
    rt: Handle,
    net: Arc<Net>,
    awake: crate::system::Awake,
    /// Asks from another `shard` launch to show the window; the UI watches it change.
    shows: std::sync::atomic::AtomicU64,
    /// Goes up each time the queue finishes (the last download done, nothing waiting).
    drained: std::sync::atomic::AtomicU64,
    inner: Mutex<Inner>,
}

struct Inner {
    db: Db,
    settings: Settings,
    /// Oldest first, which is also queue order.
    items: Vec<Item>,
    /// All downloads' speed together, once a second.
    throughput: VecDeque<u64>,
    /// Whether `awake` is holding the computer awake.
    awake_on: bool,
    /// The local (hour, minute) the scheduler last looked at, so it acts once per minute.
    clock: Option<(u8, u8)>,
}

/// Seconds of speed history kept.
const HISTORY: usize = 60;

fn push(h: &mut VecDeque<u64>, v: u64) {
    if h.len() == HISTORY {
        h.pop_front();
    }
    h.push_back(v);
}

struct Item {
    rec: Saved,
    status: Status,
    headers: HeaderMap,
    progress: Arc<Progress>,
    /// Some while the download's task runs.
    stop: Option<watch::Sender<bool>>,
    speed: u64,
    seen: Option<u64>,
    history: VecDeque<u64>,
    /// The part layout while not running (the live one is in `progress`).
    parts: Vec<(u64, u64, u64, bool)>,
    /// Per part, keyed by its start: done at the last sample, and speed.
    part_speed: HashMap<u64, (u64, u64)>,
    /// Removed while its task was still stopping: drop it when the task ends (deleting the file if true).
    doomed: Option<bool>,
    /// Its link was just replaced: the next run may resume though the server's validator changed.
    refreshed: bool,
}

impl Item {
    fn new(rec: Saved, status: Status) -> Self {
        let headers = parse_headers(&rec.headers);
        let parts = match (&rec.name, status) {
            (Some(n), s) if s != Status::Done => engine::saved_parts(&rec.dir.join(n)),
            _ => Vec::new(),
        };
        Item { rec, status, headers, progress: Arc::default(), stop: None, speed: 0, seen: None, history: VecDeque::new(), parts, part_speed: HashMap::new(), doomed: None, refreshed: false }
    }

    fn path(&self) -> Option<PathBuf> {
        self.rec.name.as_ref().map(|n| self.rec.dir.join(n))
    }

    fn save(&mut self, db: &Db) {
        self.rec.status = self.status.as_str().into();
        if self.stop.is_some() {
            self.rec.done = self.progress.done.load(Relaxed);
            self.rec.total = self.progress.total.load(Relaxed).max(self.rec.total);
        }
        if let Err(e) = db.update(&self.rec) {
            eprintln!("shard: saving download {}: {e}", self.rec.id);
        }
    }

    /// Partial downloads always go; the finished file only when asked.
    fn delete_files(&self, finished_too: bool) {
        let Some(path) = self.path() else { return };
        for p in [sibling(&path, ".part"), sibling(&path, ".resume")] {
            let _ = std::fs::remove_file(p);
        }
        if finished_too && self.status == Status::Done {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// One run of a download's task.
struct Job {
    id: Id,
    src: Source,
    connections: usize,
    progress: Arc<Progress>,
}

impl Job {
    async fn run(&self, net: &Arc<Net>, me: &Weak<Manager>, stop: watch::Receiver<bool>) -> engine::Result<Outcome> {
        let probe = engine::probe(net, &self.src).await?;
        let path = me.upgrade().ok_or("shutting down")?.named(self.id, &probe)?;
        engine::fetch(net, &self.src, probe, &path, self.connections, &self.progress, stop).await
    }
}

impl Manager {
    pub fn open(db: &Path, default_dir: PathBuf, rt: Handle) -> rusqlite::Result<Arc<Self>> {
        let db = Db::open(db)?;
        let settings = Settings {
            dir: db.setting("dir").map(PathBuf::from).unwrap_or(default_dir),
            max_active: db.setting("max_active").and_then(|v| v.parse().ok()).unwrap_or(3),
            connections: db.setting("connections").and_then(|v| v.parse().ok()).unwrap_or(8),
            speed_limit: db.setting("speed_limit").and_then(|v| v.parse().ok()).unwrap_or(0),
            keep_awake: db.setting("keep_awake").is_none_or(|v| v == "true"),
            launch_at_login: crate::system::launch_at_login(),
            notify: db.setting("notify").is_none_or(|v| v == "true"),
            ask_where: db.setting("ask_where").is_some_and(|v| v == "true"),
            sort_into_folders: db.setting("sort_into_folders").is_some_and(|v| v == "true"),
            watch_clipboard: db.setting("watch_clipboard").is_none_or(|v| v == "true"),
            start_at: db.setting("start_at").and_then(|v| parse_clock(&v)),
            stop_at: db.setting("stop_at").and_then(|v| parse_clock(&v)),
            when_done: db.setting("when_done").map_or(WhenDone::Nothing, |v| WhenDone::parse(&v)),
            auto_update: db.setting("auto_update").is_none_or(|v| v == "true"),
        };
        let net = Net::new();
        net.limit.set(settings.speed_limit);
        let items = db
            .all()?
            .into_iter()
            .map(|rec| {
                // Running when the app quit: it carries on from its checkpoint.
                let status = match Status::parse(&rec.status) {
                    Status::Downloading => Status::Queued,
                    s => s,
                };
                Item::new(rec, status)
            })
            .collect();
        let m = Arc::new_cyclic(|me| Manager {
            me: me.clone(),
            rt,
            net,
            awake: crate::system::Awake::new(),
            shows: Default::default(),
            drained: Default::default(),
            inner: Mutex::new(Inner { db, settings, items, throughput: VecDeque::new(), awake_on: false, clock: None }),
        });
        m.schedule(&mut m.lock());
        let me = Arc::downgrade(&m);
        m.rt.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let Some(m) = me.upgrade() else { break };
                m.tick();
            }
        });
        Ok(m)
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every download, newest first.
    pub fn rows(&self) -> Vec<Row> {
        let inner = self.lock();
        let waiting: Vec<Id> = inner.items.iter().filter(|i| i.status == Status::Queued).map(|i| i.rec.id).collect();
        inner
            .items
            .iter()
            .rev()
            .filter(|i| i.doomed.is_none())
            .map(|i| {
                let live = i.stop.is_some();
                let p = &i.progress;
                Row {
                    id: i.rec.id,
                    name: i.rec.name.clone().unwrap_or_else(|| i.rec.url.clone()),
                    url: i.rec.url.clone(),
                    path: i.path(),
                    status: i.status,
                    total: if live { p.total.load(Relaxed).max(i.rec.total) } else { i.rec.total },
                    done: if live { p.done.load(Relaxed) } else { i.rec.done },
                    speed: if live { i.speed } else { 0 },
                    connections: if live { p.connections.load(Relaxed) } else { 0 },
                    resumable: i.rec.resumable,
                    error: i.rec.error.clone(),
                    added: i.rec.added,
                    parts: {
                        let (live_parts, total) = (p.parts.lock().unwrap(), if live { p.total.load(Relaxed).max(i.rec.total) } else { i.rec.total });
                        let parts = if live { &live_parts[..] } else { &i.parts[..] };
                        let f = |x: u64| (x as f64 / total as f64) as f32;
                        let seg = |&(a, b, c, active): &(u64, u64, u64, bool)| Segment {
                            start: f(a),
                            done: f(b),
                            end: f(c),
                            active: active && live,
                            speed: if live { i.part_speed.get(&a).map_or(0, |s| s.1) } else { 0 },
                        };
                        if total == 0 { Vec::new() } else { parts.iter().map(seg).collect() }
                    },
                    history: i.history.iter().copied().collect(),
                    position: waiting.iter().position(|w| *w == i.rec.id).map_or(0, |n| n + 1),
                }
            })
            .collect()
    }

    /// Queue a URL (http/https only). Err is a message for the user.
    pub fn add(&self, url: &str) -> Result<Id, String> {
        self.add_with_headers(url, HeaderMap::new())
    }

    /// Queue a URL with the browser's cookies, referrer and user agent.
    pub fn add_with_headers(&self, url: &str, headers: HeaderMap) -> Result<Id, String> {
        self.add_into(url, headers, None)
    }

    /// Queue a URL to save in `dir` instead of the download folder.
    pub fn add_to(&self, url: &str, dir: PathBuf) -> Result<Id, String> {
        self.add_into(url, HeaderMap::new(), Some(dir))
    }

    fn add_into(&self, url: &str, headers: HeaderMap, dir: Option<PathBuf>) -> Result<Id, String> {
        let parsed = parse_link(url)?;
        let mut inner = self.lock();
        let (text, added) = (header_text(&headers), now());
        let dir = dir.unwrap_or_else(|| inner.settings.dir.clone());
        let id = inner.db.insert(parsed.as_str(), &text, &dir, "queued", added).map_err(|e| e.to_string())?;
        let position = inner.db.position(id).map_err(|e| e.to_string())?;
        let rec = Saved {
            id,
            url: parsed.into(),
            headers: text,
            dir,
            name: None,
            total: 0,
            done: 0,
            status: "queued".into(),
            resumable: None,
            error: None,
            added,
            position,
        };
        inner.items.push(Item::new(rec, Status::Queued));
        self.schedule(&mut inner);
        Ok(id)
    }

    /// Run `f` on one download and save it, then let the queue move.
    fn with(&self, id: Id, f: impl FnOnce(&mut Item)) {
        let mut inner = self.lock();
        let Inner { db, items, .. } = &mut *inner;
        if let Some(it) = items.iter_mut().find(|i| i.rec.id == id && i.doomed.is_none()) {
            f(it);
            it.save(db);
        }
        self.schedule(&mut inner);
    }

    /// Queue a paused or failed download again (no-op for others).
    pub fn start(&self, id: Id) {
        self.with(id, requeue);
    }

    /// Pause a queued or running download (no-op for others).
    pub fn pause(&self, id: Id) {
        self.with(id, pause);
    }

    pub fn start_all(&self) {
        self.all(requeue);
    }

    pub fn pause_all(&self) {
        self.all(pause);
    }

    fn all(&self, f: fn(&mut Item)) {
        let mut inner = self.lock();
        let Inner { db, items, .. } = &mut *inner;
        for it in items.iter_mut().filter(|i| i.doomed.is_none()) {
            f(it);
            it.save(db);
        }
        self.schedule(&mut inner);
    }

    /// Throw away what was downloaded and queue it from the start (for "the file changed on the server").
    pub fn restart(&self, id: Id) {
        self.with(id, |it| {
            if it.stop.is_some() {
                return;
            }
            it.delete_files(false);
            (it.rec.done, it.rec.error, it.status) = (0, None, Status::Queued);
            it.parts.clear();
        });
    }

    /// Forget a download; with `delete_file`, delete the file or its partial download too.
    pub fn remove(&self, id: Id, delete_file: bool) {
        let mut inner = self.lock();
        let Some(pos) = inner.items.iter().position(|i| i.rec.id == id && i.doomed.is_none()) else { return };
        if let Err(e) = inner.db.delete(id) {
            eprintln!("shard: removing download {id}: {e}");
        }
        match &inner.items[pos].stop {
            Some(stop) => {
                let _ = stop.send(true);
                inner.items[pos].doomed = Some(delete_file);
            }
            None => inner.items.remove(pos).delete_files(delete_file),
        }
    }

    /// All downloads' speed together, once a second, oldest first, for the last minute.
    pub fn throughput(&self) -> Vec<u64> {
        self.lock().throughput.iter().copied().collect()
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    /// Give a download a fresh link (its old one expired), keeping what's downloaded when the
    /// file's size is the same, and queue it again.
    pub fn refresh_link(&self, id: Id, url: &str) -> Result<(), String> {
        let parsed = parse_link(url)?;
        let mut found = false;
        self.with(id, |it| {
            found = true;
            if it.stop.is_some() {
                return;
            }
            it.rec.url = parsed.into();
            (it.refreshed, it.rec.error) = (true, None);
            if it.status != Status::Done {
                it.status = Status::Queued;
            }
        });
        found.then_some(()).ok_or_else(|| "That download is gone.".into())
    }

    /// Move a download within the queue; waiting downloads start from the top.
    pub fn move_to(&self, id: Id, to: Move) {
        let mut inner = self.lock();
        let Inner { db, items, .. } = &mut *inner;
        let Some(at) = items.iter().position(|i| i.rec.id == id) else { return };
        let it = items.remove(at);
        let to = match to {
            Move::Top => 0,
            Move::Up => at.saturating_sub(1),
            Move::Down => (at + 1).min(items.len()),
            Move::Bottom => items.len(),
        };
        items.insert(to, it);
        for (n, it) in items.iter_mut().enumerate() {
            let position = n as i64 + 1;
            if it.rec.position != position {
                it.rec.position = position;
                it.save(db);
            }
        }
        self.schedule(&mut inner);
    }

    /// Goes up by one each time the queue finishes: its last download done and nothing waiting.
    pub fn drained(&self) -> u64 {
        self.drained.load(Relaxed)
    }

    /// Ask the window to show itself (another `shard` was launched while this one runs).
    pub fn request_show(&self) {
        self.shows.fetch_add(1, Relaxed);
    }

    /// Goes up by one per `request_show`; the UI shows the window when it changes.
    pub fn show_requests(&self) -> u64 {
        self.shows.load(Relaxed)
    }

    /// Err is a message for the user (for example a folder that can't be created).
    pub fn set_settings(&self, s: Settings) -> Result<(), String> {
        std::fs::create_dir_all(&s.dir).map_err(|e| format!("Can't use {}: {e}", s.dir.display()))?;
        let s = Settings { max_active: s.max_active.clamp(1, 10), connections: s.connections.clamp(1, 32), ..s };
        if s.launch_at_login != self.lock().settings.launch_at_login {
            crate::system::set_launch_at_login(s.launch_at_login).map_err(|e| format!("Can't change launch at login: {e}"))?;
        }
        let mut inner = self.lock();
        for (k, v) in [
            ("dir", s.dir.to_string_lossy().into_owned()),
            ("max_active", s.max_active.to_string()),
            ("connections", s.connections.to_string()),
            ("speed_limit", s.speed_limit.to_string()),
            ("keep_awake", s.keep_awake.to_string()),
            ("notify", s.notify.to_string()),
            ("ask_where", s.ask_where.to_string()),
            ("sort_into_folders", s.sort_into_folders.to_string()),
            ("watch_clipboard", s.watch_clipboard.to_string()),
            ("start_at", format_clock(s.start_at)),
            ("stop_at", format_clock(s.stop_at)),
            ("when_done", s.when_done.as_str().to_string()),
            ("auto_update", s.auto_update.to_string()),
        ] {
            inner.db.set_setting(k, &v).map_err(|e| e.to_string())?;
        }
        self.net.limit.set(s.speed_limit);
        inner.settings = s;
        self.schedule(&mut inner);
        self.stay_awake(&mut inner);
        Ok(())
    }

    /// Start queued downloads, oldest first, while there are free slots.
    fn schedule(&self, inner: &mut Inner) {
        let Inner { db, settings, items, .. } = inner;
        let mut active = items.iter().filter(|i| i.stop.is_some()).count();
        for it in items.iter_mut() {
            if active >= settings.max_active {
                break;
            }
            if it.status == Status::Queued && it.stop.is_none() && it.doomed.is_none() {
                self.launch(it, settings.connections);
                it.save(db);
                active += 1;
            }
        }
    }

    fn launch(&self, it: &mut Item, connections: usize) {
        let (stop, rx) = watch::channel(false);
        it.status = Status::Downloading;
        it.rec.error = None;
        it.stop = Some(stop);
        (it.speed, it.seen) = (0, None);
        it.history.clear();
        it.part_speed.clear();
        it.progress = Arc::default();
        it.progress.done.store(it.rec.done, Relaxed);
        it.progress.total.store(it.rec.total, Relaxed);
        let job = Job {
            id: it.rec.id,
            src: Source { url: it.rec.url.clone(), headers: it.headers.clone(), refreshed: std::mem::take(&mut it.refreshed) },
            connections,
            progress: it.progress.clone(),
        };
        let (me, net) = (self.me.clone(), self.net.clone());
        self.rt.spawn(async move {
            let result = job.run(&net, &me, rx).await;
            if let Some(m) = me.upgrade() {
                m.finished(job.id, result);
            }
        });
    }

    /// The first run of a download names it: what the server suggests, made unique in its folder.
    fn named(&self, id: Id, probe: &Probe) -> engine::Result<PathBuf> {
        let mut inner = self.lock();
        let Inner { db, items, settings, .. } = &mut *inner;
        let taken: Vec<PathBuf> = items.iter().filter(|i| i.rec.id != id).filter_map(Item::path).collect();
        let it = items.iter_mut().find(|i| i.rec.id == id).ok_or("removed")?;
        if it.rec.name.is_none() && settings.sort_into_folders {
            it.rec.dir = it.rec.dir.join(category(&probe.name));
        }
        std::fs::create_dir_all(&it.rec.dir)?;
        if it.rec.name.is_none() {
            it.rec.name = Some(unique(&it.rec.dir, &probe.name, |p| taken.iter().any(|t| t == p)));
        }
        it.rec.resumable = Some(probe.resumable());
        if let Some(len) = probe.len {
            it.rec.total = len;
            it.progress.total.store(len, Relaxed);
        }
        it.save(db);
        Ok(it.path().ok_or("unnamed")?)
    }

    fn finished(&self, id: Id, result: engine::Result<Outcome>) {
        let mut inner = self.lock();
        let Inner { db, items, .. } = &mut *inner;
        let Some(pos) = items.iter().position(|i| i.rec.id == id) else { return };
        let it = &mut items[pos];
        it.save(db); // last progress, while `stop` still says it was live
        (it.stop, it.speed) = (None, 0);
        it.parts = std::mem::take(&mut *it.progress.parts.lock().unwrap());
        let finished_ok = result.as_ref().is_ok_and(|o| *o == Outcome::Done);
        if finished_ok {
            it.status = Status::Done;
            it.rec.total = it.rec.total.max(it.rec.done); // known now, even if the server never said
            it.rec.done = it.rec.total;
        }
        if let Some(delete_file) = it.doomed {
            items.remove(pos).delete_files(delete_file);
        } else {
            match result {
                Ok(Outcome::Done) => (it.headers, it.rec.headers) = (HeaderMap::new(), String::new()), // cookies aren't needed any more
                Ok(Outcome::Stopped) if it.status == Status::Downloading => it.status = Status::Paused,
                Err(e) if it.status == Status::Downloading => (it.status, it.rec.error) = (Status::Failed, Some(e.to_string())),
                _ => {}
            }
            it.save(db);
        }
        self.schedule(&mut inner);
        // The queue finished: this one done, nothing running or waiting.
        let idle = inner.items.iter().all(|i| i.stop.is_none() && i.status != Status::Queued);
        if idle && finished_ok {
            self.drained.fetch_add(1, Relaxed);
        }
    }

    /// Once a second: sample speeds and save progress.
    fn tick(&self) {
        let mut inner = self.lock();
        let Inner { db, items, throughput, .. } = &mut *inner;
        let mut total = 0;
        for it in items.iter_mut().filter(|i| i.stop.is_some() && i.doomed.is_none()) {
            let done = it.progress.done.load(Relaxed);
            if let Some(seen) = it.seen {
                it.speed = (it.speed + done.saturating_sub(seen)) / 2;
                push(&mut it.history, it.speed);
            }
            let parts = it.progress.parts.lock().unwrap().clone();
            let speeds = parts
                .iter()
                .map(|&(start, done, ..)| {
                    let speed = it.part_speed.get(&start).map_or(0, |&(seen, speed)| (speed + done.saturating_sub(seen)) / 2);
                    (start, (done, speed))
                })
                .collect();
            it.part_speed = speeds;
            it.seen = Some(done);
            total += it.speed;
            it.save(db);
        }
        push(throughput, total);
        self.stay_awake(&mut inner);
        self.keep_schedule(&mut inner, chrono::Local::now());
    }

    /// Resume paused downloads at `start_at`, pause the queue at `stop_at`, once per minute.
    fn keep_schedule(&self, inner: &mut Inner, now: chrono::DateTime<chrono::Local>) {
        use chrono::Timelike;
        let clock = Some((now.hour() as u8, now.minute() as u8));
        if inner.clock == clock {
            return;
        }
        inner.clock = clock;
        let change: Option<fn(&mut Item)> = if clock == inner.settings.start_at {
            Some(|it| {
                if it.status == Status::Paused {
                    it.status = Status::Queued;
                }
            })
        } else if clock == inner.settings.stop_at {
            Some(pause)
        } else {
            None
        };
        if let Some(change) = change {
            let Inner { db, items, .. } = &mut *inner;
            for it in items.iter_mut().filter(|i| i.doomed.is_none()) {
                change(it);
                it.save(db);
            }
            self.schedule(inner);
        }
    }
}

impl Manager {
    /// Hold the computer awake while anything downloads, if the settings say so.
    fn stay_awake(&self, inner: &mut Inner) {
        let want = inner.settings.keep_awake && inner.items.iter().any(|i| i.stop.is_some());
        if want != inner.awake_on {
            self.awake.set(want);
            inner.awake_on = want;
        }
    }
}

fn requeue(it: &mut Item) {
    if matches!(it.status, Status::Paused | Status::Failed) {
        (it.status, it.rec.error) = (Status::Queued, None);
    }
}

fn pause(it: &mut Item) {
    if matches!(it.status, Status::Queued | Status::Downloading) {
        it.status = Status::Paused;
        if let Some(stop) = &it.stop {
            let _ = stop.send(true);
        }
    }
}

/// `name`, or `name (1).ext`, `name (2).ext`... whichever isn't on disk, part-downloaded, or another download's.
fn unique(dir: &Path, name: &str, taken: impl Fn(&Path) -> bool) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    };
    (0u32..)
        .map(|n| if n == 0 { name.to_string() } else { format!("{stem} ({n}){ext}") })
        .find(|n| {
            let p = dir.join(n);
            !p.exists() && !sibling(&p, ".part").exists() && !taken(&p)
        })
        .expect("some name is free")
}

/// A link Shard can download: http or https. Err is a message for the user.
fn parse_link(url: &str) -> Result<url::Url, String> {
    let parsed = url::Url::parse(url.trim()).map_err(|_| "That isn't a valid link.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only http and https links can be downloaded.".into());
    }
    Ok(parsed)
}

fn header_text(h: &HeaderMap) -> String {
    h.iter().filter_map(|(k, v)| Some(format!("{k}: {}\n", v.to_str().ok()?))).collect()
}

fn parse_headers(s: &str) -> HeaderMap {
    s.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(": ")?;
            Some((HeaderName::from_bytes(k.as_bytes()).ok()?, HeaderValue::from_str(v).ok()?))
        })
        .collect()
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::engine::tests::{body, scratch, serve};
    use chrono::Timelike;

    pub(crate) async fn wait_until(m: &Manager, done: impl Fn(&[Row]) -> bool) -> Vec<Row> {
        for _ in 0..400 {
            let rows = m.rows();
            if done(&rows) {
                return rows;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("timed out: {:?}", m.rows());
    }

    #[test]
    fn categories_and_clock_times() {
        assert_eq!(category("Big Buck Bunny.MP4"), "Video");
        assert_eq!(category("ubuntu.iso"), "Archives");
        assert_eq!(category("README"), "Other");
        assert_eq!(parse_clock("07:05"), Some((7, 5)));
        assert_eq!((parse_clock("24:00"), parse_clock(""), parse_clock("7")), (None, None, None));
        assert_eq!(format_clock(Some((7, 5))), "07:05");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn orders_sorts_into_folders_and_keeps_the_schedule() {
        let body = body();
        let (url, _log) = serve(body.clone(), 100);
        let dir = scratch("queue-order");
        let dl = dir.join("dl");
        let m = Manager::open(&dir.join("shard.db"), dl.clone(), Handle::current()).unwrap();
        m.set_settings(Settings { dir: dl.clone(), max_active: 1, sort_into_folders: true, ..m.settings() }).unwrap();
        m.pause_all();
        let (a, b) = (m.add(&format!("{url}?a")).unwrap(), m.add_to(&format!("{url}?b"), dir.join("elsewhere")).unwrap());
        let c = m.add(&format!("{url}?c")).unwrap();
        // `a` started at once (a free slot); `b` and `c` wait, in that order. Move `c` up front.
        m.move_to(c, Move::Top);
        let pos = |id| m.rows().iter().find(|r| r.id == id).unwrap().position;
        assert_eq!((pos(c), pos(b)), (1, 2), "moved to the top of the waiting line");

        // At its stop time the scheduler pauses everything; at its start time it resumes them.
        let at = |h, mi| chrono::Local::now().with_hour(h).unwrap().with_minute(mi).unwrap();
        m.set_settings(Settings { start_at: Some((6, 0)), stop_at: Some((23, 0)), ..m.settings() }).unwrap();
        m.keep_schedule(&mut m.lock(), at(23, 0));
        assert!(m.rows().iter().all(|r| matches!(r.status, Status::Paused | Status::Done)), "{:?}", m.rows());
        m.keep_schedule(&mut m.lock(), at(6, 0));
        let drained = m.drained();
        let rows = wait_until(&m, |r| r.iter().all(|r| r.status == Status::Done)).await;
        assert!(m.drained() > drained, "the queue finishing is counted");

        // Videos go to Video/, and `b` went where it was told (sorted into its own Other/).
        let path = |id| rows.iter().find(|r| r.id == id).unwrap().path.clone().unwrap();
        assert!(path(a).starts_with(dl.join("Other")), "{:?}", path(a));
        assert!(path(b).starts_with(dir.join("elsewhere")), "{:?}", path(b));
        assert_eq!(std::fs::read(path(c)).unwrap(), *body);
        let _ = std::fs::remove_dir_all(dir); // Windows keeps the open database locked
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn queues_names_and_remembers() {
        let body = body();
        let (url, _log) = serve(body.clone(), 100);
        let dir = scratch("queue");
        let dl = dir.join("dl");
        let m = Manager::open(&dir.join("shard.db"), dl.clone(), Handle::current()).unwrap();
        m.set_settings(Settings { dir: dl.clone(), max_active: 1, connections: 4, speed_limit: 0, ..m.settings() }).unwrap();
        assert!(m.add("ftp://example.com/x").is_err());
        let a = m.add(&url).unwrap();
        m.add(&url).unwrap();

        let rows = wait_until(&m, |r| r.iter().all(|r| r.status == Status::Done)).await;
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["some file (1).bin", "some file.bin"], "newest first, the second one renamed");
        assert_eq!(std::fs::read(dl.join("some file.bin")).unwrap(), *body);
        assert!(rows.iter().all(|r| r.done == body.len() as u64 && r.resumable == Some(true)));
        let map = &rows[0].parts;
        assert!(map.len() > 1 && map.iter().all(|s| s.done == s.end && !s.active), "a finished map is full: {map:?}");
        assert_eq!((map[0].start, map.last().unwrap().end), (0.0, 1.0));

        m.remove(a, true);
        assert!(!dl.join("some file.bin").exists());
        drop(m);
        let m = Manager::open(&dir.join("shard.db"), PathBuf::new(), Handle::current()).unwrap();
        let rows = m.rows();
        assert_eq!((rows.len(), rows[0].status, m.settings().max_active), (1, Status::Done, 1));
        // Best effort: Windows won't delete the database while the manager still has it open.
        let _ = std::fs::remove_dir_all(dir);
    }
}
