# Getting help

## Where to ask

| You want to | Go to |
| --- | --- |
| Ask how to do something, or whether Shard can | [Discussions › Q&A](https://github.com/manyeya/shard/discussions/categories/q-a) |
| Share an idea before it's a concrete request | [Discussions](https://github.com/manyeya/shard/discussions) |
| Report something that's broken | [A bug report](https://github.com/manyeya/shard/issues/new/choose) |
| Ask for a specific feature | [A feature request](https://github.com/manyeya/shard/issues/new/choose) |
| Report a security problem | [Privately, through a security advisory](https://github.com/manyeya/shard/security/advisories/new) (see [SECURITY.md](SECURITY.md)) |

Search the existing issues and discussions first; your question may already have an answer.

Shard has one maintainer and no paid support. Answers come when there's time, usually from the
maintainer or other users.

## Check these first

- [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) covers the common problems: macOS or Windows
  refusing to open Shard the first time, the browser extension not handing downloads over, port
  47110 being taken, no tray icon on Linux, notifications on macOS, and resetting settings.
- The README's "Not yet" list says what Shard doesn't do yet: torrents, downloads a page starts
  with a POST.
- Make sure you're on the latest release. Open the Limits panel (the sliders button in the title
  bar, or ⌘, on macOS and Ctrl+, elsewhere): the line under "Update automatically" shows your
  version, and "Check now" looks for a newer one.

## What to include

The more of this you include, the faster someone can help:

- **Shard's version**, from the update line in the Limits panel.
- **Your system** and its version: macOS 15.1, Windows 11 24H2, Ubuntu 24.04 with GNOME on
  Wayland, and so on.
- **How you installed it**: DMG, Homebrew, Setup `.exe`, Scoop, portable zip, `.deb`, `.rpm`,
  AppImage, tarball, or built from source.
- **The browser and extension**, if the problem involves a download the browser handed over.
- **What you did, what happened, and what you expected.**
- **What Shard printed.** Shard writes its errors to the terminal it was started from. See
  [where errors go](docs/TROUBLESHOOTING.md#where-errors-go) for how to start it from one on each
  system.
- **The kind of link**, never the link itself if it's private, signed or carries a token:
  "a signed S3 link that expires after an hour", "a file behind a login on a forum", "a public
  ISO from a university mirror". Public links to public files are fine.

Don't post cookies, tokens, or the contents of `shard.db`; it holds the cookies of unfinished
downloads.
