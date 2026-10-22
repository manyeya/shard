# Security policy

## Supported versions

Only the latest release gets security fixes. Fixes ship as a new release, and installed copies
pick it up through the updater (unless updates are turned off, or Shard was installed by a
package manager that updates it instead).

| Version | Supported |
| --- | --- |
| Latest release | Yes |
| Anything older | No: update first, then check whether the problem is still there |

## Reporting a vulnerability

Please don't open a public issue, discussion or pull request for a security problem.

Report it privately through GitHub:
**<https://github.com/manyeya/shard/security/advisories/new>**

Only the maintainers can see what you send there, and the advisory becomes the place to discuss
the fix. Please include:

- what an attacker can do, and what they need first (a web page you visit, a file you download,
  local access, a network position);
- the Shard version and system, and how it was installed;
- steps to reproduce, or a proof of concept;
- whether you've told anyone else, and any date you plan to publish.

These are goals, not guarantees, from a project with one maintainer:

| Step | Goal |
| --- | --- |
| Acknowledge your report | Within 3 days |
| Triage: confirm it, judge severity, plan a fix | Within 7 days |
| Fix and release | As fast as the severity calls for; you'll be kept up to date in the advisory |

Once a fix is released, the advisory is published with credit to you, unless you'd rather not be
named. Please give us a reasonable time to ship the fix before you disclose it.

## Scope and threat model

Shard is a desktop app that downloads what you ask it to. These notes say where its security
boundaries are, so you can tell a vulnerability from expected behaviour.

### The local endpoint

Shard listens on `127.0.0.1:47110` (`SHARD_PORT` moves it) for the browser extension and for a
second `shard` launch. It answers `GET /ping`, `POST /add` (queue a URL with cookie, referrer
and user agent) and `POST /show` (bring the window back). The installer adds `POST /hello` (the
extension announcing itself, so setup can confirm it's connected) and `POST /quit` (setup asking
a running Shard to quit before replacing it).

- It binds the loopback address only, so other machines can't reach it.
- Any request carrying an `Origin` header that isn't a browser extension's
  (`chrome-extension://…` or `moz-extension://…`) is refused with 403. Browsers set `Origin` on
  the cross-site requests a web page can make, and a page can't forge it, so a web page can't
  queue downloads or show the window.
- Everything that changes state is a POST, because a page can make a GET without an `Origin`
  (an `<img>`, for example). A page may still be able to tell that something answers on the
  port, through `GET /ping`.
- Requests without an `Origin` are accepted. That's how a second `shard` launch hands over a
  link. It also means any program running as you on the same computer can add downloads, bring
  the window forward or (with the installer's `POST /quit`) make Shard quit; local programs
  already have that much access to your account, so this is outside the threat model.
- While setup waits for the extension, setup itself holds the port and answers only a
  `POST /hello` carrying an extension `Origin`; everything else gets a 404.
- Any browser extension can call it, not only Shard's: the check is on the `chrome-extension://`
  prefix, not on Shard's extension ID.
- Headers are capped at 16 KiB, bodies at 256 KiB, and each connection gets 10 seconds.
- It's plain HTTP over loopback, with no authentication token.

A way for a web page to get past the `Origin` check, or for a remote machine to reach the
endpoint, is a vulnerability.

### Cookies at rest

The extension hands Shard the cookies for each download's URL, so downloads behind a login work.
Shard stores them, with the referrer and user agent, in the `headers` column of its SQLite
database (`shard.db` in the data folder, see [docs/PRIVACY.md](docs/PRIVACY.md)), in plain text.

- On macOS and Linux, Shard sets the data folder's permissions to `0700` each time it starts, so
  other users can't read it. On Windows it relies on the per-user permissions of `%APPDATA%`.
- A download's cookies are deleted from its row when it finishes. Queued, paused and failed
  downloads keep theirs, so they can resume. Removing a download deletes its row.
- The database isn't encrypted. Anyone who can read your files (malware running as you, a backup
  of your home folder) can read the cookies of unfinished downloads. SQLite may also keep deleted
  data in free pages of the file until they are reused.

### Signed updates

Every six hours (when "Update automatically" is on, and unless `SHARD_NO_UPDATE` is set) Shard
fetches `update.txt` and `update.txt.sig` from
`https://github.com/manyeya/shard/releases/latest/download/`. `update.txt` lists the new version
and, for each platform, the update build's size, SHA-256 and URL. Shard:

1. checks `update.txt.sig` is a valid Ed25519 signature of `update.txt`'s exact bytes, made by
   the release key whose public half is compiled in (`PUBLIC_KEY` in `src/update.rs`), and
   refuses the update if it isn't;
2. ignores the manifest unless its version is newer than the running one (by semver), so an
   older signed manifest can't downgrade you;
3. downloads this platform's build from the URL in the signed manifest, and checks its SHA-256
   against the manifest, deleting it if it doesn't match;
4. installs it only when you choose "Restart to update".

What this protects against: a release asset replaced or corrupted on GitHub, a tampered
download, or anyone serving a fake update without the private key. What it doesn't cover:

- **The first install.** Nothing signed protects the DMG, Setup `.exe`, zip, tarball or package
  you download by hand. Check it against `SHA256SUMS` (below). `SHA256SUMS` is published by the
  same release, so it detects a corrupted download, not a compromised release.
- **A stolen release key.** Whoever holds `UPDATE_SIGNING_KEY` can sign updates. It lives only as
  a GitHub Actions secret and in the maintainer's offline backup.
- **Being held back.** The manifest has no expiry date, so someone able to serve an old manifest
  could stop you seeing new versions. Fetching it over HTTPS from github.com makes that hard.
- **Copies that don't update themselves**: package-managed Linux installs (under `/usr`), and
  copies in a folder Shard can't write to. They say so in the Limits panel and update through
  their package manager or by hand.

### Unsigned builds, Gatekeeper and SmartScreen

Unless the release workflow has the Apple signing secrets, the macOS app is signed ad hoc and
isn't notarised, so Gatekeeper asks before the first launch. The Windows `.exe` isn't
code-signed, so SmartScreen may warn. The Linux packages aren't signed. The update signature
above is what ties later updates to the project; for the first download, use `SHA256SUMS`.
[docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) explains how to open the app past these
prompts.

### The browser extension's permissions

| Permission | Why it's needed |
| --- | --- |
| `downloads` | To see each new download, and to cancel and erase the browser's copy once Shard has taken it over. |
| `cookies` | To read the cookies for the download's URL and pass them to Shard, so downloads behind a login work. It reads them only for the URL being downloaded, when a download starts. |
| `host_permissions: <all_urls>` | Downloads can come from any site, and Chrome only gives the cookies API access to sites the extension has host permission for. It also covers the request to `http://127.0.0.1:47110`. |

The extension sends data only to `127.0.0.1:47110`. It has no content scripts and never reads
pages. If Shard isn't running, or refuses the download, the browser keeps the download.

### Downloaded files

Shard saves what the server sends; it doesn't scan or open files. File names from
`Content-Disposition` or the URL are reduced to their last path component, with control
characters and the characters Windows refuses (`<>:"|?*`) replaced and leading dots removed, so
a server can't write outside the download folder. Windows device names (`CON`, `NUL` and so on)
aren't filtered yet.

### Out of scope

- Anything that needs an attacker to already run code as you, or to have admin rights.
- A server you chose to download from sending you a malicious file.
- Denial of service by a server, such as one that never finishes sending.
- Problems in third-party dependencies with no effect on Shard (report those upstream; we track
  advisories with `cargo deny`).

## Verifying a release

### Checksums

Every release has `SHA256SUMS`, covering every file in the release. Download it next to the files
you want, then:

```sh
# macOS
shasum -a 256 -c --ignore-missing SHA256SUMS
# Linux
sha256sum -c --ignore-missing SHA256SUMS
```

```powershell
# Windows: compare the output with the file's line in SHA256SUMS
Get-FileHash -Algorithm SHA256 .\Shard-Setup-X.Y.Z-windows-x64.exe
```

### The update signature

You can check `update.txt.sig` yourself with OpenSSL 3. The public key is the 32 bytes of
`PUBLIC_KEY` in `src/update.rs`; as a PEM file it is:

```
-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEA7+NM73Ijcux6jHSqQkfOGCtGADYWbBsAS2eLED6lwns=
-----END PUBLIC KEY-----
```

To make that file from the source rather than trusting this page, run this in a checkout (it
prefixes the raw key with the standard Ed25519 public key header and lets OpenSSL write the PEM):

```sh
sed -n '/^const PUBLIC_KEY/,/^];/p' src/update.rs | grep -o '0x[0-9a-f][0-9a-f]' | sed 's/0x//' | tr -d '\n' > key.hex
{ printf '302a300506032b6570032100'; cat key.hex; } | xxd -r -p | openssl pkey -pubin -inform DER -out shard-update.pub.pem
```

Then verify the manifest of a release:

```sh
openssl pkeyutl -verify -rawin -pubin -inkey shard-update.pub.pem -in update.txt -sigfile update.txt.sig
```

It prints `Signature Verified Successfully`. `-rawin` needs OpenSSL 3; the `openssl` that ships
with macOS is LibreSSL, which doesn't have it, so use Homebrew's (`brew install openssl@3`, then
`$(brew --prefix openssl@3)/bin/openssl`).

`update.txt` then gives the SHA-256 of each update build (the macOS app zip and the bare Windows
and Linux binaries), which you can compare with those files.

The running app does all of this itself before it installs an update. If a check fails, the
Limits panel's update line says why, for example "Couldn't check: the update's signature doesn't
match" or "Couldn't check: the update didn't match its checksum".
