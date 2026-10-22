# Changelog

All notable changes to Shard are written down here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Shard
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Every release is a tag
`vX.Y.Z` on `main` and a [GitHub release](https://github.com/manyeya/shard/releases).

## [Unreleased]

### Added

- Setup built into the app. The `shard` binary carries its own animated setup window; a copy that
  isn't installed offers to install itself, and `--setup` asks for it.
  - Windows: `Shard-Setup-X.Y.Z-windows-x64.exe` installs for the current user into
    `%LOCALAPPDATA%\Programs\Shard` without administrator rights, adds a Start menu shortcut
    (and a desktop one if you ask), registers an uninstaller under Settings › Apps, can start
    Shard at login, and walks you through adding the browser extension.
    `Shard-Setup-X.Y.Z-windows-x64.exe --silent` installs with the defaults and no window.
  - macOS: open the disk image and either drag Shard to Applications or double-click it. Opened
    from outside Applications, Shard offers to install itself into `/Applications` (or
    `~/Applications` when that isn't writable), start at login and add the browser extension.
  - Linux: the `.deb` and `.rpm` install system-wide. The tarball and AppImage binaries offer the
    same setup: the binary into `~/.local/bin`, a desktop entry and icons under
    `~/.local/share`, or "Run without installing".
  - The extension step opens Shard's page in each browser's own store (Chrome Web Store, Edge
    Add-ons or Firefox Add-ons) for every browser it finds, and waits for the extension to say
    hello when it's added, so it can confirm it's connected. It's offered only for stores where
    Shard is listed; until then, the extension is added by hand from the extension zip.
- `shard --uninstall` removes Shard (`--silent` for no window), optionally with its history and
  settings.
- `shard --version` prints `shard X.Y.Z`.
- `SHARD_NO_SETUP=1` stops a copy offering setup; `SHARD_INSTALL_DIR` changes where setup
  installs.
- The local endpoint gains `POST /hello` (the extension announcing itself) and `POST /quit`
  (setup asking a running Shard to quit before replacing it).
- New release packages: the Windows Setup `.exe` beside the portable zip; `.rpm` and AppImage
  beside the `.deb` and tarball for Linux x64 and arm64.
- A Homebrew cask (`brew install --cask manyeya/tap/shard`) and a Scoop manifest
  (`scoop bucket add manyeya https://github.com/manyeya/scoop-bucket`, then
  `scoop install shard`), updated by the release workflow.
- The repository's community and project files: dual MIT or Apache-2.0 licence files,
  contributing guide, code of conduct, security policy, support and governance pages, this
  changelog, architecture, building, releasing, privacy and troubleshooting docs, issue and pull
  request templates, Dependabot, a `cargo deny` audit and CodeQL analysis.

## [0.1.0]

The first release. It hasn't been tagged yet; the date goes here when `v0.1.0` is.

### Added

- **Segmented downloads.** Each file comes down over up to 8 connections (1 to 32 in the Limits
  panel). A connection that finishes its part takes the back half of the part with the most
  left, so none sits idle while a slow one finishes; parts are never split below 256 KiB.
  Connections use HTTP/1.1, so each is its own TCP connection.
- **Pause, resume and crash recovery.** `<file>.part` holds the bytes and `<file>.resume` how far
  each part got. The checkpoint is written only after the data it describes is synced, so after
  a crash or `kill -9` a download resumes from at most a second back. If the file changed on the
  server (by ETag or Last-Modified), Shard says so instead of splicing two versions together.
  Downloads that were running when Shard quit carry on when it starts again.
- **Backing off when a server says no.** A server that answers 429 or 503 to one connection too
  many loses that connection and the rest carry on. Half a minute later Shard tries one more,
  doubling the wait (up to four minutes) each time it's refused again. `Retry-After` in seconds is
  honoured. 404s and other client errors fail at once instead of retrying.
- **Refresh an expired link.** Paste a fresh link to the same file; if it's the same size, the
  download keeps what it has, even when the new server tags the file differently.
- **Speed limit.** One budget shared by every connection of every download.
- **The queue.** Up to 3 downloads at once (1 to 10); the rest wait, in an order you can change
  (move to top, up, down, to bottom). Search the list, filter it by status or kind of file, and
  sort it by newest, oldest, name, size, progress or queue order.
- **Schedule.** A start and a stop time for the queue, and what to do when it empties: nothing,
  sleep or shut down, after a minute's warning you can cancel.
- **Where files go.** The Downloads folder, a folder you choose, asked each time a link is added
  in the window, or sorted into Video, Music, Archives, Documents, Programs and Other folders.
- **File names** from `Content-Disposition`, else the URL, cleaned of path tricks and characters
  Windows refuses; `name (1).ext` when the name is taken.
- **Browser extension** for Chrome, Edge, Brave and Arc. It hands new downloads to Shard with the
  page's cookies, referrer and user agent, so downloads behind a login work; when Shard isn't
  running, the browser downloads as usual. Shard listens on `127.0.0.1:47110` and refuses
  requests carrying a web page's Origin.
- **Tray and menu bar.** Closing the window keeps Shard running. The icon, the sliced S, fills
  from the bottom with progress, with the speed beside it on macOS. A click opens a panel with
  the active downloads, a throughput chart and quick controls; the menu pauses or resumes
  everything, sets a speed cap, toggles keep awake and launch at login, and quits. Launching
  Shard again brings the window back.
- **Launch at login**, starting hidden in the tray.
- **Keeps the computer awake** while anything downloads (on by default). The display can still
  sleep, and closing a laptop's lid still sleeps it.
- **Notifications** when a download finishes or fails, and an offer to download a link to a file
  when you copy one.
- **Combining networks.** Optional: connections are bound to each network in turn (Wi-Fi,
  Ethernet, a phone over USB), so their speeds add up. Networks can be left out one by one.
- **Signed auto-update.** Every six hours Shard reads a signed list of releases (`update.txt`,
  Ed25519), downloads the build for this computer in the background through its own engine,
  checks its SHA-256 and offers "Restart to update". It can be turned off, and "Check now" checks
  on demand.
- **The window.** Total speed, a live graph of the connections, a throughput chart, a segment map
  of each download's parts filling and splitting, a detail panel, and a stats view.
- **Command line.** `shard <url>` queues a link in the running Shard; `shard get <url> [output]`
  downloads in the terminal and resumes when run again; `shard --hidden` starts in the tray;
  `shard serve` runs the queue and the browser endpoint without a window.
- **Releases** for macOS (a universal disk image), Windows (x64 zip), Linux (x64 and arm64
  tarballs and `.deb` packages) and the extension, with `SHA256SUMS`.
- **The website**, <https://manyeya.github.io/shard/>.

[Unreleased]: https://github.com/manyeya/shard/compare/v0.1.0...dev
[0.1.0]: https://github.com/manyeya/shard/releases/tag/v0.1.0
