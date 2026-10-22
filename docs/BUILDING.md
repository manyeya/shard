# Building Shard from source

This covers building and running Shard on each system, making the same packages the release
workflow makes, and running the tests and checks. For setting up to contribute, see
[CONTRIBUTING.md](../CONTRIBUTING.md); for how the code fits together, see
[ARCHITECTURE.md](ARCHITECTURE.md); for publishing, see [RELEASING.md](RELEASING.md).

## Contents

- [What you need](#what-you-need)
- [macOS](#macos)
- [Linux](#linux)
- [Windows](#windows)
- [Release builds](#release-builds)
- [The macOS app bundle](#the-macos-app-bundle)
- [A universal macOS binary](#a-universal-macos-binary)
- [The disk image](#the-disk-image)
- [The Windows installer and zip](#the-windows-installer-and-zip)
- [A Debian package](#a-debian-package)
- [An RPM package](#an-rpm-package)
- [The AppImage](#the-appimage)
- [The Linux tarball](#the-linux-tarball)
- [The extension packages](#the-extension-packages)
- [The package-manager manifests](#the-package-manager-manifests)
- [The setup window](#the-setup-window)
- [Tests and checks](#tests-and-checks)
- [The fixture server](#the-fixture-server)
- [Icons and the logo](#icons-and-the-logo)
- [The artwork](#the-artwork)

## What you need

- Git.
- The latest stable Rust, from [rustup](https://rustup.rs). The crate is edition 2024 and uses
  recent standard library additions; CI builds with whatever stable is current, and there's no
  minimum version pinned, so update with `rustup update stable` if a build fails on an older one.
- A C compiler: SQLite is compiled in (`rusqlite` with `bundled`), and `aws-lc-sys` (the crypto
  behind rustls) is C as well.
- The platform pieces below.

Get the code:

```sh
git clone https://github.com/manyeya/shard.git
cd shard
```

Builds are `--locked` in CI and in the release workflow, so they use exactly the versions in
`Cargo.lock`. Do the same if you want the build CI tested.

## macOS

```sh
xcode-select --install          # the Command Line Tools: the C compiler, linker and lipo
cargo build --release --locked
./target/release/shard
```

Full Xcode isn't needed. GPUI is built with its `runtime_shaders` feature, so no Metal shader
compiler runs at build time. The bundle the scripts make asks for macOS 11 or later
(`LSMinimumSystemVersion`); CI and releases build on `macos-14` runners.

A bare binary runs, but system notifications (and the copied-link offers that use them) only
work from inside an app bundle; see [the macOS app bundle](#the-macos-app-bundle).

## Linux

On Debian or Ubuntu:

```sh
sudo apt-get install -y build-essential   # if you don't have a C compiler yet
scripts/linux-deps.sh                     # the libraries GPUI links against
cargo build --release --locked
./target/release/shard
```

`scripts/linux-deps.sh` runs `apt-get update` and installs `pkg-config`, `libxkbcommon-dev`,
`libxkbcommon-x11-dev`, `libxcb1-dev`, `libx11-xcb-dev`, `libwayland-dev`, `libfontconfig-dev`
and `libfreetype-dev` with `sudo`.

Other distributions need the same libraries under their own names. On Fedora that should be
roughly:

```sh
sudo dnf install gcc pkgconf-pkg-config libxkbcommon-devel libxkbcommon-x11-devel libxcb-devel \
    libX11-devel wayland-devel fontconfig-devel freetype-devel
```

CI doesn't test that list.

To run, GPUI also loads the Wayland client library and the Vulkan loader at run time, which is
why the `.deb` depends on `libwayland-client0` and `libvulkan1` (and the `.rpm` on
`libwayland-client` and `vulkan-loader`). You need a Vulkan driver for your GPU, and a
StatusNotifierItem host for the tray icon (KDE Plasma has one; GNOME needs the AppIndicator
extension).

Releases are built on Ubuntu 22.04, so the release binaries run on distributions with glibc 2.35
or later. A binary you build runs on systems with your build machine's glibc or newer.

## Windows

1. Install the Visual Studio Build Tools with the "Desktop development with C++" workload. That
   gives the MSVC compiler and linker, and the Windows SDK, whose resource compiler `build.rs`
   uses (through `winresource`) to embed `assets/icons/shard.ico` in `shard.exe`.
2. Install Rust with rustup, keeping the default `x86_64-pc-windows-msvc` toolchain.
3. Build, with `AWS_LC_SYS_PREBUILT_NASM` set so `aws-lc-sys` uses the assembled objects it ships
   instead of needing NASM:

```powershell
$env:AWS_LC_SYS_PREBUILT_NASM = "1"
cargo build --release --locked
.\target\release\shard.exe
```

`shard.exe` is a windowed program (`windows_subsystem = "windows"`), so it opens no console. When
it's started from a terminal with an argument (`shard.exe serve`, `shard.exe get <url>`,
`shard.exe --setup --silent`, a link) it attaches to that terminal, so its messages appear there.

Releases build only `x86_64` for Windows.

## Release builds

`cargo build --release --locked` puts the binary at `target/release/shard` (`shard.exe` on
Windows). That one file is the whole app, setup included: the fonts and icons it draws are
compiled in. There's no custom release profile in `Cargo.toml`, so it's Cargo's default `release`
profile.

The version comes from `Cargo.toml`. The release workflow refuses a tag that doesn't match it.

The browser stores setup links to are compiled in from the environment variables
`SHARD_CHROME_STORE_ID`, `SHARD_EDGE_STORE_ID` and `SHARD_FIREFOX_STORE_SLUG` when they're set at
build time, as the release workflow sets them; a build without them has no extension step in
setup unless the same variables are set when it runs (see
[RELEASING.md](RELEASING.md#variables)).

## The macOS app bundle

```sh
scripts/bundle-macos.sh                     # builds a release binary first
BIN=target/universal/shard scripts/bundle-macos.sh    # or bundle a binary you built
SIGN_IDENTITY="Developer ID Application: …" scripts/bundle-macos.sh   # sign it for distribution
```

It writes `target/release/Shard.app`: the binary as `Contents/MacOS/shard`, `Shard.icns`, and an
`Info.plist` with the bundle identifier `dev.shard.app`, the version from `Cargo.toml`, and
macOS 11 as the minimum. Without `SIGN_IDENTITY` the bundle is signed ad hoc (`codesign --sign -`),
which is enough to run it on the machine that built it. With `SIGN_IDENTITY` it's signed with that
identity, the hardened runtime and a timestamp, as notarisation needs.

## A universal macOS binary

This is what `.github/workflows/release.yml` does:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin
mkdir -p target/universal
lipo -create -output target/universal/shard \
  target/aarch64-apple-darwin/release/shard target/x86_64-apple-darwin/release/shard
BIN=target/universal/shard scripts/bundle-macos.sh

# What installed copies update from: the app, zipped
ditto -c -k --keepParent target/release/Shard.app Shard-X.Y.Z-macos-universal.zip
```

## The disk image

```sh
brew install create-dmg
scripts/make-dmg.sh target/release/Shard.app Shard-X.Y.Z-macos-universal.dmg
```

`scripts/make-dmg.sh` copies the app into a scratch folder, joins
`packaging/macos/dmg-background.png` and `dmg-background@2x.png` into one TIFF with `tiffutil`,
and runs [create-dmg](https://github.com/create-dmg/create-dmg): the volume is named Shard with
`Shard.icns` as its icon, the window is 660 by 420 points with 112-point icons, and `Shard.app`
sits at (170, 205) beside a link to Applications at (490, 205), on that background.

create-dmg has Finder lay the window out itself, which is the only way current versions of Finder
reliably show the background (a `.DS_Store` written by other tools, such as dmgbuild, doesn't
show it). So it opens a Finder window while it works, and macOS may first ask whether your
terminal may control Finder. `hdiutil` sometimes fails with "Resource busy", so the script tries
up to three times.

The workflow signs the disk image when it has a Developer ID, and notarises and staples it when
the Apple secrets are set too (see [RELEASING.md](RELEASING.md#secrets)).

## The Windows installer and zip

The installer is the program itself under another name. A copy whose file name contains "setup"
(in any case) runs setup when it's opened with no arguments, and `--silent` installs with the
defaults and no window:

```powershell
Copy-Item target/release/shard.exe Shard-Setup-X.Y.Z-windows-x64.exe
```

The portable zip, which runs wherever it's unpacked and doesn't offer setup:

```powershell
$name = "Shard-X.Y.Z-windows-x64"
New-Item -ItemType Directory $name | Out-Null
Copy-Item target/release/shard.exe "$name/Shard.exe"
Copy-Item README.md, LICENSE-MIT, LICENSE-APACHE $name
Compress-Archive -Path "$name/*" -DestinationPath "$name.zip"
```

The release also publishes the bare binary as `Shard-X.Y.Z-windows-x64.exe`, which is what
installed copies update from.

## A Debian package

`Cargo.toml` has a `[package.metadata.deb]` section for
[cargo-deb](https://github.com/kornelski/cargo-deb):

```sh
cargo install cargo-deb --locked
cargo build --release --locked
cargo deb --no-build --output shard_X.Y.Z_amd64.deb
sudo apt install ./shard_X.Y.Z_amd64.deb
```

The package installs `/usr/bin/shard`, `/usr/share/applications/shard.desktop`, the icons under
`/usr/share/icons/hicolor/<size>/apps/shard.png` (32 to 512 px) and the two licence files in
`/usr/share/doc/shard/`. Its dependencies are what cargo-deb finds the binary links against, plus
`libwayland-client0` and `libvulkan1`, which GPUI loads at run time where cargo-deb can't see
them. A Shard installed under `/usr` doesn't update itself: it tells you to update through the
package manager, and it doesn't offer setup.

Use `arm64` in the file name on an arm64 machine, as the workflow does.

## An RPM package

`Cargo.toml` has a `[package.metadata.generate-rpm]` section for
[cargo-generate-rpm](https://github.com/cat-in-136/cargo-generate-rpm). The workflow pins
0.21.1:

```sh
cargo install cargo-generate-rpm --locked --version 0.21.1
cargo build --release --locked
cargo generate-rpm --output shard-X.Y.Z-1.x86_64.rpm
sudo dnf install ./shard-X.Y.Z-1.x86_64.rpm
```

It installs the same files as the `.deb`, with the licences in `/usr/share/licenses/shard/`. Its
requirements are found by reading the binary (`auto-req = "builtin"`), plus `libwayland-client`
and `vulkan-loader`. Use `aarch64` in the file name on an arm64 machine.

## The AppImage

The workflow builds it with [appimagetool](https://github.com/AppImage/appimagetool) 1.9.1. On
x64 (use `aarch64` and `arm64` for arm64):

```sh
sudo apt-get install -y --no-install-recommends file     # appimagetool needs it
curl -fsSL -o appimagetool https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
chmod +x appimagetool
mkdir -p AppDir/usr/bin AppDir/usr/share/applications AppDir/usr/share/icons/hicolor/512x512/apps
cp target/release/shard AppDir/usr/bin/
cp packaging/linux/shard.desktop AppDir/usr/share/applications/
cp packaging/linux/shard.desktop AppDir/
cp assets/icons/png/shard-512.png AppDir/usr/share/icons/hicolor/512x512/apps/shard.png
cp assets/icons/png/shard-512.png AppDir/shard.png
ln -s usr/bin/shard AppDir/AppRun
ARCH=x86_64 ./appimagetool --appimage-extract-and-run AppDir Shard-X.Y.Z-linux-x64.AppImage
```

`--appimage-extract-and-run` lets appimagetool run without FUSE, as on CI runners.

An AppImage runs the binary from a read-only mount, so setup copies that binary into
`~/.local/bin`. Run without installing, it can't update itself.

## The Linux tarball

```sh
name=shard-X.Y.Z-linux-x64
mkdir "$name"
cp target/release/shard packaging/linux/shard.desktop README.md LICENSE-MIT LICENSE-APACHE "$name/"
cp assets/icons/png/shard-512.png "$name/shard.png"
tar czf "$name.tar.gz" "$name"
```

The release also publishes the bare binary as `shard-X.Y.Z-linux-<arch>`, which is what installed
copies update from.

## The extension packages

```sh
scripts/package-extension.sh X.Y.Z [out-dir]
```

It needs `python3` and `zip`, and writes two zips from the one source in `extension/`, each with
the manifest's `version` set to `X.Y.Z`:

- `shard-extension-X.Y.Z-chrome.zip`, for the Chrome Web Store and Edge Add-ons: the manifest as
  it is, with the background script as a service worker.
- `shard-extension-X.Y.Z-firefox.zip`, for Firefox Add-ons: the same script as an event page
  (`background.scripts`), no `minimum_chrome_version`, and `browser_specific_settings.gecko`
  with the add-on ID `shard-integration@manyeya.github.io`, `strict_min_version` 128.0 and
  `data_collection_permissions` saying it collects nothing.

CI and the release workflow lint the Firefox build with Mozilla's
[web-ext](https://github.com/mozilla/web-ext), treating warnings as errors:

```sh
scripts/package-extension.sh 0.0.0 build
mkdir -p build/firefox && (cd build/firefox && unzip -q ../shard-extension-0.0.0-firefox.zip)
npx --yes web-ext@8 lint --source-dir build/firefox --warnings-as-errors
```

To use the extension from a checkout you don't need the zips; see
[CONTRIBUTING.md](../CONTRIBUTING.md#testing-the-browser-extension).

## The package-manager manifests

```sh
scripts/package-manifests.sh X.Y.Z <dist> <out>
```

`<dist>` must hold the release's `Shard-X.Y.Z-macos-universal.dmg`,
`Shard-X.Y.Z-windows-x64.zip` and `Shard-Setup-X.Y.Z-windows-x64.exe`, whose SHA-256 it records
(with `shasum`). It writes `<out>/Casks/shard.rb` (the Homebrew cask), `<out>/bucket/shard.json`
(the Scoop manifest) and `<out>/winget/manifests/m/manyeya/Shard/X.Y.Z/` (the winget version,
installer and locale manifests). Their URLs point at the GitHub release `vX.Y.Z`, so the files
have to be the ones published there. [RELEASING.md](RELEASING.md#homebrew-scoop-and-winget) says
where each one goes.

## The setup window

```sh
SHARD_SETUP_DEMO=1 cargo run -- --setup         # install, with pretend steps
SHARD_SETUP_DEMO=1 cargo run -- --uninstall     # remove, with pretend steps
SHARD_REDUCE_MOTION=1 SHARD_SETUP_DEMO=1 cargo run -- --setup   # the still version
```

`SHARD_SETUP_DEMO` (any value) runs the setup window with steps that change nothing and moves on
by itself; its extension step lists every browser it finds, whether or not Shard is in its store,
and opens nothing. `SHARD_REDUCE_MOTION=1` keeps the window still, as the system's reduce-motion
setting does; any other value animates it even when the system asks for less motion.

Without the demo, setup really installs. `SHARD_INSTALL_DIR` puts the app in another folder, but
start at login still writes the real login entry, so turn that option off when trying it. The
store IDs can be given at run time (`SHARD_CHROME_STORE_ID=<id>` and so on) to try the extension
step. On macOS, setup only installs a `Shard.app`, so bundle the build first.

## Tests and checks

```sh
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

Unit tests live next to the code. They run local HTTP servers on `127.0.0.1`, need no internet,
and use scratch folders in the system's temporary directory, never your data folder. On Windows
set `AWS_LC_SYS_PREBUILT_NASM=1` first, and on Linux install the libraries above, as for a build.
CI runs both commands on macOS, Linux and Windows, and fails on any Clippy warning.

### Setup, for real

CI and the release workflow also install Shard with setup, check what it put where, install it
again over the top, and remove it:

```sh
cargo build --locked
scripts/test-setup.sh target/debug/shard                                              # Linux
BIN=target/debug/shard scripts/bundle-macos.sh && scripts/test-setup.sh target/release/Shard.app   # macOS
```

`scripts/test-setup.sh` works in a scratch home, data folder and install folder, on port 47119,
so it doesn't touch your own Shard. It checks the installed binary, the login entry, and on Linux
the desktop entry and icon, then that `--uninstall --silent` removes them.

On Windows:

```powershell
cargo build --locked
Copy-Item target/debug/shard.exe Shard-Setup-test.exe
$env:RUNNER_TEMP = $env:TEMP          # the script keeps its data folder there; CI sets it
scripts/test-setup.ps1 Shard-Setup-test.exe
```

`scripts/test-setup.ps1` installs into the real `%LOCALAPPDATA%\Programs\Shard`, Start menu,
Settings › Apps entry and `Run` key, and removes them all at the end, so run it on a machine or
virtual machine where you don't mind losing an installed Shard.

### The extension, end to end

The browser extension has an end-to-end test that drives headless Chrome for Testing:

```sh
CHROME=/path/to/chrome-for-testing scripts/test-extension.sh
```

It needs `agent-browser`, `python3`, `sqlite3` and `curl`, and port 47110 free (quit Shard first).
[CONTRIBUTING.md](../CONTRIBUTING.md#testing-the-browser-extension) explains what it checks. CI
doesn't run it.

### The workflows

CI checks the workflow files with [actionlint](https://github.com/rhysd/actionlint) 1.7.12. To
run it yourself, install actionlint and run `actionlint` in the repository.

## The fixture server

`scripts/fixture_server.py` is the web server the extension test downloads from, and it's handy
for trying downloads by hand:

```sh
python3 scripts/fixture_server.py /tmp/expected.bin
```

It writes the 4 MB file it serves to the path you give (to compare against), then listens on
`127.0.0.1:8765`:

- `GET /` returns a page with a download link and sets the cookie `sid=s3cret`.
- `GET /files/report.bin` returns the file, honouring `Range` (206 with `Content-Range`), with
  `Content-Disposition: attachment; filename="Quarterly Report.bin"`, a strong `ETag` and
  `Accept-Ranges: bytes`, but only when the request carries that cookie. Without it, the answer is
  403.

It prints one line per request with the path, the `Range` header, whether the cookie was sent,
the referrer and the start of the user agent, so you can see what Shard asked for.

## Icons and the logo

The logo and icons are generated and committed. Rebuild them only after changing the logo:

```sh
python3 -m venv .venv && .venv/bin/pip install fonttools
.venv/bin/python scripts/logo.py                 # assets/logo/*.svg and src/logo.rs
CHROME=/path/to/chrome-for-testing scripts/icons.sh
```

`scripts/logo.py` cuts the S of `assets/fonts/ArchivoExpanded-ExtraBold.ttf` into three bands and
writes `assets/logo/shard-icon.svg`, `shard-mark.svg`, `shard-mark-mono.svg`, and `src/logo.rs`
(the outline as points, for the title bar's mark and the tray icon). `scripts/icons.sh`
rasterises the SVGs with headless Chrome into `assets/icons/png/`, `assets/icons/Shard.icns`,
`assets/icons/shard.ico` and `extension/icons/`. It uses `sips` and `iconutil`, so it runs on
macOS only.

## The artwork

The disk image's background and the store images are drawn in `packaging/art/art.html` and
committed. Rebuild them after changing that page or the site's console:

```sh
scripts/render-art.sh
```

It needs [agent-browser](https://github.com/vercel-labs/agent-browser) and `python3`. It serves
the repository on `127.0.0.1:8797`, captures the website's console a few seconds into its
simulated download (into `packaging/art/console.png`, which git ignores), then renders each piece
of `art.html`:

| Piece | Written to | Size |
| --- | --- | --- |
| `dmg` | `packaging/macos/dmg-background.png` and `dmg-background@2x.png` | 1000×480, and 2000×960 |
| `social` | `packaging/stores/social-preview.png` | 1280×640 |
| `promo-small` | `packaging/stores/promo-small.png` | 440×280 |
| `marquee` | `packaging/stores/promo-marquee.png` | 1400×560 |
| `shot-1`, `shot-2` | `packaging/stores/screenshot-1.png`, `screenshot-2.png` | 1280×800 |

[STORES.md](STORES.md#before-you-start) says which store field each image goes in.
