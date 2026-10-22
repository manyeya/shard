# Contributing to Shard

Thanks for wanting to help. This guide covers how to report problems, set up a development copy,
make a change and get it merged. Everyone taking part agrees to the
[code of conduct](CODE_OF_CONDUCT.md).

Shard is small and has one maintainer (see [GOVERNANCE.md](GOVERNANCE.md)), so a short issue
before a big change saves everyone time.

## Contents

- [Ways to contribute](#ways-to-contribute)
- [Development setup](#development-setup)
- [Build, run and test](#build-run-and-test)
- [Running a development copy beside an installed one](#running-a-development-copy-beside-an-installed-one)
- [Testing the browser extension](#testing-the-browser-extension)
- [Project layout](#project-layout)
- [Code style](#code-style)
- [Tests](#tests)
- [Commit messages](#commit-messages)
- [Branches and pull requests](#branches-and-pull-requests)
- [What CI checks](#what-ci-checks)
- [Releasing](#releasing)
- [Licensing of contributions](#licensing-of-contributions)

## Ways to contribute

- **Report a bug.** Use the [bug report form](https://github.com/manyeya/shard/issues/new/choose).
  Say which version, system and install method you use, and paste what Shard printed (see
  [where errors go](docs/TROUBLESHOOTING.md#where-errors-go)). Never paste a private or signed
  download link: describe the kind of link instead.
- **Suggest a feature.** Use the feature request form. Check the README's "Not yet" list first:
  torrents and downloads a page starts with a POST are known gaps.
- **Answer questions** in [Discussions](https://github.com/manyeya/shard/discussions).
- **Test on your setup.** Shard runs on macOS, Windows and Linux, and the parts that differ most
  are the tray (especially Linux desktops), launch at login, sleep and shut down, notifications
  and combining networks. Reports like "works on Fedora 41 with KDE, the tray panel opens the
  window" are useful.
- **Improve the docs or the website.** The docs are Markdown in this repository; the website is
  plain HTML, CSS and JavaScript in `site/`.
- **Fix a bug or build something.** Issues labelled `good first issue` or `help wanted` are a
  good start. For anything larger than a small fix, open an issue or discussion first so the
  approach can be agreed before you spend time on it.
- **Security problems** don't go in public issues. Follow [SECURITY.md](SECURITY.md).

## Development setup

You need Git and a recent stable Rust from [rustup](https://rustup.rs). The crate uses Rust
edition 2024 and recent standard library additions, and CI always builds with the latest stable
(`dtolnay/rust-toolchain@stable`), so keep yours current with `rustup update stable`. There is no
`rust-toolchain.toml` pinning a version.

Optional tools, for the scripts:

| Tool | Used by |
| --- | --- |
| `python3` | `scripts/fixture_server.py`, `scripts/logo.py`, `scripts/icons.sh`, `scripts/package-extension.sh`, `scripts/render-art.sh` |
| `zip` | `scripts/package-extension.sh` |
| Node.js (for `npx web-ext`) | Linting the Firefox build of the extension, as CI does |
| `sqlite3`, `curl`, [agent-browser](https://github.com/vercel-labs/agent-browser), Chrome for Testing | `scripts/test-extension.sh` |
| [agent-browser](https://github.com/vercel-labs/agent-browser) | `scripts/render-art.sh` |
| [create-dmg](https://github.com/create-dmg/create-dmg) (`brew install create-dmg`) | `scripts/make-dmg.sh` |
| fontTools (`pip install fonttools`) | `scripts/logo.py` |
| `sips`, `iconutil` (macOS only) | `scripts/icons.sh` |
| [actionlint](https://github.com/rhysd/actionlint) | Checking changes to `.github/workflows/`, as CI does |

### macOS

Install the Xcode Command Line Tools:

```sh
xcode-select --install
```

Full Xcode isn't needed: GPUI is built with its `runtime_shaders` feature, so no Metal compiler
runs at build time. The app bundle `scripts/bundle-macos.sh` makes asks for macOS 11 or later.

### Linux

On Debian or Ubuntu, install the libraries GPUI links against:

```sh
scripts/linux-deps.sh
```

It runs `sudo apt-get install` for `pkg-config`, `libxkbcommon-dev`, `libxkbcommon-x11-dev`,
`libxcb1-dev`, `libx11-xcb-dev`, `libwayland-dev`, `libfontconfig-dev` and `libfreetype-dev`. It
assumes a C compiler is already there (the bundled SQLite and `aws-lc-sys`, rustls' crypto, are
compiled from C); on a fresh system install `build-essential` too.

On other distributions, install the same libraries under their own names. On Fedora that should
be roughly `gcc pkgconf-pkg-config libxkbcommon-devel libxkbcommon-x11-devel libxcb-devel
libX11-devel wayland-devel fontconfig-devel freetype-devel`; CI doesn't test this list.

To run Shard you also need the Vulkan loader (`libvulkan1` on Debian and Ubuntu) and the Wayland
client library, which GPUI loads at run time, and a desktop with a StatusNotifierItem host for
the tray icon (see [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md#linux-no-tray-icon)).

### Windows

Install Rust with the default MSVC toolchain, and the Visual Studio Build Tools with the
"Desktop development with C++" workload. `build.rs` embeds the icon into `shard.exe` with
`winresource`, which uses the Windows SDK's resource compiler from those tools.

Set `AWS_LC_SYS_PREBUILT_NASM=1` before building. `aws-lc-sys` (rustls' crypto) would otherwise
need NASM to assemble its code; with this set it uses the objects it ships. CI does the same.

```powershell
$env:AWS_LC_SYS_PREBUILT_NASM = "1"     # this shell only
setx AWS_LC_SYS_PREBUILT_NASM 1         # new shells from now on
```

## Build, run and test

```sh
cargo build                                   # debug build
cargo build --release                         # what the releases ship
cargo run --release                           # open the download manager
cargo run --release -- <url>                  # queue a link (in the Shard that's already open, if there is one)
cargo run --release -- get <url> [output]     # download in the terminal; run it again to resume
cargo run --release -- --hidden               # start in the tray (what launch at login does)
cargo run --release -- serve                  # the queue and the browser endpoint, no window
SHARD_SETUP_DEMO=1 cargo run -- --setup      # the setup window, with pretend steps
scripts/bundle-macos.sh                       # macOS: target/release/Shard.app
cargo test                                    # every unit test
cargo test --locked                           # what CI runs: fails if Cargo.lock is out of date
cargo test engine::                           # one module's tests
cargo test pauses_and_resumes_without_refetching   # one test
cargo clippy --all-targets --locked -- -D warnings # what CI runs: fails on any warning
scripts/test-setup.sh target/debug/shard      # Linux: install, reinstall and remove, in a scratch home
```

`shard --help` isn't a command: anything Shard doesn't recognise prints the usage and exits
with status 2.

The tests start their own HTTP servers on `127.0.0.1` and work in scratch folders under the
system's temporary directory (`shard-test-<pid>-<name>`). They need no internet connection and
don't touch your real data folder or downloads. On macOS one test checks `pmset -g assertions`
to see that keep awake holds and releases the Mac.

More detail, including release builds, every package, the setup checks on macOS and Windows,
and the setup window's demo, is in [docs/BUILDING.md](docs/BUILDING.md).

## Running a development copy beside an installed one

Only one Shard runs per user: the browser endpoint's port, `127.0.0.1:47110`, doubles as the
lock. A second `shard` that finds a running one hands its link over (or asks it to show its
window) and exits. So a development build started while your installed Shard runs does nothing
visible, unless you move it:

```sh
SHARD_PORT=47111 SHARD_DATA="$PWD/target/dev-data" SHARD_NO_UPDATE=1 cargo run --release
```

```powershell
$env:SHARD_PORT = "47111"; $env:SHARD_DATA = "$PWD\target\dev-data"; $env:SHARD_NO_UPDATE = "1"
cargo run --release
```

| Variable | What it does |
| --- | --- |
| `SHARD_PORT` | The port for the browser endpoint and the one-copy lock (default 47110). |
| `SHARD_DATA` | The folder for `shard.db` (downloads and settings) and downloaded updates. |
| `SHARD_NO_UPDATE` | Any value: never check for updates, not even from "Check now". Without it, a development build on Windows or Linux sitting in a writable folder can download a newer release and offer to replace itself. |
| `SHARD_NO_FOCUS` | `1`: open the window without activating the app or taking focus. |
| `SHARD_NO_SETUP` | Any value: don't offer to install this copy. Builds under a `target` folder don't offer it anyway, unless their file name contains "setup". |
| `SHARD_INSTALL_DIR` | Where setup installs Shard instead of the default. |
| `SHARD_SETUP_DEMO` | Any value: run the setup window with pretend steps that change nothing. |
| `SHARD_REDUCE_MOTION` | `1`: keep the setup window still; any other value: animate it even when the system asks for reduced motion. |
| `SHARD_CHROME_STORE_ID`, `SHARD_EDGE_STORE_ID`, `SHARD_FIREFOX_STORE_SLUG` | The extension's IDs in each browser store, which setup's extension step links to. Compiled in when set at build time (the release workflow sets them from the repository's variables), and read at run time too, which wins. A store without one isn't offered. |
| `SHARD_RESTARTED` | Internal: set by the updater when it starts the new version, which then waits for the old one to free the port. Don't set it yourself. |

Things to know:

- **The extension only talks to port 47110.** `extension/background.js` has the address built
  in, so a development copy on another port never gets browser downloads. To test the extension
  against your build, quit the installed Shard (tray menu › Quit Shard) and run the build on the
  default port.
- **Launch at login writes the real entry.** Turning it on in a development copy writes
  `~/Library/LaunchAgents/dev.shard.app.plist` (macOS), `~/.config/autostart/shard.desktop`
  (Linux, or under `$XDG_CONFIG_HOME`) or the `Shard` value under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (Windows), pointing at the development
  binary. These are shared with your installed Shard. Turn it off again before you delete the
  build.
- **macOS notifications need an app bundle.** GPUI turns system notifications off when the
  binary isn't inside one, which is how `cargo run` runs it. Copied-link offers arrive as
  notifications too. To try them, bundle the build and run the binary inside the bundle:

  ```sh
  scripts/bundle-macos.sh
  SHARD_PORT=47111 SHARD_DATA="$PWD/target/dev-data" target/release/Shard.app/Contents/MacOS/shard
  ```

## Testing the browser extension

People get the extension from the Chrome Web Store, Edge Add-ons or Firefox Add-ons, which setup
opens for them ([docs/STORES.md](docs/STORES.md) is how it gets there). To try a change of your
own, load it straight from the checkout instead. Remove or turn off the store's copy first, so
only one of them takes each download.

### By hand, in a Chromium browser

1. Open `chrome://extensions` (or `edge://extensions`, `brave://extensions`, `arc://extensions`)
   and turn on Developer mode.
2. Choose Load unpacked and pick the repository's `extension` folder.
3. With Shard running on the default port, start any download in the browser. It should
   disappear from the browser's download list and appear in Shard's queue.
4. After editing `background.js`, press the reload button on the extension's card.

### By hand, in Firefox

Firefox runs the Firefox build, which `scripts/package-extension.sh` makes from the same source:

```sh
scripts/package-extension.sh 0.0.0 build
mkdir -p build/firefox && (cd build/firefox && unzip -o -q ../shard-extension-0.0.0-firefox.zip)
npx --yes web-ext@8 lint --source-dir build/firefox --warnings-as-errors   # what CI checks
```

Then open `about:debugging`, choose This Firefox, then Load Temporary Add-on, and pick
`build/firefox/manifest.json`. A temporary add-on lasts until Firefox quits; after editing
`background.js`, package it again and press Reload.

### What it does

The extension inspects each new download. It skips anything that isn't `http:` or `https:` (blob
and data links only exist inside the page) and anything not in progress (history being
restored). For the rest it reads the cookies for the URL and posts the URL, cookies, referrer
and user agent to `POST /add`. Only when Shard answers 200 does it cancel and erase the browser's
own download; if Shard isn't running or refuses, the browser downloads as usual. When it's
installed or updated (loading it unpacked counts), it posts an empty `POST /hello`, which setup
waits for.

To try setup's extension step against your browsers without changing anything, run
`SHARD_SETUP_DEMO=1 cargo run -- --setup`: the demo lists every browser it finds and pretends the
extension connects.

### End to end

`scripts/test-extension.sh` drives a real browser:

```sh
CHROME=/path/to/chrome-for-testing scripts/test-extension.sh
```

It needs `CHROME` pointing at a Chrome for Testing (or Chromium) binary, because branded Chrome
ignores unpacked extensions when automated. One way to get it is
`npx @puppeteer/browsers install chrome@stable`, which prints where it put the binary. It also
needs the `agent-browser` CLI, `python3`, `sqlite3` and `curl`.

The script builds a release binary, makes a scratch data folder whose download folder is a
scratch folder, and starts `scripts/fixture_server.py` on `127.0.0.1:8765`: a page that sets a
session cookie and a 4 MB file (with Range support and a `Content-Disposition` name) that is
only served with that cookie. It starts `shard serve` on the default port, clicks the link in
headless Chrome with the extension loaded, then checks that the file arrived intact and that
Shard sent the browser's cookie.

Quit any Shard you have running first. `shard serve` uses port 47110, and if a Shard already
answers there, the new one exits and the test ends up talking to your real one.

## Project layout

| Path | What's there |
| --- | --- |
| `src/main.rs` | Command line parsing, the one-copy lock, the data folder, and starting the runtime, queue, browser endpoint, updater and UI. |
| `src/engine.rs` | One download: the probe, the worker pool, part splitting, 429/503 backoff and regrowth, the writer and checkpoints (`<file>.part`, `<file>.resume`), file names. Also `shard get`. |
| `src/queue.rs` | `Manager`: every download, which ones run, naming and folders, pause and resume, the schedule, settings, combining networks, keeping awake, and the numbers the UI shows. |
| `src/db.rs` | SQLite: the `downloads` and `settings` tables. |
| `src/browser.rs` | The HTTP endpoint on `127.0.0.1:47110` for the extension, a second `shard` launch and setup; the client side they use; and the listener setup holds while it waits for the extension's hello. |
| `src/ui.rs` | The GPUI window, the tray panel, notifications, copied-link offers and the when-done countdown. |
| `src/tray.rs` | The menu bar or tray icon, drawn from the logo's outline, and its menu. |
| `src/system.rs` | Keeping the computer awake, launch at login, sleep and shut down. |
| `src/networks.rs` | The computer's network connections, for combining them. |
| `src/update.rs` | Checking for, verifying, downloading and installing new versions. |
| `src/logo.rs` | The logo's outline as points. Generated by `scripts/logo.py`; don't edit it by hand. |
| `src/install.rs` | Setup's steps: where Shard goes on each system, how it's removed, the browsers it finds and the store each one uses. |
| `src/setup.rs` | The setup window that runs those steps and walks through adding the extension. |
| `build.rs` | Embeds the icon in `shard.exe` on Windows. |
| `extension/` | The Manifest V3 browser extension: one source, packaged for Chromium browsers and for Firefox by `scripts/package-extension.sh`. |
| `assets/fonts/` | Geist, Geist Mono and Archivo Expanded, compiled into the binary, with their licences. |
| `assets/logo/`, `assets/icons/` | The logo and every platform's icons, generated by `scripts/logo.py` and `scripts/icons.sh`. |
| `packaging/linux/` | The `.desktop` entry. |
| `packaging/art/` | `art.html`, which `scripts/render-art.sh` renders into the disk image's background and the store images. |
| `packaging/macos/` | The disk image's background, at 1x and 2x. |
| `packaging/stores/` | The store screenshots and promo tiles, GitHub's social preview, and the listing text (`LISTING.md`). |
| `scripts/` | Build, packaging, test and asset scripts (see [docs/BUILDING.md](docs/BUILDING.md)). |
| `site/` | The website, static files published to GitHub Pages. |
| `.github/workflows/` | CI, releases, the website, dependency audit and CodeQL. |
| `docs/` | Architecture, building, releasing, the browser stores, privacy and troubleshooting. |

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains how these fit together.

## Code style

**Formatting.** The code isn't run through `rustfmt` today: lines run to about 140 characters,
and `cargo fmt --check` reports differences in every file. CI doesn't check formatting. Match the
code around your change, and don't run `cargo fmt` over whole files, because it would rewrite
lines you didn't touch and bury your change in the diff. If the project adopts a `rustfmt.toml`,
this section will change.

**Lints.** Run `cargo clippy --all-targets --locked -- -D warnings` before you push. CI runs
exactly that on macOS, Linux and Windows and fails on any warning, so code that only builds on
one system (behind `cfg`) is checked there too.

**Comments.** Say why, not what. The code already says what it does; a comment earns its place
by explaining a reason, a constraint or a trap:

```rust
/// HTTP/1.1 only: over HTTP/2 the connections would all be streams on one TCP connection, which
/// is the throttle parallel parts exist to get around.
```

- Every module starts with a `//!` paragraph saying what it is for.
- Doc comments (`///`) are short: what an item is or what a value means, in a sentence or two
  ("Parts are never split below this.").
- Every `unsafe` block has a `// SAFETY:` comment.
- A deliberate shortcut is marked `ponytail:` with what it doesn't handle, so it can be found
  later (`// ponytail: Retry-After in seconds only; the HTTP-date form falls back to backoff.`).
- Comments and messages use UK spelling: "colour", "rasterises", "honour".

**Messages.** Text people read is a full, plain sentence: "That isn't a valid link.", "Only http
and https links can be downloaded." Messages printed to stderr start with `shard: `. Network
errors leave the URL out, because signed links run to hundreds of characters.

**Structure.**

- The UI talks to the queue only through `Manager`'s public methods, and those stay cheap and
  synchronous so the UI thread can call them directly. Downloads run as tasks on the Tokio
  runtime.
- Platform differences live in one place each, with every platform's branch side by side
  (`cfg!(target_os = ...)` or `#[cfg(...)]`), as in `system.rs` and `update.rs`.
- Prefer the standard library and the dependencies already in `Cargo.toml`. A new dependency's
  licence has to be on the allow list in `deny.toml`.
- Don't edit generated files by hand: `src/logo.rs`, `assets/logo/*.svg`, `assets/icons/` and
  `extension/icons/`. Change `scripts/logo.py`, run it, then
  `CHROME=<Chrome for Testing binary> scripts/icons.sh` (macOS only, it uses `sips` and
  `iconutil`), and commit what they write.

## Tests

Unit tests sit next to the code they test, in a `#[cfg(test)] mod tests` at the bottom of the
file. Write a test for each behaviour you add or bug you fix.

- Name tests as sentences about behaviour: `pauses_and_resumes_without_refetching`,
  `only_the_extension_can_add`, `a_signed_manifest_is_read_and_a_tampered_one_refused`.
- Use the shared helpers in `engine::tests`: `serve(body, max_conns)` runs a local server that
  honours Range, answers 429 past a connection limit, and has a configurable delay and ETag;
  `body()` makes a 3 MB test file; `scratch(name)` makes a fresh temporary folder.
  `queue::tests::wait_until` polls a `Manager` until its rows match.
- Don't reach the internet. Everything runs against `127.0.0.1`.
- Waits that would be long in real use are shortened under `#[cfg(test)]` (`REGROW_AFTER`,
  `REGROW_TICK` in `engine.rs`); do the same rather than sleeping in tests.
- Clean up scratch folders best effort (`let _ = std::fs::remove_dir_all(dir);`): Windows keeps
  an open database locked.
- Gate platform-specific tests, as `system.rs` does with `#[cfg(all(test, not(windows)))]`.
- Check your change on the systems it affects. CI runs the tests on macOS, Linux and Windows,
  and installs and removes Shard with `--setup --silent` and `--uninstall --silent`, but it can't
  click through the window, the setup window, the tray or the extension; say in the pull request
  what you tried by hand.

## Commit messages

Look at `git log --oneline` for the house style:

```
Engine: connections grow back after a server's 429/503 passes
Queue: refresh links, choose folders, order the queue, and a scheduler
Tray: the template-icon calls only exist on macOS
Tests: clean up best-effort, since Windows keeps the open database locked
Site: an instrument-panel landing page, published to GitHub Pages
```

- Start with the area you changed and a colon: `Engine:`, `Queue:`, `UI:`, `Tray:`, `Site:`,
  `README:`, `Tests:`, or a feature's name (`Auto-update:`, `Combine networks:`).
- Then say what changed, in plain words, as a phrase that starts in lower case. No full stop at
  the end. Keep the line under about 72 characters.
- In the body, wrapped at about 72 to 78 characters, explain what changed and why: what someone
  would notice, and the reason for the approach when it isn't obvious.
- One logical change per commit.

## Branches and pull requests

- `dev` is where work lands. `main` is fast-forwarded to `dev` only after CI is green on macOS,
  Linux and Windows. Releases are tagged on `main`, and the website deploys from `main`.
- Fork the repository, create a branch from `dev`, and open your pull request against `dev`.
  If you open one against `main` by mistake, switch its base branch to `dev`.
- Keep a pull request to one topic. Fill in the template: what changed, why, and how you tested
  it on which systems.
- If the change is something users will notice, add a line under "Unreleased" in
  [CHANGELOG.md](CHANGELOG.md), and update the README, `docs/` or `site/` where they describe it.
- Keep your branch up to date by rebasing onto `dev` rather than merging `dev` into it.
- Labels on pull requests decide which section of the generated release notes they appear in
  (`.github/release.yml`). The maintainer sets them; add `skip-changelog` to leave one out.

## What CI checks

| Workflow | When | What |
| --- | --- | --- |
| `ci.yml`, job `test` | Every push to `main` or `dev`, every pull request | On `macos-14`, `ubuntu-22.04` (after `scripts/linux-deps.sh`) and `windows-2022` (with `AWS_LC_SYS_PREBUILT_NASM=1`): `cargo test --locked`, `cargo clippy --all-targets --locked -- -D warnings`, a debug build, and then setup for real: `scripts/test-setup.sh` (macOS on a bundle of the debug build, Linux on the binary) or `scripts/test-setup.ps1` (Windows, on the binary copied to `Shard-Setup-test.exe`) installs Shard, installs it again over the top, and removes it, checking what's where each time. `--locked` fails if `Cargo.lock` needs changing, so commit it with any dependency change. |
| `ci.yml`, job `extension` | The same | Packages the extension with `scripts/package-extension.sh` and lints the Firefox build with `web-ext lint --warnings-as-errors`. |
| `ci.yml`, job `workflows` | The same | Checks every workflow file with actionlint 1.7.12. |
| `audit.yml` | Pull requests, pushes to `main` or `dev` that touch `Cargo.toml`, `Cargo.lock`, `deny.toml` or the workflow, weekly, and by hand | `cargo deny` against `deny.toml`: known vulnerabilities, licences, duplicate or banned crates, and where crates come from. A newly published advisory doesn't fail an unrelated pull request; the weekly run reports it. |
| `codeql.yml` | Pushes to `main` or `dev`, pull requests, weekly | CodeQL analysis of the Rust code and of the workflows themselves. |
| `release.yml` | Tags `vX.Y.Z`, or run by hand | Builds and checks every package; publishes a release, updates the package managers and sends the extension to the stores only for a tag. See [docs/RELEASING.md](docs/RELEASING.md). |
| `pages.yml` | Pushes to `main` that change `site/` | Publishes the website. |

CI doesn't check formatting or run the extension's end-to-end test (it needs Chrome for Testing
and agent-browser). Run what applies to your change yourself.

## Releasing

Only the maintainer releases. The checklist, the signing key, the secrets and variables, and the
package managers are in [docs/RELEASING.md](docs/RELEASING.md); getting the extension into the
browser stores is in [docs/STORES.md](docs/STORES.md).

## Licensing of contributions

Shard is dual licensed under the [MIT License](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option.

The fonts in `assets/fonts/` (and their copies in `site/fonts/`) are third-party work under the
SIL Open Font License 1.1; their licences are `OFL-Geist.txt` and `OFL-Archivo.txt` beside them.
Don't add fonts, images or code from elsewhere unless their licence allows it, and include the
licence text with them.

There is no contributor licence agreement, and you don't need to sign off your commits (no DCO).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
