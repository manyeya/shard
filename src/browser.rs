//! The browser extension hands downloads over here, on 127.0.0.1:47110 (see `port`):
//! `GET /ping` says the app is up, `POST /add` (form fields url, cookie, referrer, useragent) queues one,
//! `POST /show` (from a second `shard` launch) brings the window back, `POST /quit` (from setup,
//! before it replaces this copy) quits, and `POST /hello` is the extension saying it's installed.
//!
//! Web pages can send requests to localhost too, so anything carrying an Origin that isn't a
//! browser extension is refused: a page can't forge its Origin, so it can't start downloads.

use crate::queue::Manager;
use reqwest::header::{COOKIE, HeaderMap, HeaderName, HeaderValue, REFERER, USER_AGENT};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Where the extension finds Shard. `SHARD_PORT` moves it, so a development copy can run beside an installed one.
pub fn port() -> u16 {
    std::env::var("SHARD_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(47110)
}
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 256 * 1024; // cookies can be long

pub async fn serve(listener: TcpListener, manager: Arc<Manager>) {
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let m = manager.clone();
        tokio::spawn(async move {
            let _ = tokio::time::timeout(Duration::from_secs(10), handle(stream, &m)).await;
        });
    }
}

async fn handle(mut stream: TcpStream, m: &Manager) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > MAX_HEAD || read_some(&mut stream, &mut buf).await? == 0 {
            return Ok(());
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request = lines.next().unwrap_or("").split(' ');
    let (method, path) = (request.next().unwrap_or(""), request.next().unwrap_or(""));
    let headers: HashMap<String, &str> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim())).collect();

    if headers.get("origin").is_some_and(|o| !(o.starts_with("chrome-extension://") || o.starts_with("moz-extension://"))) {
        return respond(&mut stream, "403 Forbidden", "only the Shard browser extension can add downloads").await;
    }
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return respond(&mut stream, "413 Payload Too Large", "too large").await;
    }
    while buf.len() < head_end + len {
        if read_some(&mut stream, &mut buf).await? == 0 {
            return Ok(());
        }
    }
    let body = &buf[head_end..head_end + len];

    match (method, path) {
        ("GET", "/ping") => respond(&mut stream, "200 OK", "shard").await,
        // POST, not GET: a page could GET it with an <img> and no Origin, over and over.
        ("POST", "/show") => {
            m.request_show();
            respond(&mut stream, "200 OK", "shown").await
        }
        ("POST", "/quit") => {
            m.request_quit();
            respond(&mut stream, "200 OK", "quitting").await
        }
        ("POST", "/hello") => respond(&mut stream, "200 OK", "hello").await,
        ("POST", "/add") => {
            let form: HashMap<String, String> = url::form_urlencoded::parse(body).into_owned().collect();
            let Some(url) = form.get("url") else {
                return respond(&mut stream, "400 Bad Request", "no url").await;
            };
            let mut h = HeaderMap::new();
            for (field, name) in [("cookie", COOKIE), ("referrer", REFERER), ("useragent", USER_AGENT)] {
                if let Some(v) = form.get(field).filter(|v| !v.is_empty()).and_then(|v| HeaderValue::from_str(v).ok()) {
                    h.insert::<HeaderName>(name, v);
                }
            }
            match m.add_with_headers(url, h) {
                Ok(_) => respond(&mut stream, "200 OK", "queued").await,
                Err(e) => respond(&mut stream, "400 Bad Request", &e).await,
            }
        }
        _ => respond(&mut stream, "404 Not Found", "not found").await,
    }
}

async fn read_some(stream: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<usize> {
    let mut chunk = [0; 8192];
    let n = stream.read(&mut chunk).await?;
    buf.extend_from_slice(&chunk[..n]);
    Ok(n)
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    let msg = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(msg.as_bytes()).await
}

/// Whether the Shard on our port is up (rather than some other program).
pub fn running() -> bool {
    ask("GET /ping", "").is_ok_and(|reply| reply == "shard")
}

/// Hand a URL to the Shard that's already running.
pub fn hand_over(url: &str) -> Result<(), String> {
    let body: String = url::form_urlencoded::Serializer::new(String::new()).append_pair("url", url).finish();
    ask("POST /add", &body).map(drop)
}

/// Bring the running Shard's window back.
pub fn show() -> Result<(), String> {
    ask("POST /show", "").map(drop)
}

/// Ask the running Shard to quit.
pub fn quit() -> Result<(), String> {
    ask("POST /quit", "").map(drop)
}

/// Setup holds the extension's port while it waits for the extension to say hello: `heard()`
/// counts the hellos. Anything else gets a 404, so the browser keeps its downloads meanwhile.
pub struct Hello {
    heard: Arc<std::sync::atomic::AtomicUsize>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl Hello {
    /// None when the port is taken.
    pub fn listen() -> Option<Hello> {
        use std::sync::atomic::Ordering::Relaxed;
        let listener = std::net::TcpListener::bind(("127.0.0.1", port())).ok()?;
        listener.set_nonblocking(true).ok()?;
        let (heard, stop) = (Arc::new(std::sync::atomic::AtomicUsize::new(0)), Arc::new(std::sync::atomic::AtomicBool::new(false)));
        let (h, s) = (heard.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if answer_hello(stream) {
                            h.fetch_add(1, Relaxed);
                        }
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        });
        Some(Hello { heard, stop })
    }

    pub fn heard(&self) -> usize {
        self.heard.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Drop for Hello {
    /// Let go of the port (within a tenth of a second), so the installed Shard can have it.
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Answer one request on setup's listener; true when it was the extension's hello.
fn answer_hello(mut s: std::net::TcpStream) -> bool {
    use std::io::{Read, Write};
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut chunk = [0; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < MAX_HEAD {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&buf).to_ascii_lowercase();
    // As in `handle`: a web page always sends its Origin, so refuse those; extensions send theirs,
    // or (Firefox, for hosts it has permission for) none.
    let from_page = head
        .lines()
        .filter_map(|l| l.strip_prefix("origin:"))
        .any(|o| !(o.trim().starts_with("chrome-extension://") || o.trim().starts_with("moz-extension://")));
    let hello = head.starts_with("post /hello ") && !from_page;
    let (status, body) = if hello { ("200 OK", "hello") } else { ("404 Not Found", "setup is running") };
    let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    hello
}

/// One request to the running Shard; Ok is the reply's body when it says 200.
fn ask(request: &str, body: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port())).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    write!(s, "{request} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}", body.len()).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    s.read_to_string(&mut reply).map_err(|e| e.to_string())?;
    match reply.split_once("\r\n\r\n") {
        Some((head, msg)) if head.starts_with("HTTP/1.1 200") => Ok(msg.to_string()),
        Some((_, msg)) => Err(msg.to_string()),
        None => Err("no answer".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::scratch;

    #[test]
    fn setup_hears_the_extensions_hello_and_not_a_pages() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let send = |origin: &str| {
            let mut s = std::net::TcpStream::connect(addr).unwrap();
            write!(s, "POST /hello HTTP/1.1\r\nHost: 127.0.0.1\r\n{origin}Content-Length: 0\r\n\r\n").unwrap();
            let (stream, _) = listener.accept().unwrap();
            let heard = answer_hello(stream);
            let mut reply = String::new();
            s.read_to_string(&mut reply).unwrap();
            (heard, reply.starts_with("HTTP/1.1 200"))
        };
        assert_eq!(send("Origin: chrome-extension://abcdef\r\n"), (true, true));
        assert_eq!(send(""), (true, true), "Firefox may send no Origin");
        assert_eq!(send("Origin: https://evil.example\r\n"), (false, false));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn only_the_extension_can_add() {
        let dir = scratch("browser");
        let m = Manager::open(&dir.join("shard.db"), dir.clone(), tokio::runtime::Handle::current()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve(listener, m.clone()));

        let post = |origin: &'static str| async move {
            let body = "url=http%3A%2F%2F127.0.0.1%3A9%2Fa.zip&cookie=sid%3D42&referrer=http%3A%2F%2Fexample.com%2F";
            let req = format!("POST /add HTTP/1.1\r\nOrigin: {origin}\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            let mut s = TcpStream::connect(addr).await.unwrap();
            s.write_all(req.as_bytes()).await.unwrap();
            let mut reply = String::new();
            s.read_to_string(&mut reply).await.unwrap();
            reply
        };
        assert!(post("https://evil.example").await.starts_with("HTTP/1.1 403"));
        assert!(m.rows().is_empty());
        assert!(post("chrome-extension://abcdef").await.starts_with("HTTP/1.1 200"));
        let rows = m.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "http://127.0.0.1:9/a.zip");
        m.pause_all(); // its URL is a closed port; no need to watch it retry
        let saved = crate::db::Db::open(&dir.join("shard.db")).unwrap().all().unwrap();
        assert!(saved[0].headers.contains("cookie: sid=42") && saved[0].headers.contains("referer: http://example.com/"));
        // Best effort: Windows won't delete the database while the manager still has it open.
        let _ = std::fs::remove_dir_all(dir);
    }
}
