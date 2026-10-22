# Building Shard from source

This covers building and running Shard on each system, making the same packages the release
workflow makes, and running the tests. For setting up to contribute, see
[CONTRIBUTING.md](../CONTRIBUTING.md); for how the code fits together, see
[ARCHITECTURE.md](ARCHITECTURE.md).

## Contents

- [What you need](#what-you-need)
- [macOS](#macos)
- [Linux](#linux)
- [Windows](#windows)
- [Release builds](#release-builds)
- [The macOS app bundle](#the-macos-app-bundle)
- [A universal macOS binary and disk image](#a-universal-macos-binary-and-disk-image)
- [A Debian package](#a-debian-package)
- [The Linux tarball](#the-linux-tarball)
- [The Windows zip](#the-windows-zip)
- [The extension zip](#the-extension-zip)
- [Tests](#tests)
- [The fixture server](#the-fixture-server)
- [Icons and the logo](#icons-and-the-logo)

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
why the `.deb` depends on `libwayland-client0` and `libvulkan1`. You need a Vulkan driver for
your GPU, and a StatusNotifierItem host for the tray icon (KDE Plasma has one; GNOME needs the
AppIndicator extension).

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
it's started from a terminal with an argument (`shard.exe serve`, `shard.exe get <url>`, a link)
it attaches to that terminal, so its messages appear there.

Releases build only `x86_64` for Windows.

## Release builds

`cargo build --release --locked` puts the binary at `target/release/shard` (`shard.exe` on
Windows). That one file is the whole app: the fonts and icons it draws are compiled in. There's no
custom release profile in `Cargo.toml`, so it's Cargo's default `release` profile.

The version comes from `Cargo.toml`. The release workflow refuses a tag that doesn't match it.

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

## A universal macOS binary and disk image

This is what `.github/workflows/release.yml` does:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin
mkdir -p target/universal
lipo -create -output target/universal/shard \
  target/aarch64-apple-darwin/release/shard target/x86_64-apple-darwin/release/shard
BIN=target/universal/shard scripts/bundle-macos.sh

# The disk image: the app and a link to /Applications
mkdir dmg && cp -R target/release/Shard.app dmg/ && ln -s /Applications dmg/Applications
hdiutil create -volname Shard -srcfolder dmg -ov -format UDZO Shard-X.Y.Z-macos-universal.dmg

# What installed copies update from: the app, zipped
ditto -c -k --keepParent target/release/Shard.app Shard-X.Y.Z-macos-universal.zip
```

`hdiutil` sometimes fails with "Resource busy"; trying again gets past it. The workflow signs the
disk image and notarises and staples it only when the Apple secrets are set (see
[RELEASING.md](RELEASING.md)).

## A Debian package

`Cargo.toml` has a `[package.metadata.deb]` section for
[cargo-deb](https://github.com/kornelski/cargo-deb):

```sh
cargo install cargo-deb --locked
cargo build --release --locked
cargo deb --no-build --output shard_X.Y.Z_amd64.deb
sudo apt install ./shard_X.Y.Z_amd64.deb
```

The package installs `/usr/bin/shard`, `/usr/share/applications/shard.desktop` and the icons
under `/usr/share/icons/hicolor/<size>/apps/shard.png` (32 to 512 px). Its dependencies are what
cargo-deb finds the binary links against, plus `libwayland-client0` and `libvulkan1`, which GPUI
loads at run time where cargo-deb can't see them. A Shard installed under `/usr` doesn't update
itself: it tells you to update through the package manager.

Use `arm64` in the file name on an arm64 machine, as the workflow does.

## The Linux tarball

```sh
name=shard-X.Y.Z-linux-x64
mkdir "$name"
cp target/release/shard packaging/linux/shard.desktop README.md "$name/"
cp assets/icons/png/shard-512.png "$name/shard.png"
tar czf "$name.tar.gz" "$name"
```

## The Windows zip

```powershell
$name = "Shard-X.Y.Z-windows-x64"
New-Item -ItemType Directory $name | Out-Null
Copy-Item target/release/shard.exe "$name/Shard.exe"
Copy-Item README.md $name
Compress-Archive -Path "$name/*" -DestinationPath "$name.zip"
```

The release also publishes the bare binary as `Shard-X.Y.Z-windows-x64.exe`, which is what
installed copies update from.

## The extension zip

The workflow sets the manifest's version to the release's, then zips the extension:

```sh
(cd extension && zip -r ../shard-extension-X.Y.Z.zip manifest.json background.js icons)
```

To use the extension from a checkout you don't need the zip: load the `extension` folder
unpacked (see [CONTRIBUTING.md](../CONTRIBUTING.md#testing-the-browser-extension)).

## Tests

```sh
cargo test --locked
```

Unit tests live next to the code. They run local HTTP servers on `127.0.0.1`, need no internet,
and use scratch folders in the system's temporary directory, never your data folder. On Windows
set `AWS_LC_SYS_PREBUILT_NASM=1` first, and on Linux install the libraries above, as for a build.

The browser extension has an end-to-end test that drives headless Chrome for Testing:

```sh
CHROME=/path/to/chrome-for-testing scripts/test-extension.sh
```

It needs `agent-browser`, `python3`, `sqlite3` and `curl`, and port 47110 free (quit Shard first).
[CONTRIBUTING.md](../CONTRIBUTING.md#testing-the-browser-extension) explains what it checks.

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
