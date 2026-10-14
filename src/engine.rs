//! The download engine: one file over several connections, resumable after a crash.
//!
//! `probe` asks for one byte to learn the size, a validator and a file name. `fetch` then runs a
//! pool of workers: each takes a part nobody is on, or splits the busy part with the most left, so
//! a slow connection never leaves the others idle at the end. A server that answers 429/503 to one
//! connection too many retires that worker and the rest carry on. One writer owns `out.part` and
//! saves `out.resume` only after syncing the data it describes, so a checkpoint never claims bytes
//! that aren't on disk.

use bytes::Bytes;
use percent_encoding::percent_decode_str;
use reqwest::header::{
    CONTENT_DISPOSITION, CONTENT_RANGE, ETAG, HeaderMap, IF_RANGE, LAST_MODIFIED, RANGE, RETRY_AFTER,
};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use std::ffi::OsString;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

/// Parts are never split below this.
const MIN_PART: u64 = 256 * 1024;
/// Failed attempts in a row, without a byte of progress, before giving up.
const RETRIES: u32 = 5;
/// After a server refuses a connection (429/503), how long to wait before trying one more; it
/// doubles each time the extra one is refused again, up to `REGROW_MAX`.
#[cfg(not(test))]
const REGROW_AFTER: Duration = Duration::from_secs(30);
#[cfg(test)]
const REGROW_AFTER: Duration = Duration::from_millis(100);
const REGROW_MAX: Duration = Duration::from_secs(240);
#[cfg(not(test))]
const REGROW_TICK: Duration = Duration::from_secs(5);
#[cfg(test)]
const REGROW_TICK: Duration = Duration::from_millis(25);

#[derive(Clone, Default)]
pub struct Source {
    pub url: String,
    /// Sent with every request: cookies, referrer and user agent from the browser.
    pub headers: HeaderMap,
    /// A fresh link for a file already part-downloaded. Another server may give the same file a
    /// different ETag, so resume when the size matches even if the validator doesn't.
    pub refreshed: bool,
}

/// Live numbers for the UI, written by `fetch`.
#[derive(Default)]
pub struct Progress {
    pub total: AtomicU64,
    pub done: AtomicU64,
    pub connections: AtomicUsize,
    /// (start, done, end, being fetched) of every part, refreshed a few times a second.
    pub parts: Mutex<Vec<(u64, u64, u64, bool)>>,
}

impl Progress {
    fn publish(&self, plan: &Plan) {
        *self.parts.lock().unwrap() = plan.layout();
    }
}

/// The parts of a paused download, from its checkpoint next to `out`.
pub fn saved_parts(out: &Path) -> Vec<(u64, u64, u64, bool)> {
    std::fs::read_to_string(sibling(out, ".resume")).ok().and_then(|s| Plan::parse(&s)).map_or_else(Vec::new, |p| p.layout())
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Done,
    Stopped,
}

pub struct Probe {
    pub len: Option<u64>,
    pub name: String,
    validator: String,
    /// The server ignored the range and is sending the whole file: one stream, no resume.
    whole: Option<Response>,
}

impl Probe {
    pub fn resumable(&self) -> bool {
        self.whole.is_none()
    }
}

/// What a failed request means for the next one.
enum Fail {
    /// 429/503: too many connections, or the server wants a break.
    Busy(Option<Duration>),
    Retry(Error),
    Fatal(Error),
}

impl Fail {
    fn wait(&self, failures: u32) -> Duration {
        match self {
            Fail::Busy(Some(d)) => (*d).min(Duration::from_secs(60)),
            _ => Duration::from_millis(500 << failures.min(6)),
        }
    }

    fn into_error(self) -> Error {
        match self {
            Fail::Busy(_) => "the server is refusing connections (HTTP 429/503)".into(),
            Fail::Retry(e) | Fail::Fatal(e) => e,
        }
    }
}

/// What every download shares: the HTTP client and the speed limit.
pub struct Net {
    pub client: Client,
    pub limit: Limiter,
}

impl Net {
    /// HTTP/1.1 only: over HTTP/2 the connections would all be streams on one TCP connection,
    /// which is the throttle parallel parts exist to get around. The read timeout turns a stalled
    /// connection into a retry.
    pub fn new() -> Arc<Net> {
        let client = Client::builder()
            .http1_only()
            .user_agent(concat!("shard/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .build()
            .expect("the TLS backend loads");
        Arc::new(Net { client, limit: Limiter::default() })
    }
}

/// A byte rate shared by every connection that takes from it; 0 means unlimited.
#[derive(Default)]
pub struct Limiter {
    rate: AtomicU64,
    /// Bytes that may go now (negative: owed), as of when.
    budget: Mutex<Option<(f64, tokio::time::Instant)>>,
}

impl Limiter {
    pub fn set(&self, bytes_per_sec: u64) {
        self.rate.store(bytes_per_sec, Relaxed);
    }

    /// Wait until `n` more bytes fit under the limit. Bursts are capped at one second's worth.
    async fn take(&self, n: u64) {
        let rate = self.rate.load(Relaxed) as f64;
        if rate == 0.0 {
            return;
        }
        let owed = {
            let mut budget = self.budget.lock().unwrap();
            let now = tokio::time::Instant::now();
            let (left, at) = budget.unwrap_or((rate, now));
            let left = (left + (now - at).as_secs_f64() * rate).min(rate) - n as f64;
            *budget = Some((left, now));
            -left
        };
        if owed > 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(owed / rate)).await;
        }
    }
}

/// A network error as people can read it: without the URL (signed links run to hundreds of
/// characters), and with the reason underneath reqwest's summary ("connection refused").
fn network(e: reqwest::Error) -> Error {
    let e = e.without_url();
    let mut text = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text = format!("{text}: {cause_text}");
        }
        source = cause.source();
    }
    text.into()
}

fn get(client: &Client, src: &Source) -> RequestBuilder {
    client.get(&src.url).headers(src.headers.clone())
}

async fn send(req: RequestBuilder) -> std::result::Result<Response, Fail> {
    let resp = req.send().await.map_err(|e| Fail::Retry(network(e)))?;
    let status = resp.status();
    if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE {
        // ponytail: Retry-After in seconds only; the HTTP-date form falls back to backoff.
        let secs = resp.headers().get(RETRY_AFTER).and_then(|v| v.to_str().ok()?.trim().parse().ok());
        return Err(Fail::Busy(secs.map(Duration::from_secs)));
    }
    // 416 passes through: the probe's answer for an empty file.
    if status.is_success() || status == StatusCode::RANGE_NOT_SATISFIABLE {
        return Ok(resp);
    }
    let e = format!("the server answered {status}").into();
    Err(if status.is_server_error() || status == StatusCode::REQUEST_TIMEOUT { Fail::Retry(e) } else { Fail::Fatal(e) })
}

/// Ask for the first byte: servers that honour ranges report the size, the rest send the whole file.
pub async fn probe(net: &Net, src: &Source) -> Result<Probe> {
    let (mut failures, mut range) = (0, true);
    let resp = loop {
        let mut req = get(&net.client, src);
        if range {
            req = req.header(RANGE, "bytes=0-0");
        }
        match send(req).await {
            Ok(r) if range && r.status() == StatusCode::RANGE_NOT_SATISFIABLE => range = false,
            Ok(r) => break r,
            Err(Fail::Fatal(e)) => return Err(e),
            Err(f) => {
                failures += 1;
                if failures > RETRIES {
                    return Err(f.into_error());
                }
                tokio::time::sleep(f.wait(failures)).await;
            }
        }
    };
    let name = file_name(&resp);
    if resp.status() != StatusCode::PARTIAL_CONTENT {
        return Ok(Probe { len: resp.content_length(), name, validator: String::new(), whole: Some(resp) });
    }
    let header = |n| resp.headers().get(n).and_then(|v| v.to_str().ok());
    let len = header(CONTENT_RANGE)
        .and_then(|v| v.rsplit('/').next()?.parse().ok())
        .ok_or("the server did not report the file's size")?;
    let validator = header(ETAG)
        .filter(|e| !e.starts_with("W/")) // If-Range takes only strong ETags
        .or_else(|| header(LAST_MODIFIED))
        .unwrap_or("")
        .to_string();
    Ok(Probe { len: Some(len), name, validator, whole: None })
}

/// Download what `probe` found to `out` over up to `connections` connections. Returns `Stopped` once
/// `stop` turns true, checkpoint saved, so the next `fetch` carries on from there.
pub async fn fetch(
    net: &Arc<Net>,
    src: &Source,
    probe: Probe,
    out: &Path,
    connections: usize,
    progress: &Progress,
    mut stop: watch::Receiver<bool>,
) -> Result<Outcome> {
    let (part, resume) = (sibling(out, ".part"), sibling(out, ".resume"));
    progress.total.store(probe.len.unwrap_or(0), Relaxed);
    if let Some(resp) = probe.whole {
        let _ = tokio::fs::remove_file(&resume).await; // a stale checkpoint would describe bytes this overwrites
        return whole(resp, &part, out, progress, stop).await;
    }
    let len = probe.len.unwrap_or(0);

    let saved = tokio::fs::read_to_string(&resume)
        .await
        .ok()
        .and_then(|s| Plan::parse(&s))
        .filter(|c| c.len == len && (c.validator == probe.validator || src.refreshed) && part.exists());
    let (mut file, plan) = match saved {
        Some(mut c) => {
            c.validator = probe.validator.clone(); // the new server's, for If-Range
            (OpenOptions::new().write(true).open(&part).await?, c)
        }
        None => {
            let f = File::create(&part).await?;
            f.set_len(len).await?;
            let whole = Part { start: 0, end: len, done: 0, sent: 0, busy: false };
            (f, Plan { len, validator: probe.validator, parts: vec![whole] })
        }
    };
    progress.done.store(plan.parts.iter().map(|p| p.done - p.start).sum(), Relaxed);
    progress.publish(&plan);

    let n = connections.max(1);
    let ctx = Arc::new(Ctx {
        net: net.clone(),
        src: src.clone(),
        plan: Mutex::new(plan),
        active: AtomicUsize::new(n),
        refused_at: Mutex::new(None),
    });
    let (tx, mut rx) = mpsc::channel(64);
    let mut workers = JoinSet::new(); // dropped on return, which aborts any still running
    for _ in 0..n {
        workers.spawn(worker(ctx.clone(), tx.clone()));
    }
    // Weak, so the channel still closes when every worker is done; regrowing upgrades it.
    let spare = tx.downgrade();
    drop(tx);

    let (mut saved_at, mut published_at) = (Instant::now(), Instant::now());
    let (mut regrow, mut regrow_after, mut regrown_at) = (tokio::time::interval(REGROW_TICK), REGROW_AFTER, None::<Instant>);
    let outcome = loop {
        tokio::select! {
            msg = rx.recv() => {
                let Some((i, off, chunk)) = msg else { break Outcome::Done };
                file.seek(SeekFrom::Start(off)).await?;
                file.write_all(&chunk).await?;
                ctx.plan.lock().unwrap().parts[i].done = off + chunk.len() as u64;
                progress.done.fetch_add(chunk.len() as u64, Relaxed);
                progress.connections.store(ctx.active.load(Relaxed), Relaxed);
                if published_at.elapsed() >= Duration::from_millis(200) {
                    progress.publish(&ctx.plan.lock().unwrap());
                    published_at = Instant::now();
                }
                if saved_at.elapsed() >= Duration::from_secs(1) {
                    file.sync_data().await?;
                    ctx.save(&resume).await?;
                    saved_at = Instant::now();
                }
            }
            Some(r) = workers.join_next() => r??,
            () = stopped(&mut stop) => break Outcome::Stopped,
            _ = regrow.tick() => {
                // A server that refused one connection too many may take it now: try one more.
                let Some(refused) = *ctx.refused_at.lock().unwrap() else { continue };
                if regrown_at.is_some_and(|t| refused > t) {
                    // The last extra one was refused too: wait longer before the next.
                    regrow_after = (regrow_after * 2).min(REGROW_MAX);
                    regrown_at = None;
                }
                let quiet = refused.elapsed() >= regrow_after && regrown_at.is_none_or(|t| t.elapsed() >= regrow_after);
                if quiet && ctx.active.load(Relaxed) < n && ctx.plan.lock().unwrap().splittable()
                    && let Some(tx) = spare.upgrade() {
                        ctx.active.fetch_add(1, Relaxed);
                        workers.spawn(worker(ctx.clone(), tx));
                        regrown_at = Some(Instant::now());
                    }
            }
        }
    };
    progress.connections.store(0, Relaxed);
    progress.publish(&ctx.plan.lock().unwrap());
    file.sync_data().await?;
    ctx.save(&resume).await?;
    if outcome == Outcome::Stopped {
        return Ok(outcome);
    }
    while let Some(r) = workers.join_next().await {
        r??;
    }
    if ctx.plan.lock().unwrap().parts.iter().any(|p| p.done != p.end) {
        return Err("the download ended with parts missing".into());
    }
    drop(file);
    tokio::fs::rename(&part, out).await?;
    let _ = tokio::fs::remove_file(&resume).await;
    Ok(Outcome::Done)
}

async fn stopped(stop: &mut watch::Receiver<bool>) {
    if stop.wait_for(|s| *s).await.is_err() {
        std::future::pending::<()>().await // nobody can stop us any more
    }
}

/// The server ignored the range: one stream from the start, nothing to resume from.
async fn whole(mut resp: Response, part: &Path, out: &Path, progress: &Progress, mut stop: watch::Receiver<bool>) -> Result<Outcome> {
    let mut file = File::create(part).await?;
    progress.done.store(0, Relaxed);
    progress.connections.store(1, Relaxed);
    // A body cut short errors here (hyper checks Content-Length and chunk framing), so a short file is never renamed.
    let result = loop {
        let chunk = tokio::select! {
            c = resp.chunk() => c,
            () = stopped(&mut stop) => break Ok(Outcome::Stopped),
        };
        match chunk {
            Ok(Some(chunk)) => {
                file.write_all(&chunk).await?;
                let done = progress.done.fetch_add(chunk.len() as u64, Relaxed) + chunk.len() as u64;
                *progress.parts.lock().unwrap() = vec![(0, done, progress.total.load(Relaxed).max(done), true)];
            }
            Ok(None) => break Ok(Outcome::Done),
            Err(e) => break Err(network(e)),
        }
    };
    progress.connections.store(0, Relaxed);
    file.flush().await?;
    drop(file);
    if let Ok(Outcome::Done) = result {
        tokio::fs::rename(part, out).await?;
    }
    result
}

/// Every part of the file and how far it got; `out.resume` is this as text.
#[derive(Debug, PartialEq)]
struct Plan {
    len: u64,
    /// Strong ETag or Last-Modified: resuming against a changed file would splice two versions.
    validator: String,
    parts: Vec<Part>,
}

/// Bytes `[start, end)`. Everything below `done` is on disk; everything below `sent` is on its way to the writer.
#[derive(Debug, PartialEq)]
struct Part {
    start: u64,
    end: u64,
    done: u64,
    sent: u64,
    busy: bool,
}

impl Plan {
    fn parse(s: &str) -> Option<Self> {
        let mut lines = s.lines();
        let len = lines.next()?.parse().ok()?;
        let validator = lines.next()?.to_string();
        let parts = lines
            .map(|l| {
                let mut n = l.split(' ').map(|x| x.parse().ok());
                let (start, end, done) = (n.next()??, n.next()??, n.next()??);
                (start <= done && done <= end && end <= len).then_some(Part { start, end, done, sent: done, busy: false })
            })
            .collect::<Option<_>>()?;
        Some(Plan { len, validator, parts })
    }

    fn layout(&self) -> Vec<(u64, u64, u64, bool)> {
        let mut parts: Vec<_> = self.parts.iter().map(|p| (p.start, p.done, p.end, p.busy && p.sent < p.end)).collect();
        parts.sort_unstable();
        parts
    }

    fn to_text(&self) -> String {
        let mut s = format!("{}\n{}\n", self.len, self.validator);
        for p in &self.parts {
            s += &format!("{} {} {}\n", p.start, p.end, p.done);
        }
        s
    }

    /// Whether `take` would find work for another worker.
    fn splittable(&self) -> bool {
        self.parts.iter().any(|p| (!p.busy && p.sent < p.end) || (p.busy && p.end - p.sent >= 2 * MIN_PART))
    }

    /// Work for an idle worker: a part nobody is on, else the back half of the busy part with the most left.
    fn take(&mut self) -> Option<usize> {
        if let Some(i) = self.parts.iter().position(|p| !p.busy && p.sent < p.end) {
            self.parts[i].busy = true;
            return Some(i);
        }
        let p = self.parts.iter_mut().filter(|p| p.busy).max_by_key(|p| p.end - p.sent)?;
        let left = p.end - p.sent;
        if left < 2 * MIN_PART {
            return None;
        }
        let mid = p.end - left / 2;
        let end = std::mem::replace(&mut p.end, mid);
        self.parts.push(Part { start: mid, end, done: mid, sent: mid, busy: true });
        Some(self.parts.len() - 1)
    }
}

struct Ctx {
    net: Arc<Net>,
    src: Source,
    plan: Mutex<Plan>,
    /// Workers still running; one too many for the server retires itself.
    active: AtomicUsize,
    /// When the server last refused a connection (429/503).
    refused_at: Mutex<Option<Instant>>,
}

type Chunks = mpsc::Sender<(usize, u64, Bytes)>;

impl Ctx {
    async fn save(&self, path: &Path) -> Result<()> {
        // ponytail: no fsync on the checkpoint itself; a torn one fails to parse and the download restarts.
        let text = self.plan.lock().unwrap().to_text();
        let tmp = sibling(path, ".tmp");
        tokio::fs::write(&tmp, text).await?;
        tokio::fs::rename(&tmp, path).await?;
        Ok(())
    }

    fn position(&self, i: usize) -> (u64, u64) {
        let plan = self.plan.lock().unwrap();
        (plan.parts[i].sent, plan.parts[i].end)
    }

    fn release(&self, i: usize) {
        self.plan.lock().unwrap().parts[i].busy = false;
    }

    /// One ranged request for part `i`, streamed to the writer until the part ends or the server stops.
    async fn stream(&self, i: usize, tx: &Chunks) -> std::result::Result<(), Fail> {
        let ((from, end), validator) = (self.position(i), self.plan.lock().unwrap().validator.clone());
        let mut req = get(&self.net.client, &self.src).header(RANGE, format!("bytes={from}-{}", end - 1));
        if !validator.is_empty() {
            req = req.header(IF_RANGE, validator);
        }
        let mut resp = send(req).await?;
        let start: Option<u64> = resp
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok()?.strip_prefix("bytes ")?.split('-').next()?.parse().ok());
        if resp.status() != StatusCode::PARTIAL_CONTENT || start != Some(from) {
            return Err(Fail::Fatal("the file changed on the server; download it again from the start".into()));
        }
        while let Some(chunk) = resp.chunk().await.map_err(|e| Fail::Retry(network(e)))? {
            // The part may have shrunk since the request went out: another worker took its back half.
            let (off, take, finished) = {
                let mut plan = self.plan.lock().unwrap();
                let p = &mut plan.parts[i];
                let (off, take) = (p.sent, (chunk.len() as u64).min(p.end - p.sent));
                p.sent += take;
                (off, take, p.sent == p.end)
            };
            if take > 0 {
                self.net.limit.take(take).await;
                tx.send((i, off, chunk.slice(..take as usize))).await.map_err(|_| Fail::Fatal("stopped".into()))?;
            }
            if finished {
                break;
            }
        }
        Ok(())
    }
}

async fn worker(ctx: Arc<Ctx>, tx: Chunks) -> Result<()> {
    let (mut failures, mut current) = (0, None);
    loop {
        let i = match current {
            Some(i) => i,
            None => match ctx.plan.lock().unwrap().take() {
                Some(i) => i,
                None => {
                    ctx.active.fetch_sub(1, Relaxed);
                    return Ok(());
                }
            },
        };
        current = Some(i);
        let before = ctx.position(i).0;
        let fail = ctx.stream(i, &tx).await.err();
        let (sent, end) = ctx.position(i);
        if sent == end {
            ctx.release(i);
            (current, failures) = (None, 0);
            continue;
        }
        if matches!(fail, Some(Fail::Busy(_))) {
            *ctx.refused_at.lock().unwrap() = Some(Instant::now());
        }
        // One connection too many for this server: hand the part back and retire, unless we're the last.
        if matches!(fail, Some(Fail::Busy(_))) && ctx.active.try_update(Relaxed, Relaxed, |n| (n > 1).then(|| n - 1)).is_ok() {
            ctx.release(i);
            return Ok(());
        }
        if let Some(Fail::Fatal(e)) = fail {
            return Err(e);
        }
        if sent > before {
            failures = 0;
        }
        failures += 1;
        let wait = fail.as_ref().map_or(Duration::from_millis(500 << failures), |f| f.wait(failures));
        if failures > RETRIES {
            return Err(fail.map_or_else(|| "the server kept closing the connection early".into(), Fail::into_error));
        }
        tokio::time::sleep(wait).await;
    }
}

pub fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = OsString::from(path);
    s.push(suffix);
    s.into()
}

/// The name the server suggests, else the last segment of the final URL, made safe to save under.
fn file_name(resp: &Response) -> String {
    let header = resp.headers().get(CONTENT_DISPOSITION).and_then(|v| disposition_name(&String::from_utf8_lossy(v.as_bytes())));
    let url = || resp.url().path_segments()?.next_back().map(|s| percent_decode_str(s).decode_utf8_lossy().into_owned());
    [header, url()]
        .into_iter()
        .flatten()
        .map(|n| sanitize(&n))
        .find(|n| !n.is_empty())
        .unwrap_or_else(|| "download".into())
}

fn disposition_name(v: &str) -> Option<String> {
    // ponytail: splits on every ';', so a quoted name containing one comes out cut short.
    let mut plain = None;
    for (key, val) in v.split(';').filter_map(|p| p.split_once('=')) {
        match key.trim().to_ascii_lowercase().as_str() {
            // RFC 5987: charset'language'percent-encoded
            "filename*" => {
                if let Some(enc) = val.trim().splitn(3, '\'').nth(2) {
                    return Some(percent_decode_str(enc).decode_utf8_lossy().into_owned());
                }
            }
            "filename" => plain = Some(val.trim().trim_matches('"').to_string()),
            _ => {}
        }
    }
    plain
}

/// The last path component, without control characters or ones Windows refuses, and no leading dots.
pub fn sanitize(name: &str) -> String {
    // ponytail: Windows device names (CON, NUL...) aren't caught; add when Windows ships.
    let name = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = name.chars().map(|c| if c.is_control() || r#"<>:"|?*"#.contains(c) { '_' } else { c }).collect();
    let clean = clean.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let mut end = clean.len().min(200);
    while !clean.is_char_boundary(end) {
        end -= 1;
    }
    clean[..end].to_string()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::Ordering::SeqCst;

    #[derive(Default)]
    pub(crate) struct Log {
        ranges: Mutex<Vec<(u64, u64)>>,
        refused: AtomicUsize,
        open: AtomicUsize,
        /// Connections served at once, at most; above it the server answers 429.
        max_conns: AtomicUsize,
        /// Milliseconds between 64 KB chunks.
        delay: AtomicU64,
        /// The most connections being served at once.
        peak: AtomicUsize,
        /// The ETag it sends; "v1" unless a test changes it.
        etag: Mutex<String>,
    }

    /// A local server that honours Range, refuses (429) past `max_conns` open connections, and
    /// sends slowly enough for connections to overlap.
    pub(crate) fn serve(body: Arc<Vec<u8>>, max_conns: usize) -> (String, Arc<Log>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/some%20file.bin", listener.local_addr().unwrap());
        let log = Arc::new(Log::default());
        log.max_conns.store(max_conns, SeqCst);
        log.delay.store(2, SeqCst);
        *log.etag.lock().unwrap() = "\"v1\"".into();
        let shared = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let (stream, body, log) = (stream.unwrap(), body.clone(), shared.clone());
                std::thread::spawn(move || {
                    let mut range = (0, body.len() as u64 - 1);
                    let mut lines = BufReader::new(&stream).lines();
                    while let Some(line) = lines.next().map(|l| l.unwrap().to_ascii_lowercase()).filter(|l| !l.is_empty()) {
                        if let Some((a, b)) = line.strip_prefix("range: bytes=").and_then(|r| r.split_once('-')) {
                            range = (a.parse().unwrap(), b.parse().unwrap());
                        }
                    }
                    let open = log.open.fetch_add(1, SeqCst);
                    if open >= log.max_conns.load(SeqCst) {
                        log.refused.fetch_add(1, SeqCst);
                        let _ = (&stream).write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    } else {
                        log.peak.fetch_max(open + 1, SeqCst);
                        log.ranges.lock().unwrap().push(range);
                        let slice = &body[range.0 as usize..=range.1 as usize];
                        let head = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nETag: {}\r\nConnection: close\r\n\r\n",
                            slice.len(),
                            range.0,
                            range.1,
                            body.len(),
                            log.etag.lock().unwrap()
                        );
                        let _ = (&stream).write_all(head.as_bytes());
                        for c in slice.chunks(64 * 1024) {
                            if (&stream).write_all(c).is_err() {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(log.delay.load(SeqCst)));
                        }
                    }
                    log.open.fetch_sub(1, SeqCst);
                });
            }
        });
        (url, log)
    }

    pub(crate) fn body() -> Arc<Vec<u8>> {
        Arc::new((0..3_000_000usize).map(|i| (i * 7 % 251) as u8).collect())
    }

    pub(crate) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shard-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn pauses_and_resumes_without_refetching() {
        let body = body();
        let (url, log) = serve(body.clone(), 100);
        let dir = scratch("resume");
        let out = dir.join("file.bin");
        let (net, src) = (Net::new(), Source { url, ..Default::default() });

        let probe1 = probe(&net, &src).await.unwrap();
        assert_eq!(probe1.name, "some file.bin");
        assert!(probe1.resumable());
        let progress = Progress::default();
        let (stop, rx) = watch::channel(false);
        let pause = async {
            while progress.done.load(SeqCst) < body.len() as u64 / 4 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            stop.send(true).unwrap();
        };
        let (outcome, ()) = tokio::join!(fetch(&net, &src, probe1, &out, 2, &progress, rx), pause);
        assert_eq!(outcome.unwrap(), Outcome::Stopped);
        let plan = Plan::parse(&std::fs::read_to_string(sibling(&out, ".resume")).unwrap()).unwrap();
        assert_eq!(Plan::parse(&plan.to_text()).as_ref(), Some(&plan));
        let on_disk: Vec<_> = plan.parts.iter().map(|p| (p.start, p.done)).filter(|(s, d)| d > s).collect();
        assert!(!on_disk.is_empty());

        log.ranges.lock().unwrap().clear();
        let (_stop, rx) = watch::channel(false);
        let progress = Progress::default();
        let outcome = fetch(&net, &src, probe(&net, &src).await.unwrap(), &out, 8, &progress, rx).await;
        assert_eq!(outcome.unwrap(), Outcome::Done);
        assert_eq!(std::fs::read(&out).unwrap(), *body);
        assert!(!sibling(&out, ".resume").exists() && !sibling(&out, ".part").exists());
        for &(a, b) in log.ranges.lock().unwrap().iter().filter(|r| **r != (0, 0)) {
            for &(s, d) in &on_disk {
                assert!(b < s || a >= d, "refetched {a}-{b}, which overlaps {s}..{d} already on disk");
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_refreshed_link_resumes_though_its_etag_changed() {
        let body = body();
        let (url, log) = serve(body.clone(), 100);
        let dir = scratch("refresh");
        let out = dir.join("file.bin");
        let net = Net::new();
        let src = Source { url, ..Default::default() };

        // Half-download, then pause.
        let progress = Progress::default();
        let (stop, rx) = watch::channel(false);
        let pause = async {
            while progress.done.load(SeqCst) < body.len() as u64 / 3 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            stop.send(true).unwrap();
        };
        let (outcome, ()) = tokio::join!(fetch(&net, &src, probe(&net, &src).await.unwrap(), &out, 2, &progress, rx), pause);
        assert_eq!(outcome.unwrap(), Outcome::Stopped);
        let plan = Plan::parse(&std::fs::read_to_string(sibling(&out, ".resume")).unwrap()).unwrap();
        let on_disk: Vec<_> = plan.parts.iter().map(|p| (p.start, p.done)).filter(|(s, d)| d > s).collect();

        // The new link's server tags the same file differently. As a refresh it resumes anyway.
        *log.etag.lock().unwrap() = "\"v2-other-cdn\"".into();
        log.ranges.lock().unwrap().clear();
        let fresh = Source { refreshed: true, ..src.clone() };
        let (_stop, rx) = watch::channel(false);
        let outcome = fetch(&net, &fresh, probe(&net, &fresh).await.unwrap(), &out, 4, &Progress::default(), rx).await;
        assert_eq!(outcome.unwrap(), Outcome::Done);
        assert_eq!(std::fs::read(&out).unwrap(), *body);
        for &(a, b) in log.ranges.lock().unwrap().iter().filter(|r| **r != (0, 0)) {
            for &(s, d) in &on_disk {
                assert!(b < s || a >= d, "refetched {a}-{b}, which overlaps {s}..{d} already on disk");
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn backs_off_to_the_servers_connection_limit() {
        let body = body();
        let (url, log) = serve(body.clone(), 3);
        let dir = scratch("limit");
        let out = dir.join("file.bin");
        let (net, src) = (Net::new(), Source { url, ..Default::default() });
        let (_stop, rx) = watch::channel(false);
        let outcome = fetch(&net, &src, probe(&net, &src).await.unwrap(), &out, 8, &Progress::default(), rx).await;
        assert_eq!(outcome.unwrap(), Outcome::Done);
        assert_eq!(std::fs::read(&out).unwrap(), *body);
        assert!(log.refused.load(SeqCst) > 0, "the server never had to refuse, so nothing was tested");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn connections_grow_back_when_the_server_allows_more() {
        let body: Arc<Vec<u8>> = Arc::new((0..6_000_000usize).map(|i| (i * 13 % 251) as u8).collect());
        let (url, log) = serve(body.clone(), 2);
        log.delay.store(6, SeqCst);
        let dir = scratch("regrow");
        let out = dir.join("file.bin");
        let (net, src) = (Net::new(), Source { url, ..Default::default() });
        let (_stop, rx) = watch::channel(false);
        let raise = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            log.peak.store(0, SeqCst);
            log.max_conns.store(8, SeqCst);
        };
        let (found, progress) = (probe(&net, &src).await.unwrap(), Progress::default());
        let (outcome, ()) = tokio::join!(fetch(&net, &src, found, &out, 8, &progress, rx), raise);
        assert_eq!(outcome.unwrap(), Outcome::Done);
        assert_eq!(std::fs::read(&out).unwrap(), *body);
        assert!(log.refused.load(SeqCst) > 0, "the server never refused, so nothing was tested");
        assert!(log.peak.load(SeqCst) > 2, "connections never grew back past the old limit: peak {}", log.peak.load(SeqCst));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn limits_speed() {
        let limit = Limiter::default();
        limit.set(100_000);
        let start = tokio::time::Instant::now();
        for _ in 0..10 {
            limit.take(20_000).await;
        }
        // 200 KB at 100 KB/s, the first second's worth free: one second.
        assert_eq!(start.elapsed().as_millis(), 1000);
    }

    #[tokio::test]
    async fn network_errors_say_why_without_the_url() {
        // Port 9 (discard) on localhost: nothing listens, so the connection is refused.
        let url = "http://127.0.0.1:9/a-very-long-signed-link?token=0123456789abcdef";
        let err = Net::new().client.get(url).send().await.unwrap_err();
        let text = network(err).to_string();
        assert!(!text.contains("token="), "{text}");
        assert!(text.to_lowercase().contains("refused"), "the reason should show: {text}");
    }

    #[test]
    fn file_names() {
        assert_eq!(disposition_name(r#"attachment; filename="a b.zip""#).as_deref(), Some("a b.zip"));
        assert_eq!(
            disposition_name("attachment; filename=x.zip; filename*=UTF-8''%E2%82%AC%20rates.pdf").as_deref(),
            Some("€ rates.pdf")
        );
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize(r"..\..\boot.ini"), "boot.ini");
        assert_eq!(sanitize(".."), "");
        assert_eq!(sanitize("a<b>:c?.txt\n"), "a_b__c_.txt_");
    }
}
