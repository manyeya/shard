# shard

A segmented download manager: every file comes down in parallel shards. Get it from
[the website](https://manyeya.github.io/shard/) or the [releases](https://github.com/manyeya/shard/releases). Rust, Tokio + Reqwest for
downloading, GPUI (through [gpui-kit](https://gpui-kit.com)) for the window, SQLite for the list and settings.

```
cargo run --release              # open the download manager
cargo run --release -- <url>     # queue a link (in the open Shard, if there is one)
cargo run --release -- get <url> [output]   # download in the terminal; rerun to resume
cargo run --release -- --hidden  # start in the tray (what launch at login does)
cargo run --release -- serve     # the queue and browser endpoint, no window
scripts/bundle-macos.sh          # target/release/Shard.app
cargo test
CHROME=<Chrome for Testing binary> scripts/test-extension.sh   # the extension, end to end
```

## What it does

- **Parallel parts.** Each download runs up to 8 connections (Settings: 1–32). When a connection
  finishes its part it takes the back half of the part with the most left, so no connection sits
  idle while a slow one finishes.
- **Connection limits.** A server that answers 429 or 503 to one connection too many loses that
  connection; the rest carry on. Half a minute later Shard tries one more, doubling the wait
  (up to four minutes) each time the server refuses again. `Retry-After` is honoured. 404s and
  other client errors fail at once instead of retrying.
- **Combining networks.** Optional (Settings): connections are bound to each network in turn
  (Wi-Fi, Ethernet, a phone over USB), so their speeds add up.
- **Refresh an expired link.** Paste a fresh link to the same file; if it's the same size, the
  download keeps what it has.
- **Pause, resume, crash recovery.** `<file>.part` holds the bytes, `<file>.resume` how far
  each part got. The checkpoint is written only after the data it describes is synced, so after a
  crash or `kill -9` the download resumes from at most a second back. If the file changed on the
  server (ETag / Last-Modified), it says so instead of splicing two versions together.
- **Speed limit.** One budget shared by every connection of every download (Settings, 0 = none).
- **Live segment map.** The UI shows each part of a download at its place in the file, filling
  as its connection works, splitting as idle connections take the back half of a slow part.
- **Tray.** Closing the window keeps Shard running in the menu bar (macOS) or tray (Windows,
  Linux): the S, filling from the bottom with progress, the speed beside it, and a panel with
  the active downloads, a throughput chart and quick controls. Launching Shard again brings the
  window back.
- **Launch at login.** Optional; Shard starts hidden in the tray.
- **Keeps the computer awake** while anything downloads (Settings; on by default). The display
  can still sleep, and closing a laptop's lid still sleeps it.
- **Queue.** Up to 3 downloads at once (Settings: 1–10); the rest wait, in an order you can
  change. Downloads that were running when Shard quit resume when it starts again. Search and
  sort the list.
- **Schedule.** A start and a stop time for the queue, and what to do when it empties: nothing,
  sleep or shut down (after a minute you can cancel).
- **Where files go.** The Downloads folder, a folder of your choosing, asked each time, or sorted
  into Video, Music, Archives, Documents and Programs.
- **Clipboard and notifications.** Copy a link to a file and Shard offers to download it; a
  notification says when a download finishes or why it couldn't.
- **Updates.** Every six hours Shard reads a signed list of releases (`update.txt`, Ed25519),
  downloads the build for this computer in the background, checks its SHA-256 and offers
  Restart to update. Settings can turn it off.
- **Names.** From `Content-Disposition`, else the URL, cleaned of path tricks and characters
  Windows refuses; `name (1).ext` when the name is taken.
- **Browser integration.** The extension in `extension/` hands new downloads to Shard with the
  page's cookies, referrer and user agent, so downloads behind a login work. If Shard isn't
  running, the browser downloads as usual.

## Browser extension (Chrome, Edge, Brave)

1. Open `chrome://extensions` and turn on Developer mode.
2. Load unpacked, and pick the `extension` folder.

Shard listens on `127.0.0.1:47110` and only takes downloads from browser extensions: a request
carrying a web page's Origin is refused, so a site can't queue downloads on your machine.

## Layout

| File | What |
| --- | --- |
| `src/engine.rs` | One download: probe, worker pool, part splitting, checkpoints |
| `src/queue.rs` | The queue manager: scheduling, naming, pause/resume, progress for the UI |
| `src/db.rs` | SQLite: downloads and settings |
| `src/browser.rs` | The localhost endpoint the extension talks to |
| `src/system.rs` | Keeping the computer awake, launch at login, sleep and shut down |
| `src/networks.rs` | The computer's network connections, for combining them |
| `src/update.rs` | Checking for, downloading and installing new versions |
| `src/tray.rs` | The menu bar / tray icon and its menu |
| `src/logo.rs` | The logo's outline, for drawing it (made by `scripts/logo.py`) |
| `src/ui.rs` | The GPUI window |
| `site/` | The website, static, published by `.github/workflows/pages.yml` |
| `extension/` | The browser extension |
| `assets/fonts/` | Geist, Geist Mono and Archivo Expanded, under the SIL Open Font License |
| `assets/logo/` | The logo, the sliced S: `shard-icon.svg` (app icon), `shard-mark.svg`, `shard-mark-mono.svg` (one colour), all made by `scripts/logo.py` from the display font |
| `assets/icons/` | Generated from the logo by `scripts/icons.sh`: `Shard.icns`, `shard.ico`, `png/` |
| `packaging/linux/` | The `.desktop` entry |
| `build.rs` | Embeds the icon in `shard.exe` on Windows |

Data lives in `~/Library/Application Support/shard` (macOS), `~/.local/share/shard` (Linux) or
`%APPDATA%\shard` (Windows); `SHARD_DATA` overrides it.

## Not yet

- Torrents.
- Downloads a page starts with a POST (some form submissions): Shard fetches the URL with a GET.
- Firefox: its Manifest V3 wants `background.scripts`, so it needs its own manifest.

## Releases

`.github/workflows/release.yml` builds every platform when a tag `vX.Y.Z` matching
`Cargo.toml`'s version is pushed, and publishes them as one GitHub release with `SHA256SUMS`:

| Platform | Package |
| --- | --- |
| macOS (Apple silicon and Intel) | `Shard-X.Y.Z-macos-universal.dmg` |
| Windows (x64) | `Shard-X.Y.Z-windows-x64.zip` |
| Linux (x64, arm64) | `shard-X.Y.Z-linux-<arch>.tar.gz` and `shard_X.Y.Z_<arch>.deb` |
| Chrome, Edge, Brave, Arc | `shard-extension-X.Y.Z.zip` |

It also publishes what installed copies update from: `update.txt` (the version, and each
platform's file with its size and SHA-256), its Ed25519 signature `update.txt.sig`, the app
zipped for macOS and the bare binaries for Windows and Linux. The release fails without the
`UPDATE_SIGNING_KEY` secret, the private key whose public half is in `src/update.rs`.

Running the workflow by hand builds the same packages as artifacts without releasing.
`.github/workflows/ci.yml` runs the tests on all three systems for every push and pull request.

Signing is optional, step by step: without secrets the macOS app is signed ad hoc (Gatekeeper asks
before the first launch) and the extension isn't uploaded to the Chrome Web Store.

| Secrets | What they turn on |
| --- | --- |
| `MACOS_CERTIFICATE` (Developer ID Application `.p12`, base64), `MACOS_CERTIFICATE_PASSWORD` | Signing with the hardened runtime |
| `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` | Notarising and stapling the disk image |
| `CWS_EXTENSION_ID`, `CWS_CLIENT_ID`, `CWS_CLIENT_SECRET`, `CWS_REFRESH_TOKEN` | Publishing the extension to the Chrome Web Store |

After changing the logo, run `scripts/logo.py` (needs fontTools), then
`CHROME=<Chrome for Testing binary> scripts/icons.sh`, and commit what they write.
