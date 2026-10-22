# Privacy

Shard has no accounts, no analytics, no telemetry and no crash reporting. It doesn't send anything
about you or your downloads to the project. This page lists every network request Shard makes on
its own, everything it stores, and what the browser extension sees, so you can check that claim
against the code.

## What Shard sends, and to whom

### The servers you download from

When you add a link, Shard talks to that link's server: a first request for one byte (to learn the
size and name), then one ranged request per connection. With each request it sends:

- the URL, and any redirects the server sends it to;
- for downloads the browser extension handed over: the page's cookies for that URL, the referrer
  and the browser's user agent, exactly as the browser would have sent them;
- otherwise the user agent `shard/<version>`.

Those servers see your IP address, as they would for any download. With "Combine networks" on,
they may see one address per network you use.

### GitHub, to check for updates

When "Update automatically" is on (it is by default), Shard checks for a new version when it
starts and every six hours after, while it runs with its window or tray (not in `shard serve` or
`shard get`). Each check fetches two small files:

```
https://github.com/manyeya/shard/releases/latest/download/update.txt
https://github.com/manyeya/shard/releases/latest/download/update.txt.sig
```

GitHub redirects these to its file servers. If there's a newer version, Shard downloads the build
for your computer from the URL in that signed list, which is also on GitHub. The requests carry
the user agent `shard/<version>`, which tells GitHub which version you run, and like any request
they show GitHub your IP address. GitHub's handling of that is covered by
[GitHub's privacy statement](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement).
Nothing else goes with them: no ID, no list of downloads, no settings.

To stop update checks:

- turn off "Update automatically" in the Limits panel; "Check now" still checks when you press it;
- or set the environment variable `SHARD_NO_UPDATE` (any value), which also stops "Check now".

### Nothing else

There are no other requests. In particular:

- the fonts and icons are compiled into the app, so drawing the window fetches nothing;
- combining networks looks at your network interfaces locally (on macOS by running
  `networksetup -listallhardwareports`); it contacts no server to do it;
- notifications are the system's own, shown locally.

## What Shard keeps on your computer

Everything is in your user account. The data folder is:

| System | Folder |
| --- | --- |
| macOS | `~/Library/Application Support/shard` |
| Windows | `%APPDATA%\shard` |
| Linux | `$XDG_DATA_HOME/shard`, else `~/.local/share/shard` |

(`SHARD_DATA` moves it.) It holds:

- **`shard.db`** (with `shard.db-wal` and `shard.db-shm`), an SQLite database of your downloads
  and settings. For each download it stores the URL, the folder and file name, size and progress,
  its status and any error, and when you added it.
- **Cookies.** For downloads the extension handed over, the same database stores the cookies,
  referrer and user agent the browser gave it, in plain text, so the download can resume after a
  pause, a crash or a restart. They're deleted from the download's row when it finishes. Queued,
  paused and failed downloads keep theirs until they finish or you remove them. On macOS and Linux
  Shard makes the data folder readable only by you (`0700`) each time it starts. The database
  isn't encrypted, and SQLite may keep deleted data in unused pages of the file for a while.
- **`updates/`**, the update builds Shard downloaded, by version.

Next to each unfinished download, in its download folder, are `<file>.part` (the bytes so far)
and `<file>.resume` (how far each part got: offsets and the server's ETag or Last-Modified date,
no cookies). Both go away when the download finishes or is removed.

Launch at login, when on, is an entry in the system's own place for it (a LaunchAgent on macOS,
the `Run` registry key on Windows, an autostart entry on Linux), pointing at Shard.

### Copied links

With "Offer copied links" on (it is by default), Shard reads the clipboard once a second while it
runs. It only looks for text that is a single `http://` or `https://` link; when one looks like a
link to a file, a notification offers to download it. The clipboard's contents aren't stored or
sent anywhere. Turn "Offer copied links" off in the Limits panel to stop the background reading.

Separately, and whatever that setting says, the link bar reads the clipboard when you bring the
window forward, and fills in a copied link if the bar is empty and the link isn't already in the
list.

### Deleting your data

Removing a download from the list deletes its row (and its partial files; the finished file too
if you tick the box). To delete everything, quit Shard and delete the data folder. Uninstalling
through setup offers to delete it for you.

## The browser extension

The extension's job is to hand downloads to Shard. It:

- reacts only when the browser starts a download; it has no content scripts and never reads the
  pages you visit;
- reads the cookies for that download's URL only, at that moment;
- sends the URL, those cookies, the referrer and the browser's user agent to Shard on your own
  computer, `http://127.0.0.1:47110`, and nowhere else;
- if Shard takes the download, cancels the browser's copy and removes it from the browser's
  download list; if Shard isn't running or refuses, leaves the download to the browser;
- when it's installed or updated, sends an empty `POST /hello` to the same address, so setup can
  show that it's connected. The message carries nothing; setup only counts that it arrived.

Its permissions, and why each is needed, are listed in
[SECURITY.md](../SECURITY.md#the-browser-extensions-permissions).

## The website

The website, <https://manyeya.github.io/shard/>, is static and hosted by GitHub Pages. It has no
analytics, cookies or third-party scripts; its fonts are served with it. To show the latest
release, your browser asks GitHub's API for it
(`https://api.github.com/repos/manyeya/shard/releases/latest`). GitHub sees those requests and the
page views, under its own privacy statement.

## Changes

If Shard ever makes a new kind of request, this page will be updated in the same change, and the
changelog will say so.
