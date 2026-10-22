# Troubleshooting

Fixes for the problems people run into most. If yours isn't here, ask in
[Discussions](https://github.com/manyeya/shard/discussions) or
[open a bug report](https://github.com/manyeya/shard/issues/new/choose), with what Shard printed
(see [where errors go](#where-errors-go)).

## Contents

- [Where errors go](#where-errors-go)
- [macOS: "Shard can't be opened"](#macos-shard-cant-be-opened)
- [Windows: SmartScreen stops Shard](#windows-smartscreen-stops-shard)
- [Port 47110 is taken](#port-47110-is-taken)
- [Setup didn't see the extension connect](#setup-didnt-see-the-extension-connect)
- [The browser extension doesn't hand downloads over](#the-browser-extension-doesnt-hand-downloads-over)
- [A download fails](#a-download-fails)
- [Linux: no tray icon](#linux-no-tray-icon)
- [Linux: Shard won't start](#linux-shard-wont-start)
- [Notifications or copied-link offers don't appear](#notifications-or-copied-link-offers-dont-appear)
- [Updates don't install](#updates-dont-install)
- [Sleep or shut down when done doesn't happen](#sleep-or-shut-down-when-done-doesnt-happen)
- [Combining networks shows nothing to combine](#combining-networks-shows-nothing-to-combine)
- [Resetting settings](#resetting-settings)

## Where errors go

Shard writes its errors to standard error, the terminal it was started from. It keeps no log
file. Its messages start with `shard:`, for example
`shard: port 47110 is taken (...); the browser extension won't reach this Shard`.

To see them, quit Shard first (tray menu › Quit Shard, or ⌘Q / Ctrl+Q in the window): a second
copy started while one runs hands over to it and exits straight away. Then start it from a
terminal:

```sh
# macOS
/Applications/Shard.app/Contents/MacOS/shard
# Linux
shard            # or the path to the binary you unpacked
```

```powershell
# Windows: Shard is a windowed program and only attaches to the terminal when given an argument.
# --hidden starts it in the tray; choose Open Shard there for the window.
& "$env:LOCALAPPDATA\Programs\Shard\Shard.exe" --hidden
```

On Windows the prompt comes back at once, and Shard's messages appear in that window as they
happen. Use the path where your Shard is if it isn't installed there (the portable zip runs
wherever you unpacked it).

`shard get <url>` prints its progress and any error to the terminal too, which is a quick way to
see what a server says about a link.

## macOS: "Shard can't be opened"

Release builds are signed ad hoc and not notarised by Apple unless the project has a Developer ID,
so the first time you open Shard, macOS refuses with a message such as "Apple could not verify
'Shard' is free of malware" or "'Shard' can't be opened because Apple cannot check it for
malicious software".

On macOS 15 Sequoia and later:

1. Try to open Shard once, and close the message.
2. Open System Settings › Privacy & Security.
3. Scroll to Security. Next to the note about Shard, choose **Open Anyway**, and confirm with your
   password or Touch ID.

On macOS 14 and earlier you can instead Control-click Shard in Finder, choose Open, then Open
again.

macOS only asks once. Updates that Shard installs itself don't ask again.

Before opening it, you can check you have the real file by comparing it with the release's
`SHA256SUMS` (see [SECURITY.md](../SECURITY.md#verifying-a-release)).

## Windows: SmartScreen stops Shard

The Windows builds aren't code-signed yet, so Microsoft Defender SmartScreen may show "Windows
protected your PC" when you run the Setup `.exe` or `Shard.exe`. Choose **More info**, then
**Run anyway**. As with macOS, you can check the file against `SHA256SUMS` first.

## Port 47110 is taken

Shard listens on `127.0.0.1:47110` for the browser extension, and the same port makes sure only
one Shard runs. If another program already uses it, Shard still starts but prints:

```
shard: port 47110 is taken (...); the browser extension won't reach this Shard
```

and the extension can't hand it downloads.

First check whether the program on the port is another Shard:

```sh
curl http://127.0.0.1:47110/ping      # prints "shard" if it is
```

If it is, that's the Shard that's running (maybe in the tray, or a development copy); quit it or
use it. If not, find what's listening:

```sh
# macOS and Linux
lsof -nP -iTCP:47110 -sTCP:LISTEN
# Linux, without lsof
ss -ltnp 'sport = :47110'
```

```powershell
# Windows
Get-NetTCPConnection -LocalPort 47110 -State Listen | ForEach-Object { Get-Process -Id $_.OwningProcess }
```

Stop or reconfigure that program. Setting `SHARD_PORT` moves Shard to another port, but the
extension always talks to 47110, so that only helps if you don't use the extension (it's meant for
running a development copy beside an installed one).

## Setup didn't see the extension connect

Setup's last step opens Shard's page in each browser's store and waits for the extension to say
hello. As it's added, the extension sends an empty `POST /hello` to `127.0.0.1:47110`; setup holds
that port while the step is up, and when the hello arrives it shows "Shard is in" the browser and
moves on. Until then it says "Waiting for" the browser "to add it…", or, if it couldn't listen at
all, "Setup can't hear the browser from here; move on once it's added".

The extension works whether or not setup saw it arrive, so you can always choose **Skip this one**
or **Finish**, then start a download to check (see
[the browser extension doesn't hand downloads over](#the-browser-extension-doesnt-hand-downloads-over)).
The usual reasons setup doesn't hear it:

- **The extension wasn't added.** The store page opened, but its button (Add to Chrome, Get, or
  Add to Firefox) and then the browser's own confirmation weren't both pressed. Choose **Open it
  again** and finish adding it. When setup sends Edge to the Chrome Web Store, Edge first asks
  you to allow extensions from other stores.
- **It was there already.** The extension says hello only when it's installed or updated, so if
  it was in that browser before setup ran, setup never hears from it. Check the browser's
  extensions page (`chrome://extensions`, `edge://extensions`, `about:addons` in Firefox): if
  "Shard integration" is there and on, you're done.
- **Something else holds port 47110.** Setup closes a running Shard as its first step, but if
  another program (or a Shard started again since) has the port, setup can't listen there. Find
  it as in [port 47110 is taken](#port-47110-is-taken). Setup started with `SHARD_PORT` set
  listens on that port instead, which the extension never uses.
- **Requests to 127.0.0.1 are blocked.** A browser policy set by an organisation, another
  extension or security software can stop the extension reaching Shard on your own computer. The
  extension's console shows it: in a Chromium browser, turn on Developer mode at
  `chrome://extensions` and open the extension's service worker; in Firefox, open
  `about:debugging`, then This Firefox, and Inspect beside Shard integration. A failed request to
  `http://127.0.0.1:47110/hello` there means it was blocked; the same block stops downloads being
  handed over, so allow `127.0.0.1` for the browser or that program.

## The browser extension doesn't hand downloads over

Work through these in order:

1. **Is Shard running?** The extension only hands a download over when Shard answers. If Shard
   isn't running (not even in the tray), the browser downloads the file itself. That's by design,
   so a download is never lost. Start Shard, or turn on launch at login.
2. **Is Shard reachable?** `curl http://127.0.0.1:47110/ping` should print `shard`. If it
   doesn't, see [port 47110 is taken](#port-47110-is-taken). A Shard started with `SHARD_PORT`
   set can't be reached by the extension.
3. **Is the extension added and on?** Open `chrome://extensions` (`edge://extensions`,
   `brave://extensions`, `arc://extensions`, and so on, or `about:addons` in Firefox) and check
   "Shard integration" is there and enabled. If it isn't, add it from your browser's store (the
   Chrome Web Store, Edge Add-ons or Firefox Add-ons), where setup's last step would have taken
   you.
4. **Is it a kind of download Shard takes?** The extension leaves these to the browser:
   - links that aren't `http://` or `https://`, such as `blob:` and `data:` files a page makes
     itself;
   - downloads that aren't in progress, such as old ones the browser restores to its list.
5. **Is the browser supported?** The extension runs in Chromium browsers from version 116
   (Chrome, Edge, Brave, Arc, Vivaldi, Opera and Chromium) and in Firefox 128 or later. If your
   browser is managed by an organisation, its policy may block extensions from the stores, or
   requests to `127.0.0.1`.

Some downloads are handed over but then fail in Shard:

- **Downloads a page starts with a form submission (POST).** Shard fetches the URL with a GET,
  which some servers answer differently or refuse. Turn the extension off for those for now.
- **Downloads that need more than cookies.** Shard sends the page's cookies, referrer and user
  agent. A site that checks something else may refuse it.

## A download fails

The download's card shows the reason. The common ones:

| Message | What it means and what to do |
| --- | --- |
| `the server answered 403 Forbidden` (or `401`, `404`, `410`…) | The server refused the link. A link that has expired: right-click the download, choose **Refresh link…** and paste a fresh link to the same file; if it's the same size, Shard keeps what it has. A link behind a login: start it in the browser with the extension, so the cookies come along. |
| `the file changed on the server; download it again from the start` | The server's copy is different from the one Shard started (its ETag or Last-Modified changed), so the parts can't be joined. Choose **Restart from scratch**. |
| `the server is refusing connections (HTTP 429/503)` | The server kept refusing even a single connection. Try again later, or lower "Connections per download" in the Limits panel. Shard already drops connections a server refuses and adds them back later. |
| `the server did not report the file's size` | The server answered Shard's first range request without saying how big the file is, so Shard can't plan the parts and stops. |
| `the server kept closing the connection early` | The server kept ending the transfer early, several times in a row without any progress. Check your connection, then resume. |
| `Only http and https links can be downloaded.` | Shard doesn't do FTP, torrents or other kinds of link. |

A download that's much slower than expected may simply be limited by your line: parallel
connections only help when one connection doesn't fill it. Check the speed cap in the Limits panel
and the tray menu too.

## Linux: no tray icon

Shard's tray icon is a StatusNotifierItem, sent over D-Bus, so it needs a desktop with a
StatusNotifierItem host:

- **KDE Plasma** has one.
- **GNOME** needs the "AppIndicator and KStatusNotifierItem Support" extension. Ubuntu's GNOME
  includes it; on other distributions install it from your package manager or
  extensions.gnome.org.
- **Other desktops** (Xfce, Cinnamon, MATE, sway, Hyprland…) need their own StatusNotifierItem
  applet or bar module.

Without a host, Shard can't always tell, so it may behave as if it had a tray: closing the window
leaves Shard running with no icon. To get the window back, run `shard` again or open it from your
app launcher; the running copy brings its window forward. To quit, press Ctrl+Q in the window.

If there's no D-Bus session at all, Shard knows there's no tray. It prints `shard: no tray icon:
...`, quits when you close the window, and `--hidden` opens the window instead.

On Linux, clicking the tray icon opens the main window rather than the small panel, because Linux
trays don't say where their icon is.

## Linux: Shard won't start

Shard's window is drawn with GPUI, which needs:

- the Vulkan loader and a Vulkan driver for your GPU (on Debian and Ubuntu, `libvulkan1`, and
  `mesa-vulkan-drivers` for Intel and AMD graphics; NVIDIA's own driver includes Vulkan);
- the Wayland client library on Wayland, and the X11 libraries on X11;
- fontconfig and FreeType.

The `.deb` asks for what it needs. For the tarball, AppImage or a binary you built, on Debian or
Ubuntu:

```sh
sudo apt install libvulkan1 mesa-vulkan-drivers libwayland-client0 libxkbcommon0 libxkbcommon-x11-0 \
    libxcb1 libx11-xcb1 libfontconfig1 libfreetype6
```

Release binaries need glibc 2.35 or later (Ubuntu 22.04, Debian 12 or newer). Run Shard from a
terminal to see why it stopped.

## Notifications or copied-link offers don't appear

- **Check the setting.** "Notify when done" (finished and failed downloads) and "Offer copied
  links" are both in the Limits panel.
- **macOS: run the app, not the bare binary.** macOS only shows notifications for apps in a
  bundle. `Shard.app` from the disk image or setup is one; a binary run with `cargo run` isn't,
  and shows none. Also check System Settings › Notifications › Shard.
- **Windows:** check Settings › System › Notifications, and that Do not disturb is off.
- **Linux:** your desktop needs a notification service; most have one.
- **Copied links** are offered only when the link is a single `http(s)` link, its last part looks
  like a file (an extension such as `.zip`, `.mp4`, `.pdf` or `.iso`), it isn't already in the
  list, and it's a new copy. They arrive as notifications, so the points above apply too.

## Updates don't install

The update line in the Limits panel says what's happening. If it says:

| Message | What to do |
| --- | --- |
| `Installed by a package manager: update Shard through it.` | Shard was installed under `/usr`, by the `.deb` or `.rpm`. Update it with `apt`, `dnf` or the package you downloaded. |
| `Updates install into Shard.app, and this copy isn't one.` | On macOS, only a `Shard.app` updates itself. Use the app from the disk image. |
| `Move Shard somewhere you can write to (not …) to update it.` | Shard needs to replace itself in that folder. Install it into Applications (macOS), your user folder (Windows) or `~/.local/bin` (Linux), or run setup. On macOS, a Shard opened straight from the disk image or Downloads may be running from a read-only copy; move it to Applications. An AppImage run without installing is always read-only (the folder named is the AppImage's own mount, usually under `/tmp`): start it with `--setup` to install it into `~/.local/bin`, where it updates itself. |
| `Couldn't check: …` | The check failed, often because there's no connection to github.com. It tries again in six hours, or choose **Check now**. If it says the signature doesn't match, don't install anything by hand from elsewhere: report it (see [SECURITY.md](../SECURITY.md)). |

Updates are off when "Update automatically" is off (Check now still works) or when the
environment variable `SHARD_NO_UPDATE` is set (then Check now does nothing either).

When an update is ready, choose **Restart to update** in the title bar or the tray menu. Shard
replaces itself, keeps the old version beside the new one until the next start, and opens the new
one. Downloads carry on from where they were.

## Sleep or shut down when done doesn't happen

When the queue finishes with "When done" set to sleep or shut down, Shard waits a minute (with a
notification and a banner you can cancel), then asks the system. Anything starting in that minute
cancels it. What it runs:

- **macOS:** `pmset sleepnow` to sleep, and `osascript -e 'tell application "System Events" to shut down'`
  to shut down. macOS may ask whether Shard may control System Events; if you refuse, shut down
  doesn't happen.
- **Windows:** `rundll32.exe powrprof.dll,SetSuspendState 0,1,0` to sleep, `shutdown /s /t 0` to
  shut down. With hibernation turned on, Windows may hibernate instead of sleeping.
- **Linux:** `systemctl suspend` and `systemctl poweroff`, which your system has to allow for your
  user (most desktops do).

If the command can't be run, Shard prints `shard: couldn't run <program>: ...`.

Keep awake only stops idle sleep while something downloads. The display can still turn off, and
closing a laptop's lid still sleeps it.

## Combining networks shows nothing to combine

Combining needs at least two networks that can each reach the internet. Shard counts interfaces
that are up, aren't loopback, have a routable address, and aren't a VPN, tunnel, bridge or
virtual machine adapter (`utun`, `wg`, `tailscale`, `tun`, `tap`, `docker`, `vmnet`, `vboxnet`,
`bridge` and similar are left out). It looks again every 30 seconds, so a phone you just plugged
in shows up shortly. A download takes the networks there are when it starts; pause and resume it
to use one that appeared since.

## Resetting settings

Settings and the download list are in `shard.db` in Shard's data folder:

| System | Data folder |
| --- | --- |
| macOS | `~/Library/Application Support/shard` |
| Windows | `%APPDATA%\shard` |
| Linux | `~/.local/share/shard` (or `$XDG_DATA_HOME/shard`) |

To start fresh, quit Shard, then delete `shard.db`, `shard.db-wal` and `shard.db-shm` from that
folder. Shard makes a new, empty database when it starts, with the default settings. Files you
downloaded stay where they are. Unfinished downloads leave their `.part` and `.resume` files in
their folders; delete those by hand.

To reset only the settings and keep the list, quit Shard and run:

```sh
sqlite3 "<data folder>/shard.db" "DELETE FROM settings;"
```

Launch at login isn't stored in the database. Turn it off in the Limits panel or the tray menu, or
delete `~/Library/LaunchAgents/dev.shard.app.plist` (macOS), `~/.config/autostart/shard.desktop`
(Linux), or the `Shard` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
(Windows).

To try Shard with clean settings without touching yours, quit it and start it with `SHARD_DATA`
pointing at an empty folder, for example `SHARD_DATA=/tmp/shard-clean shard`.
