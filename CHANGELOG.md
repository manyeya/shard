# Changelog

All notable changes to Shard are written down here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Shard
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Every release is a tag
`vX.Y.Z` on `main` and a [GitHub release](https://github.com/manyeya/shard/releases).

## [Unreleased]

### Added

- **Setup, built into the app.** The `shard` binary carries its own animated setup window. A
  copy that isn't installed offers to install itself when it's opened, `--setup` asks for it, and
  `--silent` does the same with the defaults and no window. It closes a running Shard first
  (downloads carry on where they left off), copies the new one in beside the old and swaps them,
  so a failed copy leaves the old one working, and can start Shard at login.
  - Windows: `Shard-Setup-X.Y.Z-windows-x64.exe` installs for the current user into
    `%LOCALAPPDATA%\Programs\Shard` without administrator rights, adds a Start menu shortcut
    (and a desktop one if you ask), and registers an uninstaller under Settings › Apps.
  - macOS: open the disk image and either drag Shard to Applications or open it there. Opened
    from outside Applications, Shard offers to install itself into `/Applications` (or
    `~/Applications` when that isn't writable).
  - Linux: the `.deb` and `.rpm` install system-wide. The AppImage and the tarball's binary offer
    setup instead: the binary into `~/.local/bin`, a desktop entry and icons under
    `~/.local/share`, or "Run without installing".
  - The last step finds the browsers on the computer, opens Shard's page in each one's store
    (the Chrome Web Store for Chrome, Arc, Brave, Vivaldi, Opera and Chromium; Edge Add-ons for
    Edge, or the Chrome Web Store until Edge has a listing; Firefox Add-ons for Firefox), and
    waits for the extension to say hello, so it can show it's connected. A store is offered once
    Shard's ID there is known.
- `shard --uninstall` removes Shard (`--silent` for no window), optionally with its history and
  settings. On Windows, Settings › Apps runs it.
- `shard --version` prints `shard X.Y.Z`.
- `SHARD_NO_SETUP` stops a copy offering setup, `SHARD_INSTALL_DIR` changes where setup installs,
  `SHARD_SETUP_DEMO` runs the setup window with pretend steps, and `SHARD_REDUCE_MOTION=1` keeps
  it still (as the system's reduce-motion setting does).
- The local endpoint answers `POST /hello` (the extension announcing itself when it's installed
  or updated) and `POST /quit` (setup asking a running Shard to quit before replacing or removing
  it). Both refuse web pages, like the rest.
- **The extension for Firefox** (128 or later), built from the same source as the Chromium one,
  and published to the browsers' stores: the Chrome Web Store, Edge Add-ons and Firefox Add-ons.
  It says hello to Shard when it's installed, for setup.
- **New release files.** `Shard-Setup-X.Y.Z-windows-x64.exe`; an AppImage and an `.rpm` for Linux
  x64 and arm64, beside the `.deb` and the tarball; a disk image with a background and the
  Applications link laid out by Finder (made with create-dmg); the extension as
  `shard-extension-X.Y.Z-chrome.zip` and `shard-extension-X.Y.Z-firefox.zip`. The Windows zip
  and the Linux tarballs carry both licence files, and the `.deb` and `.rpm` install them.
- **Package managers.** A Homebrew cask (`brew install --cask manyeya/tap/shard`) and a Scoop
  manifest (`scoop bucket add manyeya https://github.com/manyeya/scoop-bucket`, then
  `scoop install shard`), updated by the release workflow, and winget manifests (`manyeya.Shard`)
  written with each release for submitting to winget-pkgs.
- **Store publishing.** For each tagged release the workflow sends the extension to the Chrome
  Web Store, Edge Add-ons and Firefox Add-ons, each once its credentials are set, and builds carry
  the store IDs from the repository's variables so setup can link to them.
- **Checks.** CI runs Clippy with warnings as errors, and installs, reinstalls and removes Shard
  with setup for real on macOS, Linux and Windows; it packages the extension and lints the Firefox
  build with web-ext, and checks the workflows with actionlint. Each release build runs the same
  setup check before its files are uploaded. `cargo deny` checks dependencies for advisories,
  licences and sources, CodeQL analyses the Rust code and the workflows, and Dependabot proposes
  updates weekly.
- **Docs and project files.** Dual MIT or Apache-2.0 licence files, with the licence and
  repository details in `Cargo.toml`; a contributing guide, code of conduct, security policy,
  support and governance pages and this changelog; docs on architecture, building, releasing,
  getting the extension into the stores, privacy and troubleshooting; the store listing text and
  artwork (`packaging/stores/`, rendered by `scripts/render-art.sh`); a privacy page on the
  website; issue and pull request templates.

### Changed

- The extension comes from the browsers' stores, which setup opens, instead of being loaded
  unpacked from a zip.
- Release maintainers: the Chrome Web Store item is now the repository variable
  `CHROME_STORE_ID`; the `CWS_EXTENSION_ID` secret isn't read any more.

### Fixed

- The Linux update file `shard-X.Y.Z-linux-<arch>` is the bare binary, which installed copies
  update from; the release workflow had been uploading a folder under that name.

### Security

- rustls 0.23.46 for RUSTSEC-2026-0346, RUSTSEC-2026-0347, RUSTSEC-2026-0348 and
  RUSTSEC-2026-0349, and rustls-webpki 0.103.16 for RUSTSEC-2026-0382.

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
