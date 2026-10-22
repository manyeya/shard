# Architecture

How Shard is put together: the threads it runs, how a link becomes a file on disk, how the queue
schedules downloads, how the browser extension and a second launch talk to it, how updates are
checked and installed, how setup installs it, and where everything is stored. File and function
names refer to `src/`.

## Contents

- [One binary, several modes](#one-binary-several-modes)
- [Threads and runtimes](#threads-and-runtimes)
- [From a link to bytes on disk](#from-a-link-to-bytes-on-disk)
- [Backing off and growing back](#backing-off-and-growing-back)
- [The queue manager](#the-queue-manager)
- [Combining networks](#combining-networks)
- [The browser endpoint](#the-browser-endpoint)
- [The UI](#the-ui)
- [The tray](#the-tray)
- [Updates](#updates)
- [Setup](#setup)
- [Persistence](#persistence)
- [Where files live](#where-files-live)

## One binary, several modes

Everything is one executable, `shard`. `main.rs` picks what to do from the arguments:

| Command | What runs |
| --- | --- |
| `shard` | The window, the tray, the queue, the browser endpoint and the updater. |
| `shard <url>` | The same, with the link queued. If a Shard is already running, the link is handed to it and this process exits. |
| `shard --hidden` | The same without the window: just the tray (what launch at login runs). |
| `shard serve` | The queue and the browser endpoint, with no window, no tray and no updater. |
| `shard get <url> [output]` | One download in the terminal, with no queue, database or endpoint. Running it again resumes. |
| `shard --setup` | The setup window, installing this copy (see [Setup](#setup)). |
| `shard --uninstall` | The setup window, removing the installed Shard. |
| `--silent`, with either | The same steps with the defaults and no window, reported on stdout; the exit code says how it went. |
| `shard --version` | Prints `shard X.Y.Z`. |

Before any of those, `install::mode` decides whether this launch is setup: `--setup` and
`--uninstall` ask for it, and a copy that isn't installed offers it when it's opened with no
arguments (or only `--silent`). Setup runs instead of the app: no queue, endpoint, tray or
updater.

Only one Shard runs per user. The browser endpoint's port (`127.0.0.1:47110`, or `SHARD_PORT`)
doubles as the lock: `app()` tries to bind it, and if that fails and `GET /ping` answers
`shard`, this launch hands its link over (`POST /add`) or asks for the window (`POST /show`) and
exits. If the port is taken by some other program, Shard starts anyway, prints that the browser
extension won't reach it, and runs without the endpoint.

## Threads and runtimes

```
                     shard process
 ┌─────────────────────────────────────────────────────────────────────┐
 │ main thread: GPUI                                                    │
 │   main window (IdmView)   polls Manager every 400 ms                 │
 │   tray panel (TrayPanel)  polls Manager every 400 ms                 │
 │   shell loop              every 100 ms: tray clicks, show requests,  │
 │                           window closed; every 1 s: notifications,   │
 │                           copied links, when-done countdown, tray    │
 │                                                                      │
 │            │ plain method calls (Manager's API is sync and cheap)    │
 │            ▼                                                         │
 │ Arc<Manager>  ── Mutex<Inner>: settings, items, SQLite connection    │
 │            │ spawns                                                  │
 │            ▼                                                         │
 │ Tokio multi-thread runtime                                           │
 │   Manager tick (every 1 s)    one task per running download          │
 │   browser endpoint (accept loop, a task per connection)              │
 │   updater loop (every 6 h, or on "Check now")                        │
 │                                                                      │
 │ keep-awake thread (owns the OS's "don't idle-sleep" request)         │
 │ tray-icon's own threads on Linux (StatusNotifierItem over D-Bus)     │
 └─────────────────────────────────────────────────────────────────────┘
```

- **The main thread belongs to GPUI.** `ui::run` starts the GPUI application and returns only
  when it quits (on macOS GPUI ends the process through `[NSApp terminate:]` instead). The UI
  never awaits a download; it reads the queue's state and calls its methods.
- **A Tokio multi-thread runtime** (`tokio::runtime::Runtime::new()` in `main.rs`) runs
  everything that does I/O: downloads, the endpoint, the updater and the queue's once-a-second
  tick.
- **`Manager` sits between them.** Its public methods are synchronous and only take a mutex
  briefly, so the UI thread calls them directly. Anything slow is spawned onto the runtime
  through the `Handle` the manager keeps.
- **Keeping awake** has its own thread (`system::Awake`), because on Windows the request belongs
  to the thread that made it, and the runtime's threads come and go. The queue sends it on or
  off over a channel.
- `shard get` starts a runtime and blocks on one download; `shard serve` starts the runtime and
  waits forever.

## From a link to bytes on disk

```
 add(url) ──► parse_link: http/https only
    │
    ▼
 db.insert (status "queued", position = last + 1) ──► Item in Manager
    │
    ▼  schedule(): a free slot (fewer than max_active running)
 launch(): stop channel, Job spawned on the runtime
    │
    ▼
 engine::probe ── GET with "Range: bytes=0-0"
    │   206 ─► size from Content-Range, validator = strong ETag or Last-Modified
    │   200 ─► the server ignored the range: one stream, not resumable
    │   416 ─► asked again without a range (an empty file)
    ▼
 Manager::named: first run only. Category folder if "Sort into folders",
                 then a free name: "name.ext", "name (1).ext", ...
    │
    ▼
 engine::fetch
    ├─ whole response?  ─► stream it into <file>.part, rename when complete
    └─ ranged:
         load <file>.resume if it matches, else create <file>.part at full size
         spawn N workers ──► chunks ──► one writer ──► <file>.part
                                         every 1 s: sync, then save <file>.resume
         all parts complete ─► rename .part to the file, delete .resume
```

### The probe

`engine::probe` asks for the first byte. A server that honours ranges answers 206 with
`Content-Range: bytes 0-0/<size>`, which gives the size. The validator is the strong `ETag`
(weak ones, `W/…`, can't be used with `If-Range`) or else `Last-Modified`. The file name comes
from `Content-Disposition` (`filename*` in RFC 5987 form first, then `filename`), else the last
segment of the final URL, and is then sanitised: only the last path component, control
characters and `<>:"|?*` replaced, leading dots and spaces trimmed, at most 200 bytes.

If the server answers 200 instead, it ignored the range and is sending the whole file. That
response is kept and streamed as is: one connection, nothing to resume from.

### The plan and the workers

For a ranged download, `fetch` holds a `Plan`: the file's length, its validator, and a list of
parts, each `[start, end)` with `done` (written to disk) and `sent` (handed to the writer). A new
download starts with one part covering the whole file and a `.part` file preallocated to full
size with `set_len`.

It then spawns `connections` workers (8 by default). Each worker asks the plan for work
(`Plan::take`):

1. a part nobody is working on, if there is one;
2. otherwise the busy part with the most bytes left: its back half is cut off into a new part,
   as long as at least `2 × MIN_PART` (512 KiB) is left, so no part is ever smaller than
   `MIN_PART` (256 KiB).

So the first connection takes the whole file, each one after it takes the back half of the
largest remaining part, and a connection that finishes early splits the slowest part again. No
connection sits idle while there's a part worth splitting.

A worker sends `Range: bytes=<sent>-<end - 1>` with `If-Range: <validator>`, and accepts the
answer only if it's 206 starting exactly where it asked. Anything else means the file changed on
the server, which fails the download with "the file changed on the server; download it again from
the start" rather than splicing two versions together. While it streams, the part may shrink
under it (another worker took its back half), so each chunk is trimmed to the part's current end.

### The writer and checkpoints

Workers send `(part, offset, bytes)` over a bounded channel (64 chunks) to one writer, the loop in
`fetch`, which owns the file. It seeks, writes, and moves the part's `done` forward. Every 200 ms
it publishes the part layout for the UI's segment map. Every second it calls `sync_data` on the
`.part` file and only then writes the checkpoint, so a checkpoint never claims bytes that aren't
on disk.

The checkpoint, `<file>.resume`, is plain text:

```
<length>
<validator>
<start> <end> <done>
<start> <end> <done>
...
```

It's written to `<file>.resume.tmp` and renamed over the old one. After a crash, a download
resumes from at most about a second back. The checkpoint itself isn't fsynced: a torn one fails to
parse, and the download starts again from the beginning.

On the next run, the checkpoint is used only if its length matches the probe's, the `.part` file
exists, and the validator matches. A refreshed link (see below) skips the validator check, since
another server may tag the same file differently; the length must still match.

### Finishing and stopping

When every worker has finished and every part's `done` equals its `end`, the writer syncs, the
`.part` file is renamed to the final name and the checkpoint is deleted. Pausing sends `true` on
the download's stop channel: the writer syncs, saves the checkpoint and returns `Stopped`, and the
remaining workers are aborted.

### HTTP details

- One `reqwest` client per route, HTTP/1.1 only. Over HTTP/2 every part would be a stream on one
  TCP connection, which is the throttle parallel parts exist to get around.
- User agent `shard/<version>` unless the browser's is passed along. Connect timeout 15 s; a read
  that stalls for 30 s becomes a retry.
- Requests carry the download's saved headers (cookie, referrer, user agent from the browser).
- Error messages drop the URL, since signed links run to hundreds of characters, and include the
  underlying cause ("connection refused").

### Retries

Each failed request is classified:

| Answer | Treated as | What happens |
| --- | --- | --- |
| 429, 503 | Busy | See [backing off](#backing-off-and-growing-back). `Retry-After` in seconds is honoured (capped at a minute). |
| Network error, 5xx, 408 | Retry | Wait `500 ms × 2^n` (n up to 6) and try again. |
| Other 4xx (404, 403…) | Fatal | The download fails at once. |

A worker retries up to 5 times in a row without a byte of progress, then fails the download;
any progress resets the count. The probe retries the same way.

### The speed limit

`Limiter` is a token bucket shared by every connection of every download (one `Net` is shared by
the whole queue). Each chunk waits until it fits under the rate; bursts are capped at one second's
worth. 0 means no limit.

## Backing off and growing back

Many servers allow only a few connections per client and answer the rest with 429 or 503.

```
 8 workers ──► server allows 3 ──► 5 get 429
                                    │ each refused worker hands its part back and retires
                                    │ (unless it is the last one, which keeps retrying)
                                    ▼
                          3 workers carry the download
                                    │ every 5 s: has it been quiet for regrow_after?
                                    ▼
                          spawn one more worker ──► accepted: keep it, try another later
                                                └─► refused: retire it, regrow_after × 2
```

- When a worker's request is refused (Busy), `refused_at` is set. If more than one worker is
  still active, the refused worker releases its part and exits. The last one never retires: it
  keeps retrying with the usual backoff, and fails the download only if it's refused more than 5
  times in a row without progress.
- Every `REGROW_TICK` (5 s) the writer checks: if `REGROW_AFTER` (30 s, doubled up to
  `REGROW_MAX`, 4 minutes) has passed since both the last refusal and the last extra worker,
  fewer workers are running than asked for, and the plan has a part worth splitting, it spawns one
  more worker.
- If that extra worker is refused too, the wait before the next try doubles.

The result is that Shard settles at the server's limit and returns to the full count when the
limit lifts.

## The queue manager

`queue::Manager` owns every download and the settings. Its state, `Inner`, is behind one mutex:
the SQLite connection, the settings, the items in queue order, the throughput history, the
networks and their speeds, and a few flags.

Each item has a status: `queued`, `downloading`, `paused`, `done` or `failed`.

- **Scheduling.** `schedule()` walks the items in queue order and launches queued ones while
  fewer than `max_active` (3 by default, 1 to 10) are running. It runs after every change: adding,
  pausing, resuming, moving, a download finishing, a settings change.
- **One task per download.** A download's task is never started twice. A paused download resumed
  while its last task is still stopping waits as queued until that task ends.
- **Queue order.** Each download has a `position`. Moving one (top, up, down, bottom) renumbers
  them all, and waiting downloads start from the top.
- **The tick.** Once a second, `tick()` samples each running download's speed (a running average
  of the last two seconds), keeps a minute of speed history per download and for the whole queue,
  works out each part's speed for the segment map, saves progress to the database, updates each
  network's speed, rescans networks if 30 s have passed, holds or releases the keep-awake request,
  and applies the schedule's start and stop times.
- **The schedule.** Once per minute, at `start_at` paused downloads are queued again, and at
  `stop_at` everything queued or running is paused.
- **Finishing.** When a task ends: done downloads drop their saved headers (the cookies aren't
  needed any more); stopped ones become paused; errors become failed with the message. When the
  last download finishes and nothing is waiting, a `drained` counter goes up, and the UI starts
  the when-done countdown if one is set.
- **Restarting.** On open, downloads saved as `downloading` become `queued`, so they carry on from
  their checkpoints.
- **Refreshing a link.** `refresh_link` replaces the URL, clears the error, queues the download
  again and marks the next run as refreshed (resume if the size matches, whatever the validator).
- **Removing.** The row is deleted at once. A running download is stopped first and dropped when
  its task ends. Partial files (`.part`, `.resume`) are always deleted; the finished file only
  when asked.
- **Settings.** `set_settings` clamps the numbers (1 to 10 downloads, 1 to 32 connections),
  writes every setting to the `settings` table, changes launch at login through the system if it
  changed, applies the speed limit, and rescans networks.

## Combining networks

```
 networks::list()          every 30 s: interfaces that are up, not loopback, with a routable
        │                  address, and not a VPN, bridge or VM adapter (utun, awdl, docker,
        ▼                  tailscale, wg, ...); one per interface, IPv4 preferred
 settings: combine_networks on, minus networks_off
        │
        ▼
 Net::set_routes  ── fewer than 2 ─► no routes: the system's default route
        │
        ▼ 2 or more
 one reqwest client per network, bound to the interface (macOS, Linux)
 or to its address (Windows, where reqwest can't name an interface)
        │
        ▼
 worker k starts on route k mod n; a network error moves it to the next route
```

Each route counts the bytes it carried, which the UI shows as each network's share. A download
takes the routes that exist when its run starts. On macOS, interface names are matched to the
names people know ("Wi-Fi", "iPhone USB") with `networksetup -listallhardwareports`. The probe
always uses the default route.

## The browser endpoint

`browser.rs` is a small hand-written HTTP/1.1 server on `127.0.0.1:47110` (`SHARD_PORT` moves
it). Every response is `text/plain` with `Connection: close`.

| Request | Body | Answer |
| --- | --- | --- |
| `GET /ping` | | `200 shard`. How a second launch, and the extension test, tell that Shard is up. |
| `POST /add` | Form-encoded `url`, and optionally `cookie`, `referrer`, `useragent` | `200 queued`, or `400` with the reason ("no url", "Only http and https links can be downloaded."). |
| `POST /show` | | `200 shown`; the window comes forward. Sent by a second `shard` launch. |
| `POST /hello` | | `200 hello`. The extension sends it, empty, when it's installed or updated; a running Shard only answers it, and setup counts it (see [Setup](#setup)). |
| `POST /quit` | | `200 quitting`; Shard quits. Setup sends it before replacing or removing a running Shard. |
| anything else | | `404 not found` |

The client side, used by a second launch and by setup, is in the same file: `running()` (does
`GET /ping` answer `shard`?), `hand_over()`, `show()` and `quit()`.

Before routing, any request with an `Origin` header that doesn't start with `chrome-extension://`
or `moz-extension://` is refused with `403`. Headers are limited to 16 KiB and the body to
256 KiB (cookies can be long, so `413` above that), and each connection has 10 seconds. The
cookie, referrer and user agent become `Cookie`, `Referer` and `User-Agent` headers on every
request for that download.

The extension (`extension/background.js`) listens to `chrome.downloads.onCreated`. For an
`http(s)` download in progress, it reads the cookies for the URL, posts them to `/add`, and on a
`200` cancels and erases the browser's own download. Any failure (Shard not running, a refusal)
leaves the download with the browser. From `chrome.runtime.onInstalled` it posts an empty
`/hello`. The address `http://127.0.0.1:47110` is written into the script, so it never follows
`SHARD_PORT`.

The same script runs everywhere. `scripts/package-extension.sh` packages it twice: for Chromium
browsers as a Manifest V3 service worker, and for Firefox (128 or later) as an event page with
the add-on ID `shard-integration@manyeya.github.io`. People install it from the Chrome Web
Store, Edge Add-ons or Firefox Add-ons.

## The UI

`ui.rs` is a GPUI application drawn as an instrument panel: a command bar in the title bar, a hero
card with total speed, the live connection graph and the throughput chart, the queue as cards with
a detail panel, a stats view, and the Limits panel for settings.

- **Polling, not events.** The main window polls the manager every 400 ms (`POLL`): `rows()`,
  `throughput()`, `settings()`, `networks()` and the updater's state. Drawn values ease towards
  each new poll over 0.4 s, so numbers and bars move smoothly between samples.
- **The shell loop** (`run_shell`) runs every 100 ms: it handles tray clicks and menu choices,
  opens the window when a second launch asked (`show_requests` went up), and notices the window
  closing. Once a second it posts notifications for downloads that finished or failed, offers a
  copied link to a file, runs the when-done countdown (a minute's warning, cancelled if anything
  starts), and updates the tray.
- **Notifications** go through GPUI's system notifications. On macOS GPUI turns them off when the
  binary isn't inside an app bundle.
- **Copied links.** With "Offer copied links" on, the clipboard is read once a second. A new
  `http(s)` link whose last segment looks like a file (by extension), and that isn't already in
  the list, gets a notification with a Download button. The link bar also picks up a copied link
  when the window is activated.
- **Closing the window** leaves Shard in the tray (on macOS it leaves the Dock too). With no tray,
  closing the last window quits.
- **Keys.** ⌘ on macOS, Ctrl elsewhere: Q quits, L or N focuses the link bar, comma opens Limits,
  V pastes a link. In the list: space pauses or resumes, the arrow keys select, Delete or
  Backspace removes, Return opens the selected file once
  it's finished.

## The tray

`tray.rs` uses the `tray-icon` crate everywhere: native on macOS and Windows, and on Linux its
StatusNotifierItem backend over D-Bus (built without GTK). The icon is the logo rasterised from
`logo.rs`'s outline at 44 px, filling from the bottom with the running downloads' progress; it's
redrawn only when the percentage changes. On macOS it's a template image (only alpha counts) with
the speed as its title.

A left click opens the tray panel by the icon. Linux trays don't report where their icon is, so
there a left click opens the main window. The menu has the summary line, Open Shard, Pause all,
Resume all, a speed cap submenu (no cap, 1, 5, 10, 25 MB/s), keep awake, launch at login and Quit,
plus "Cancel" while a sleep or shut down counts down and "Restart to update" when an update is
ready.

## Updates

```
 every 6 h (or "Check now")
   │  skipped when "Update automatically" is off (except Check now) or SHARD_NO_UPDATE is set
   ▼
 target(): can this copy update itself?  no ─► Manual(reason) in the Limits panel
   │ yes
   ▼
 GET releases/latest/download/update.txt and update.txt.sig   (404: no release yet)
   ▼
 Ed25519 verify_strict(update.txt bytes, sig, PUBLIC_KEY)     fail ─► Failed
   ▼
 parse; manifest version > running version?                  no ─► Idle
   ▼
 download this platform's build to <data>/updates/<version>/ with the engine (4 connections,
 resumable; skipped if a file with the right SHA-256 is already there)
   ▼
 SHA-256 matches the manifest?                                no ─► delete, Failed
   ▼
 stage: macOS unzips Shard.app with ditto; elsewhere chmod 755
   ▼
 Ready ─► "Restart to update" in the title bar, the Limits panel and the tray menu
   ▼  (the user chooses it)
 swap: target ─► target.old, staged ─► target (copy if the rename can't cross volumes;
       the old one goes back if that fails)
   ▼
 start the new binary with SHARD_RESTARTED=1, quit
   ▼
 new process: retries binding the port for up to 10 s while the old one exits;
              at start, update::clean_up() deletes target.old
```

The manifest, `update.txt`, is written by the release workflow:

```
shard-update 1
version 0.2.0
macos-universal <size> <sha256> https://github.com/manyeya/shard/releases/download/v0.2.0/Shard-0.2.0-macos-universal.zip
windows-x64 <size> <sha256> https://github.com/manyeya/shard/releases/download/v0.2.0/Shard-0.2.0-windows-x64.exe
linux-x64 <size> <sha256> https://github.com/manyeya/shard/releases/download/v0.2.0/shard-0.2.0-linux-x64
linux-arm64 <size> <sha256> https://github.com/manyeya/shard/releases/download/v0.2.0/shard-0.2.0-linux-arm64
```

The first line is the format version; a manifest in a format this build doesn't know is refused.
Versions compare by semver. The platform key is `macos-universal` on any Mac, `windows-x64`,
`linux-x64` or `linux-arm64`; other platforms find no build and stay as they are.

What an update replaces (`target()`): on macOS, the `Shard.app` around the running binary (a
binary that isn't in one can't update itself); on Linux, nothing under `/usr` (package-managed
installs update through their package manager); elsewhere, the running binary. In every case the
folder must be writable, which `target()` tests by creating and deleting `.shard-update-check`
there. The old copy is kept beside the new one as `.old` until the next start, because a running
Windows binary can be renamed but not deleted. An AppImage that's run without installing can't
update itself: its binary is in the AppImage's read-only mount.

## Setup

`install.rs` knows where Shard goes on each system and makes the steps (`Task`s: a label, a
detail, and work that reports progress from 0 to 1) that put it there and take it out again.
`setup.rs` is the window that runs them one at a time on a worker thread and draws them.
`--silent` runs the same steps with the defaults (start at login on, no desktop shortcut, no
extension step, history kept when removing), printing each label, and exits non-zero if one
fails.

### When setup runs

`--setup` and `--uninstall` always run it. Otherwise a copy opened with no arguments offers it
when its file name contains "setup" (the Windows `Shard-Setup-….exe`); on macOS, when it's a
`Shard.app` outside any `Applications` folder (opened from the disk image or Downloads); on Linux,
when it isn't under `/usr`, `/opt`, `/nix`, `/snap` or `~/.local/bin` and nobody has chosen "Run
without installing" there before (that leaves a `run-in-place` file in the data folder). Builds
under a `target` folder and the Windows portable zip never offer it, and `SHARD_NO_SETUP` (set to
anything) turns the offer off. Setup sets it itself when it starts Shard afterwards.

### Where it installs

| System | The app | Also |
| --- | --- | --- |
| macOS | `/Applications/Shard.app`, or `~/Applications/Shard.app` when `/Applications` isn't writable | The login entry, if chosen |
| Windows | `%LOCALAPPDATA%\Programs\Shard\Shard.exe` | `Shard.lnk` in the Start menu (and on the desktop, if chosen); the Settings › Apps entry under `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Shard`; the login entry, if chosen. No administrator rights are needed. |
| Linux | `~/.local/bin/shard` | `shard.desktop` in `$XDG_DATA_HOME/applications` (else `~/.local/share/applications`), with `Exec` set to the installed binary; the icon at 32 to 512 px under `icons/hicolor` beside it; the login entry, if chosen |

`SHARD_INSTALL_DIR` replaces the folder the app goes in.

### Installing

The steps, in order:

1. **Closing Shard**, if one answers on the port: `POST /quit`, then wait (up to 15 seconds) until
   `GET /ping` stops answering.
2. **Copying Shard**, unless this copy is the installed one: copied in beside the old one as
   `.new` (with `ditto` on macOS, which then removes the quarantine attribute, so the installed
   copy can update itself), then swapped in the way updates are, so a failed copy leaves the old
   one working. On macOS only a `Shard.app` installs: run from a bare binary, setup stops with an
   error before the first step.
3. On Windows, **adding shortcuts** (made with the shell's `WScript.Shell` object through
   PowerShell) and **adding to Settings › Apps**: the display name, version, publisher "The Shard
   Authors", icon, size, and `UninstallString` and `QuietUninstallString`, which run
   `Shard.exe --uninstall` with and without `--silent`.
4. On Linux, **adding to your apps**: the desktop entry and icons, then `update-desktop-database`
   and `gtk-update-icon-cache` if they're there.
5. **Starting at login**, or turning it off.

### The window

The window has four scenes: the choices, the steps running, the browser extension, and done (or
"Setup stopped" with the error and Try again). The first scene's title says Install, Update to
X.Y.Z or Reinstall, from the version the installed copy reports with `--version`. Its options
are the install folder, start at login, a desktop shortcut (Windows), and the browser extension,
with a chip for each browser it can send to a store; "Run without installing" starts this copy as
it is instead.
Removing has one option, to delete the history and settings (the data folder) as well.

`SHARD_SETUP_DEMO` (set to anything) runs the window with steps that only pretend.
`SHARD_REDUCE_MOTION=1`, or the system's reduce-motion setting, keeps it still.

### The extension step

`install::browsers()` looks for browsers: app bundles in `/Applications` and `~/Applications` on
macOS, known paths under `%ProgramFiles%`, `%ProgramFiles(x86)%` and `%LOCALAPPDATA%` on Windows,
and commands on `PATH` on Linux. Each has a store: Chrome, Arc, Brave, Vivaldi, Opera and Chromium
use the Chrome Web Store; Edge uses Edge Add-ons, or the Chrome Web Store while Shard has no Edge
listing; Firefox uses Firefox Add-ons. A store's link is built from Shard's ID there, compiled in
from `SHARD_CHROME_STORE_ID`, `SHARD_EDGE_STORE_ID` or `SHARD_FIREFOX_STORE_SLUG`, or read from
the same variables at run time. Only browsers whose store has an ID are listed, and with none
there's no extension step.

For each browser picked, setup opens Shard's page in that store in that browser (`open -a` on
macOS, the browser's binary elsewhere), shows which buttons to press, and waits for the extension
to say hello. While the step is up, setup holds the endpoint's port itself (`browser::Hello`). It
answers a `POST /hello` carrying a `chrome-extension://` or `moz-extension://` `Origin` with
`200` and counts it; everything else gets `404 setup is running`, so the extension's `/add`
fails and the browser keeps its downloads meanwhile. A hello marks the current browser as
connected and moves on to the next. If the port is taken, setup can't listen: it says it can't
hear the browser, and the person moves on by hand. Choosing Open Shard at the end lets go of the
port before starting the installed copy.

### Removing

The steps: closing a running Shard, turning off start at login, on Windows removing the shortcuts
and the Settings › Apps entry (and the notification registration under
`HKCU\Software\Classes\AppUserModelId\dev.shard.app`), on Linux removing the desktop entry and
icons, deleting the data folder if asked, and removing the app. A running Windows program can't
delete itself, so there a hidden `cmd` waits a couple of seconds for Shard to exit, then deletes
`Shard.exe`, `Shard.exe.old` and the folder if it's empty.

## Persistence

`db.rs` opens `shard.db` in the data folder with SQLite (rusqlite, with SQLite compiled in) in WAL
mode. It has two tables.

`downloads`, one row per download:

| Column | Type | Meaning |
| --- | --- | --- |
| `id` | integer, primary key | |
| `url` | text | The link, replaced when it's refreshed. |
| `headers` | text | `Name: value` lines sent with every request: the browser's cookie, referrer and user agent. Emptied when the download finishes. |
| `dir` | text | The folder it goes in (including a category folder, once named). |
| `name` | text, null | The file name, chosen on the first run. |
| `total` | integer | Size in bytes; 0 when the server didn't say. |
| `done` | integer | Bytes downloaded, saved every second while running. |
| `status` | text | `queued`, `downloading`, `paused`, `done` or `failed`. |
| `resumable` | integer, null | Whether the server supports ranges; null until the first request. |
| `error` | text, null | Why it failed. |
| `added` | integer | When it was added, Unix seconds. |
| `position` | integer | Place in the queue; lower runs first. Databases from before queue order get it added and numbered by `id`. |

`settings`, key and value as text:

| Key | Default | Meaning |
| --- | --- | --- |
| `dir` | `~/Downloads` | Download folder. |
| `max_active` | `3` | Downloads at once (1 to 10). |
| `connections` | `8` | Connections per download (1 to 32). |
| `speed_limit` | `0` | Bytes per second for everything together; 0 is none. |
| `keep_awake` | `true` | Keep the computer from idle-sleeping while downloading. |
| `notify` | `true` | Notify when a download finishes or fails. |
| `ask_where` | `false` | Ask for a folder for each link added in the window. |
| `sort_into_folders` | `false` | Put files in Video, Music, Archives, Documents, Programs or Other. |
| `watch_clipboard` | `true` | Offer copied links to files. |
| `start_at`, `stop_at` | empty | Schedule times, `HH:MM`. |
| `when_done` | `nothing` | `nothing`, `sleep` or `shut down`. |
| `auto_update` | `true` | Check for and download updates. |
| `combine_networks` | `false` | Spread connections over every network. |
| `networks_off` | empty | Comma-separated interface names left out of combining. |

Launch at login isn't stored here: Shard reads and writes it in the system itself (a LaunchAgent
on macOS, an autostart entry on Linux, the `Run` registry key on Windows).

The byte-level resume state is not in the database. It's in `<file>.resume` next to each
download, described [above](#the-writer-and-checkpoints).

## Where files live

The data folder comes from `data_dir()` in `main.rs`; `SHARD_DATA` overrides it.

| System | Data folder |
| --- | --- |
| macOS | `~/Library/Application Support/shard` |
| Windows | `%APPDATA%\shard` |
| Linux | `$XDG_DATA_HOME/shard`, else `~/.local/share/shard` |

On macOS and Linux Shard sets the folder's permissions to `0700` at start, because the database
holds the cookies browser downloads need.

| What | Where |
| --- | --- |
| Downloads and settings | `<data>/shard.db`, with SQLite's `shard.db-wal` and `shard.db-shm` beside it |
| Update builds | `<data>/updates/<version>/` (and `unpacked/Shard.app` on macOS) |
| A download in progress | `<file>.part` and `<file>.resume` in the download's folder (`<file>.resume.tmp` while a checkpoint is written) |
| The previous version after an update | `Shard.app.old`, `Shard.exe.old` or `shard.old` beside the app, until the next start |
| Launch at login | `~/Library/LaunchAgents/dev.shard.app.plist` (macOS), `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `Shard` (Windows), `$XDG_CONFIG_HOME/autostart/shard.desktop` or `~/.config/autostart/shard.desktop` (Linux) |
| "Run without installing" chosen (Linux) | `<data>/run-in-place` |
| The installed app, shortcuts and entries | See [where it installs](#where-it-installs) |
