# Releasing Shard

Releases are built and published by `.github/workflows/release.yml` when a tag `vX.Y.Z` is pushed.
This is the maintainer's checklist: what the workflow makes, the secrets and variables it reads,
how the package managers and browser stores get each version, how to check a release, and how
the update signing key works. Getting the extension into the stores the first time is in
[STORES.md](STORES.md).

## Contents

- [Before you start](#before-you-start)
- [The checklist](#the-checklist)
- [What the workflow does](#what-the-workflow-does)
- [What a release contains](#what-a-release-contains)
- [Secrets](#secrets)
- [Variables](#variables)
- [Homebrew, Scoop and winget](#homebrew-scoop-and-winget)
- [The browser stores](#the-browser-stores)
- [Checking the release](#checking-the-release)
- [A dry run](#a-dry-run)
- [When something goes wrong](#when-something-goes-wrong)
- [The update signing key](#the-update-signing-key)

## Before you start

- Versions follow [semver](https://semver.org). Installed copies compare versions by semver to
  decide whether an update is newer, and the browser stores only take a version higher than the
  one they have, so a version must always go up.
- `UPDATE_SIGNING_KEY` must be set in the repository's Actions secrets, or the release job fails
  (see [the update signing key](#the-update-signing-key)).
- Everything to release is on `dev`, and CI is green there: the tests, Clippy and the setup
  check on macOS, Linux and Windows, the Firefox build's lint, and actionlint.
- The store IDs are compiled into every binary the workflow builds. Set a store's
  [variable](#variables) before the release whose setup should offer that store, and only once
  its listing is public.

## The checklist

1. **Update the changelog.** In `CHANGELOG.md`, rename `## [Unreleased]` to
   `## [X.Y.Z] - YYYY-MM-DD`, add a new empty `## [Unreleased]` above it, and update the links at
   the bottom:

   ```
   [Unreleased]: https://github.com/manyeya/shard/compare/vX.Y.Z...dev
   [X.Y.Z]: https://github.com/manyeya/shard/compare/vPREVIOUS...vX.Y.Z
   ```

   The release job uses this section as the release notes: everything from the line starting
   `## [X.Y.Z]` to the next `## [` heading.

2. **Bump the version** in `Cargo.toml` (`version = "X.Y.Z"` under `[package]`) and in
   `extension/manifest.json` (`"version": "X.Y.Z"`). `scripts/package-extension.sh` sets the
   version in the extension zips itself, but keeping the source the same means a copy loaded
   unpacked during development says the right version too.

3. **Update `Cargo.lock`** so it records the new version, without changing anything else:

   ```sh
   cargo update --workspace
   ```

   CI builds with `--locked`, which fails if `Cargo.lock` is out of date.

4. **Commit on `dev`** in the house style, for example `Release: 0.2.0`, and push:

   ```sh
   git switch dev
   git commit -am "Release: X.Y.Z"
   git push origin dev
   ```

5. **Wait for CI to pass** for that commit.

6. **Fast-forward `main`** to `dev`. Never merge or commit straight to `main`:

   ```sh
   git switch main
   git merge --ff-only dev
   git push origin main
   ```

   Pushing `main` also republishes the website if `site/` changed.

7. **Tag the release on `main`** and push the tag:

   ```sh
   git tag -a vX.Y.Z -m "Shard X.Y.Z"
   git push origin vX.Y.Z
   ```

   The tag must be exactly `v` plus `Cargo.toml`'s version; the workflow's first job fails
   otherwise.

8. **Watch the Release workflow** in the Actions tab (or with `gh run watch -R manyeya/shard`)
   until the `release` and `packages` jobs have finished.

9. **Check the release**, following [checking the release](#checking-the-release).

10. **Submit the winget manifests** from the run's `winget-manifests` artifact (see
    [winget](#winget)). Nothing else does it.

11. **Read the release notes.** They're the version's section of `CHANGELOG.md`, followed by
    GitHub's generated notes (pull requests grouped by label, see `.github/release.yml`). If the
    section wasn't found, the notes only say to see the changelog; paste the section in by hand.

12. **Go back to `dev`** (`git switch dev`) for the next change.

## What the workflow does

`release.yml` runs on any pushed tag matching `v*`, and by hand (`workflow_dispatch`). It passes
the repository variables `CHROME_STORE_ID`, `EDGE_STORE_ID` and `FIREFOX_STORE_SLUG` to every
job as `SHARD_CHROME_STORE_ID`, `SHARD_EDGE_STORE_ID` and `SHARD_FIREFOX_STORE_SLUG`, so every
binary it builds carries them (see [variables](#variables)).

| Job | Runner | Runs after | What it does |
| --- | --- | --- | --- |
| `version` | `ubuntu-latest` | | Reads the version from `Cargo.toml`. For a tag, fails unless the tag is `v<version>`. |
| `macos` | `macos-14` | `version` | Builds `aarch64-apple-darwin` and `x86_64-apple-darwin` and joins them with `lipo`. If `MACOS_CERTIFICATE` is set, loads it into a temporary keychain and finds its Developer ID Application identity. Bundles `Shard.app` with `scripts/bundle-macos.sh` (signed with that identity and the hardened runtime, otherwise ad hoc). Runs `scripts/test-setup.sh` on the bundle. Installs `create-dmg` with Homebrew and makes the disk image with `scripts/make-dmg.sh`, signing it when there's an identity. Notarises and staples it when there's an identity and `APPLE_ID`. Zips `Shard.app` with `ditto` for updates. |
| `windows` | `windows-2022` | `version` | Builds `shard.exe` (with `AWS_LC_SYS_PREBUILT_NASM=1`), writes the portable zip, the Setup `.exe` and the bare `.exe`, then runs `scripts/test-setup.ps1` on the Setup `.exe`. |
| `linux` | `ubuntu-22.04` (x64), `ubuntu-22.04-arm` (arm64) | `version` | Installs the build libraries with `scripts/linux-deps.sh`, builds, runs `scripts/test-setup.sh` on the binary, then makes the tarball, the `.deb` (cargo-deb), the bare binary, the `.rpm` (cargo-generate-rpm 0.21.1) and the AppImage (appimagetool 1.9.1). |
| `extension` | `ubuntu-latest` | `version` | Packages the Chrome and Firefox zips with `scripts/package-extension.sh` and lints the Firefox build with `web-ext lint --warnings-as-errors` (web-ext 8). |
| `release` | `ubuntu-latest` | all of the above | Only for a tag. Takes the version's section of `CHANGELOG.md` as the release notes, writes `update.txt`, signs it into `update.txt.sig` with `UPDATE_SIGNING_KEY`, writes `SHA256SUMS` over every file, and publishes them all as a GitHub release, with GitHub's generated notes after the changelog's. |
| `packages` | `ubuntu-latest` | `version`, `release` | Only when `release` ran. Writes the Homebrew cask, the Scoop manifest and the winget manifests with `scripts/package-manifests.sh` from the release's files, pushes the cask to `manyeya/homebrew-tap` and the Scoop manifest to `manyeya/scoop-bucket`, and uploads the winget manifests as the `winget-manifests` artifact. The cask's note about Open Anyway is left out when the disk image was notarised. |
| `chrome-web-store`, `edge-add-ons`, `firefox-add-ons` | `ubuntu-latest` | `version`, `release` | Only when `release` ran. Each sends the extension to its store, if that store's credentials are set (see [the browser stores](#the-browser-stores)). |

The setup check each build job runs installs the app with `--setup --silent` (on Windows, the
Setup `.exe` with `--silent`), checks what landed where, installs again over the top as an update
would, then removes it with `--uninstall --silent` and checks it's gone. On macOS and Linux it
works in a scratch home and install folder; on Windows it uses the runner's real
`%LOCALAPPDATA%\Programs\Shard`, Start menu and registry.

`update.txt` is the manifest installed copies read:

```
shard-update 1
version X.Y.Z
macos-universal <size> <sha256> https://github.com/manyeya/shard/releases/download/vX.Y.Z/Shard-X.Y.Z-macos-universal.zip
windows-x64 <size> <sha256> https://github.com/manyeya/shard/releases/download/vX.Y.Z/Shard-X.Y.Z-windows-x64.exe
linux-x64 <size> <sha256> https://github.com/manyeya/shard/releases/download/vX.Y.Z/shard-X.Y.Z-linux-x64
linux-arm64 <size> <sha256> https://github.com/manyeya/shard/releases/download/vX.Y.Z/shard-X.Y.Z-linux-arm64
```

It's signed with:

```sh
openssl pkeyutl -sign -rawin -inkey update-key.pem -in update.txt -out update.txt.sig
```

Installed copies fetch `https://github.com/manyeya/shard/releases/latest/download/update.txt`,
which is the newest non-prerelease, non-draft release. Publishing a release is what makes the
update reach people, within six hours.

## What a release contains

`<arch>` is `x64` or `arm64`.

| File | What it is |
| --- | --- |
| `Shard-X.Y.Z-macos-universal.dmg` | The disk image: `Shard.app` beside an Applications link, on the background in `packaging/macos/`. Opened from there, Shard offers to install itself. |
| `Shard-X.Y.Z-macos-universal.zip` | `Shard.app`, zipped with `ditto`: what installed copies on macOS update from. |
| `Shard-Setup-X.Y.Z-windows-x64.exe` | The Windows installer. It's the same binary as `Shard.exe`: a copy whose file name contains "setup" runs setup when it's opened. |
| `Shard-X.Y.Z-windows-x64.zip` | The portable zip: `Shard.exe`, `README.md`, `LICENSE-MIT` and `LICENSE-APACHE`. It runs where it's unpacked and doesn't offer setup. Scoop installs this. |
| `Shard-X.Y.Z-windows-x64.exe` | The bare binary installed copies on Windows update from. |
| `Shard-X.Y.Z-linux-<arch>.AppImage` | The AppImage. Opened, it offers to install Shard into `~/.local/bin`. |
| `shard_X.Y.Z_amd64.deb`, `shard_X.Y.Z_arm64.deb` | Debian and Ubuntu packages, built by cargo-deb from `[package.metadata.deb]` in `Cargo.toml`. |
| `shard-X.Y.Z-1.x86_64.rpm`, `shard-X.Y.Z-1.aarch64.rpm` | Fedora, RHEL and openSUSE packages, built by cargo-generate-rpm from `[package.metadata.generate-rpm]`. |
| `shard-X.Y.Z-linux-<arch>.tar.gz` | A folder with `shard`, `shard.desktop`, `shard.png` (the 512 px icon), `README.md`, `LICENSE-MIT` and `LICENSE-APACHE`. |
| `shard-X.Y.Z-linux-<arch>` | The bare binary installed copies on Linux update from. |
| `shard-extension-X.Y.Z-chrome.zip` | The extension for the Chrome Web Store and Edge Add-ons. |
| `shard-extension-X.Y.Z-firefox.zip` | The extension for Firefox Add-ons. |
| `update.txt`, `update.txt.sig` | The update manifest and its Ed25519 signature. |
| `SHA256SUMS` | The SHA-256 of every other file in the release. |

## Secrets

Set these under Settings › Secrets and variables › Actions › Secrets, or with
`gh secret set NAME -R manyeya/shard`, which asks for the value so it stays out of your shell
history. Every one except `UPDATE_SIGNING_KEY` is optional: a step that needs one is skipped
without it.

| Secret | Used by | What it does, and what happens without it |
| --- | --- | --- |
| `UPDATE_SIGNING_KEY` | `release` | The Ed25519 private key (PEM) that signs `update.txt`. Required: the release job fails without it. |
| `MACOS_CERTIFICATE` | `macos` | A Developer ID Application certificate as a `.p12`, base64-encoded. With it, `Shard.app` and the disk image are signed with that identity (the app with the hardened runtime). Without it, the app is signed ad hoc and the disk image isn't signed. |
| `MACOS_CERTIFICATE_PASSWORD` | `macos` | The `.p12`'s password. |
| `APPLE_ID` | `macos` | The Apple ID that notarises. Notarising and stapling run only when there's a signing identity and this is set. |
| `APPLE_TEAM_ID` | `macos` | The team ID, for notarising. |
| `APPLE_APP_PASSWORD` | `macos` | An app-specific password for that Apple ID. |
| `TAP_DEPLOY_KEY` | `packages` | A private SSH deploy key with write access to `manyeya/homebrew-tap`. Without it, the cask isn't pushed (the log says "No deploy key for manyeya/homebrew-tap; skipping it"). |
| `BUCKET_DEPLOY_KEY` | `packages` | The same for `manyeya/scoop-bucket` and the Scoop manifest. |
| `CWS_CLIENT_ID` | `chrome-web-store` | The OAuth client ID for the Chrome Web Store API. |
| `CWS_CLIENT_SECRET` | `chrome-web-store` | That client's secret. |
| `CWS_REFRESH_TOKEN` | `chrome-web-store` | A refresh token for the account that owns the Chrome Web Store item. The upload runs only for a tag, and only when this and the `CHROME_STORE_ID` and `CWS_PUBLISHER_ID` variables are set. |
| `EDGE_PRODUCT_ID` | `edge-add-ons` | The extension's product ID in Partner Center (a GUID, not the ID in the listing's URL). |
| `EDGE_CLIENT_ID` | `edge-add-ons` | The Client ID from Partner Center's Publish API page. |
| `EDGE_API_KEY` | `edge-add-ons` | An API key from the same page. The Edge upload runs only for a tag, and only when this is set. |
| `AMO_JWT_ISSUER` | `firefox-add-ons` | The JWT issuer of an addons.mozilla.org API key. The Firefox submission runs only for a tag, and only when this is set. |
| `AMO_JWT_SECRET` | `firefox-add-ons` | That key's JWT secret. |

`CWS_EXTENSION_ID` is no longer read: the Chrome Web Store item is the `CHROME_STORE_ID`
variable. [STORES.md](STORES.md) explains how to get each store's credentials.

To turn the certificate into the secret: `base64 -i DeveloperID.p12 | pbcopy` on macOS.

To make a deploy key:

```sh
ssh-keygen -t ed25519 -N "" -C "shard release" -f tap-deploy-key
```

Add `tap-deploy-key.pub` to the tap repository under Settings › Deploy keys with "Allow write
access", and store `tap-deploy-key` (the private half) as `TAP_DEPLOY_KEY`. Do the same for the
Scoop bucket with `BUCKET_DEPLOY_KEY`. Use a separate key for each repository. Both keys are set
up already; you only need this to replace one.

Without the Apple secrets, Gatekeeper asks before Shard's first launch; without the store
credentials, the extension zips are only published with the release. Neither stops a release.

## Variables

Set these under Settings › Secrets and variables › Actions › Variables, or with `gh variable set`.
They aren't secret: they're the extension's public IDs in each store.

| Variable | Builds read it as | What it is | Also used for |
| --- | --- | --- | --- |
| `CHROME_STORE_ID` | `SHARD_CHROME_STORE_ID` | The 32-letter item ID at the end of `https://chromewebstore.google.com/detail/<id>` | The item the Chrome Web Store upload goes to |
| `EDGE_STORE_ID` | `SHARD_EDGE_STORE_ID` | The ID at the end of `https://microsoftedge.microsoft.com/addons/detail/<id>` | |
| `FIREFOX_STORE_SLUG` | `SHARD_FIREFOX_STORE_SLUG` | The slug at the end of `https://addons.mozilla.org/firefox/addon/<slug>` | |
| `CWS_PUBLISHER_ID` | (not read by builds) | Your Chrome Web Store publisher ID, from the dashboard's Publisher › Settings | The Chrome Web Store upload, which version 2 of its API needs |

```sh
gh variable set CHROME_STORE_ID -R manyeya/shard --body <id>
gh variable set EDGE_STORE_ID -R manyeya/shard --body <id>
gh variable set FIREFOX_STORE_SLUG -R manyeya/shard --body <slug>
```

`src/install.rs` reads them with `option_env!` when Shard is compiled, and the same environment
variables at run time override what was compiled in (for trying a listing without a release).
Setup builds each store's link from them, and only lists browsers whose store has one:

- Chrome, Arc, Brave, Vivaldi, Opera and Chromium use the Chrome Web Store;
- Edge uses Edge Add-ons when `EDGE_STORE_ID` is set, and the Chrome Web Store otherwise;
- Firefox uses Firefox Add-ons.

With none set, setup has no extension step. A change reaches people with the next release's
builds.

## Homebrew, Scoop and winget

### The tap and the bucket

After the release is published, the `packages` job downloads the release's files and runs:

```sh
scripts/package-manifests.sh X.Y.Z dist manifests
```

It writes:

- `manifests/Casks/shard.rb`, the Homebrew cask: the disk image's URL and SHA-256, a livecheck
  on the latest GitHub release, `auto_updates true` (Shard updates itself), macOS Big Sur or
  later, an `uninstall` that quits Shard and removes its launch agent `dev.shard.app`, a `zap`
  that also deletes the data folder and the login entry's plist, and a caveat about opening an
  app Apple hasn't notarised;
- `manifests/bucket/shard.json`, the Scoop manifest: the portable zip's URL and SHA-256, a
  `shard` shim and a Start menu shortcut for `Shard.exe`, and `checkver` and `autoupdate`
  entries;
- `manifests/winget/manifests/m/manyeya/Shard/X.Y.Z/`, the winget manifests (see
  [winget](#winget)).

For each of the tap and the bucket, the job clones the repository with its deploy key, copies
`Casks/` or `bucket/` over, commits as `github-actions[bot]` with the message `shard X.Y.Z`, and
pushes. If there's nothing to commit it says "Already up to date". Both repositories and both
deploy keys already exist.

People install with:

```sh
brew install --cask manyeya/tap/shard
```

```powershell
scoop bucket add manyeya https://github.com/manyeya/scoop-bucket
scoop install shard
```

If a push fails, the job's log shows it. Fix the key and re-run the `packages` job; it writes and
pushes the same files again.

### winget

The workflow doesn't submit to winget. It uploads the manifests as the run's `winget-manifests`
artifact, to go into [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) by pull
request. The package is `manyeya.Shard`: the Setup `.exe`, installed per user, silently with
`--silent`, and matched to the entry setup writes in Settings › Apps (product code `Shard`,
publisher "The Shard Authors").

The first time, and again for each release:

1. Download the artifact:

   ```sh
   gh run list -R manyeya/shard -w Release -L 1      # the run ID
   gh run download <run ID> -R manyeya/shard -n winget-manifests -D winget
   ```

   The manifests are in `winget/manifests/m/manyeya/Shard/X.Y.Z/`.

2. Optionally, on Windows, check them and try the install from them:

   ```powershell
   winget validate --manifest winget\manifests\m\manyeya\Shard\X.Y.Z
   winget settings --enable LocalManifestFiles     # once, as administrator
   winget install --manifest winget\manifests\m\manyeya\Shard\X.Y.Z
   ```

3. Submit them, either with
   [wingetcreate](https://github.com/microsoft/winget-create), which forks winget-pkgs under
   your GitHub account and opens the pull request:

   ```powershell
   wingetcreate submit --token <GitHub token> winget\manifests\m\manyeya\Shard\X.Y.Z
   ```

   or by hand: fork microsoft/winget-pkgs, copy the folder to the same path
   (`manifests/m/manyeya/Shard/X.Y.Z/`) in your fork, and open a pull request.

4. winget-pkgs' checks validate the manifests and run the installer in a sandbox, and a
   moderator merges the pull request. After that, `winget install manyeya.Shard` installs it.

The installer manifest's `ReleaseDate` is the day the `packages` job ran.

## The browser stores

Once the GitHub release is out, three jobs send its extension to the stores whose credentials are
set: `chrome-web-store`, `edge-add-ons` and `firefox-add-ons`. Each runs on its own, so a store
that's down or refuses the update fails only its own job, and never holds back the release or the
other stores. Re-running a store's job fails if that store already has this version. Each store
has to have the extension already: its first submission is made by hand, as
[STORES.md](STORES.md) describes.

| Store | Runs when | What it does |
| --- | --- | --- |
| Chrome Web Store | the variables `CHROME_STORE_ID` and `CWS_PUBLISHER_ID`, and `CWS_REFRESH_TOKEN` | `npx --yes chrome-webstore-upload-cli@4.0.2 --source shard-extension-X.Y.Z-chrome.zip`, which uploads to the item `CHROME_STORE_ID` through version 2 of the Chrome Web Store API (with `CWS_CLIENT_ID`, `CWS_CLIENT_SECRET` and `CWS_REFRESH_TOKEN`) and submits it for review. |
| Edge Add-ons | `EDGE_API_KEY` | Uploads `shard-extension-X.Y.Z-chrome.zip` as the draft package of `EDGE_PRODUCT_ID` through the Edge Add-ons API (with `EDGE_CLIENT_ID` and `EDGE_API_KEY`), waits up to ten minutes for Edge to accept it, then submits the draft with the notes "Shard X.Y.Z" and waits for that too. The job fails if Edge refuses either, or takes longer. |
| Firefox Add-ons | `AMO_JWT_ISSUER` | `npx --yes web-ext@8 sign --channel listed --source-dir firefox --api-key … --api-secret … --approval-timeout 0`, which submits the Firefox build to the listed add-on and doesn't wait for approval. |

Each store reviews the new version before people get it, and browsers then update the extension
from the store themselves.

## Checking the release

Download everything into an empty folder:

```sh
gh release download vX.Y.Z -R manyeya/shard -D shard-X.Y.Z
cd shard-X.Y.Z
```

1. **The files match `SHA256SUMS`.** Every line should say `OK`:

   ```sh
   sha256sum -c SHA256SUMS              # Linux
   shasum -a 256 -c SHA256SUMS          # macOS
   ```

2. **The manifest is signed by the key the app trusts.** With OpenSSL 3, using the public key
   PEM from [SECURITY.md](../SECURITY.md#the-update-signature):

   ```sh
   openssl pkeyutl -verify -rawin -pubin -inkey shard-update.pub.pem -in update.txt -sigfile update.txt.sig
   ```

3. **The manifest is right.** `update.txt` says the new version, has all four platform lines,
   every URL points at this tag, and each size and SHA-256 matches its file.

4. **macOS.** Open the disk image: the window shows the background, with Shard beside the
   Applications link. Open Shard from it (on an unsigned build, System Settings › Privacy &
   Security › Open Anyway first): setup offers to install it. Install, choose Open Shard, and
   check it's in `/Applications` and runs.

5. **Windows.** Run `Shard-Setup-X.Y.Z-windows-x64.exe`: setup installs into
   `%LOCALAPPDATA%\Programs\Shard` with no administrator prompt. Check the Start menu shortcut,
   and that Settings › Apps lists Shard X.Y.Z from The Shard Authors; uninstall it from there.
   Unzip the portable zip and check its `Shard.exe` opens without offering setup.

6. **Linux.** On x64 (and arm64 if you have one):

   ```sh
   chmod +x Shard-X.Y.Z-linux-x64.AppImage && ./Shard-X.Y.Z-linux-x64.AppImage   # offers setup
   sudo apt install ./shard_X.Y.Z_amd64.deb && shard                             # Debian, Ubuntu
   sudo dnf install ./shard-X.Y.Z-1.x86_64.rpm && shard                          # Fedora
   tar xzf shard-X.Y.Z-linux-x64.tar.gz && shard-X.Y.Z-linux-x64/shard           # offers setup
   ```

7. **Homebrew.** On a Mac:

   ```sh
   brew update
   brew install --cask manyeya/tap/shard        # or: brew upgrade --cask shard
   brew info --cask manyeya/tap/shard           # says X.Y.Z
   ```

8. **Scoop.** On Windows:

   ```powershell
   scoop bucket add manyeya https://github.com/manyeya/scoop-bucket   # once
   scoop update; scoop install shard            # or: scoop update shard
   scoop info shard                             # says X.Y.Z
   ```

9. **The stores.** In each configured store's dashboard, the new version is submitted or in
   review. Once it's approved, the listing shows it.

10. **Updates arrive.** On a machine with the previous version, open Limits and choose "Check
    now". It should download the new version and offer "Restart to update", and restarting should
    bring up the new version with your downloads intact.

## A dry run

Run the Release workflow by hand on a branch: Actions › Release › Run workflow, or
`gh workflow run release.yml -R manyeya/shard --ref dev`. It builds and checks every package and
uploads them as the run's artifacts (`macos`, `windows`, `linux-x64`, `linux-arm64`,
`extension`), but skips the store uploads, the `release` job and the `packages` job, which only
run for a tag. Use it to test packaging changes before tagging.

Choose a branch, not a tag: run on a tag, the workflow treats it as a release and publishes it.

## When something goes wrong

- **The `version` job fails.** The tag doesn't match `Cargo.toml`. Delete the tag
  (`git push origin :refs/tags/vX.Y.Z` and `git tag -d vX.Y.Z`), fix the version on `dev`, and
  start the checklist again.
- **A build job fails** (for example `hdiutil` failing inside `create-dmg` even after
  `scripts/make-dmg.sh`'s three tries, or the setup check). Re-run the failed jobs from the run's
  page. Nothing is published, to GitHub or the stores, until every build job and the
  `extension` job have succeeded.
- **A store's job fails.** The release is out, and so is every other store's update. Fix what
  its log says (an expired refresh token, a new permission the store wants justified) and re-run
  just that job. If the store did take the version before failing, re-running is refused; submit
  that version's zip from the release by hand in the store's dashboard instead.
- **The release job fails with "UPDATE_SIGNING_KEY isn't set".** Add the secret, then re-run
  the job.
- **The `packages` job fails.** The release is out; only the tap, the bucket or the winget
  artifact are missing. Fix what the log says and re-run the job.
- **A published release is broken.** Fix it on `dev` and release a new patch version. Don't
  replace assets on an existing release: installed copies may already have read its manifest,
  and the hashes in `update.txt`, the cask and the Scoop manifest must stay true. If you must
  stop a bad update spreading, delete the release or mark it a prerelease, so `releases/latest`
  points at the previous one again.

## The update signing key

Installed copies trust exactly one Ed25519 public key, `PUBLIC_KEY` in `src/update.rs`. The
private half is the `UPDATE_SIGNING_KEY` secret, as PEM text.

### Making a key

```sh
openssl genpkey -algorithm ed25519 -out shard-update-key.pem
```

Store `shard-update-key.pem`'s whole contents (including the `BEGIN` and `END` lines) as the
`UPDATE_SIGNING_KEY` secret, and keep a copy offline, such as in a password manager. If the key
is lost, no installed copy can verify another update: everyone would have to download and install
a new version by hand.

Print the public half in the form `src/update.rs` uses:

```sh
openssl pkey -in shard-update-key.pem -pubout -outform DER | tail -c 32 | xxd -i
```

That gives 32 bytes as `0x..` values to paste into `PUBLIC_KEY`. (The DER encoding of an Ed25519
public key is a 12-byte header followed by the 32-byte key.)

### Rotating the key

An installed copy can only verify manifests signed by the key compiled into it. Changing the
secret alone would leave every installed copy unable to update. Rotate in two releases:

1. Make the new key. Put its public half in `PUBLIC_KEY` and release that version **signed with
   the old key** (leave `UPDATE_SIGNING_KEY` as it is). Installed copies verify it with the old
   key they have, install it, and from then on trust only the new key.
2. Wait until most people have updated. Then replace `UPDATE_SIGNING_KEY` with the new private key
   for the next release.

Shard only ever reads the latest release's manifest, so a copy still running a version from
before step 1 when the step 2 release comes out never sees the step 1 release: it fails to verify
the new manifest and stays where it is until it's updated by hand. The longer the gap between the
two releases, the fewer copies that strands.

If the private key leaks, rotate straight away, and say so in the release notes. Someone holding
it could sign a manifest, though they would still need to get it served from this repository's
latest release (or break the HTTPS connection to github.com) for installed copies to read it.
