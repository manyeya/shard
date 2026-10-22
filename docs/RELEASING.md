# Releasing Shard

Releases are built and published by `.github/workflows/release.yml` when a tag `vX.Y.Z` is pushed.
This is the maintainer's checklist, with what the workflow does, how to check its output, and how
the update signing key and the other secrets work.

## Contents

- [Before you start](#before-you-start)
- [The checklist](#the-checklist)
- [What the workflow does](#what-the-workflow-does)
- [Checking the release](#checking-the-release)
- [A dry run](#a-dry-run)
- [When something goes wrong](#when-something-goes-wrong)
- [The update signing key](#the-update-signing-key)
- [Secrets](#secrets)
- [Homebrew and Scoop](#homebrew-and-scoop)
- [The Chrome Web Store](#the-chrome-web-store)

## Before you start

- Versions follow [semver](https://semver.org). Installed copies compare versions by semver to
  decide whether an update is newer, so a version must always go up.
- `UPDATE_SIGNING_KEY` must be set in the repository's Actions secrets, or the release job fails
  (see [the update signing key](#the-update-signing-key)).
- Everything to release is on `dev`, and CI is green there on macOS, Linux and Windows.

## The checklist

1. **Update the changelog.** In `CHANGELOG.md`, rename `## [Unreleased]` to
   `## [X.Y.Z] - YYYY-MM-DD`, add a new empty `## [Unreleased]` above it, and update the links at
   the bottom:

   ```
   [Unreleased]: https://github.com/manyeya/shard/compare/vX.Y.Z...dev
   [X.Y.Z]: https://github.com/manyeya/shard/compare/vPREVIOUS...vX.Y.Z
   ```

2. **Bump the version** in `Cargo.toml` (`version = "X.Y.Z"` under `[package]`) and in
   `extension/manifest.json` (`"version": "X.Y.Z"`). The workflow sets the extension zip's version
   itself, but keeping the source copy the same means an unpacked extension says the right
   version too.

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

5. **Wait for CI to pass** on all three systems for that commit.

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

8. **Watch the Release workflow** in the Actions tab until it finishes, then
   [check the release](#checking-the-release).

9. **Edit the release notes.** The release is created with GitHub's generated notes (pull
   requests grouped by label, see `.github/release.yml`). Commits pushed straight to `dev`
   without a pull request don't appear there, so paste the changelog section in as well.

10. **Go back to `dev`** (`git switch dev`) for the next change.

## What the workflow does

`release.yml` runs on any pushed tag matching `v*`, and by hand (`workflow_dispatch`).

| Job | Runner | What it makes |
| --- | --- | --- |
| `version` | `ubuntu-latest` | Reads the version from `Cargo.toml`; for a tag, fails unless the tag is `v<version>`. |
| `macos` | `macos-14` | Builds `aarch64` and `x86_64`, joins them with `lipo`, bundles `Shard.app` (signed with the Developer ID if the certificate secrets are set, ad hoc otherwise), makes `Shard-X.Y.Z-macos-universal.dmg` (signed, notarised and stapled when the Apple secrets are set) and the update archive `Shard-X.Y.Z-macos-universal.zip`. |
| `windows` | `windows-2022` | Builds `shard.exe` and packages `Shard-X.Y.Z-windows-x64.zip` (`Shard.exe` and the README) and the bare `Shard-X.Y.Z-windows-x64.exe` that installed copies update from. |
| `linux` | `ubuntu-22.04` and `ubuntu-22.04-arm` | For x64 and arm64: `shard-X.Y.Z-linux-<arch>.tar.gz`, `shard_X.Y.Z_<amd64 or arm64>.deb` (with cargo-deb), and the bare binary `shard-X.Y.Z-linux-<arch>`. |
| `extension` | `ubuntu-latest` | `shard-extension-X.Y.Z.zip`, with the manifest's version set to the release's; uploads it to the Chrome Web Store when that's set up. |
| `release` | `ubuntu-latest` | Only for a tag, after every build job succeeded: writes `update.txt`, signs it into `update.txt.sig` with `UPDATE_SIGNING_KEY`, writes `SHA256SUMS` over every file, and publishes them all as a GitHub release with generated notes. |

The packages being added with the installer (the Windows `Shard-Setup-X.Y.Z-windows-x64.exe`, the
Linux `.rpm` and AppImage, and the Homebrew and Scoop updates) are produced by the same workflow;
check the workflow file for their exact steps.

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

## Checking the release

Download `SHA256SUMS`, `update.txt` and `update.txt.sig` from the release, then:

1. **The manifest is signed by the key the app trusts.** With OpenSSL 3, using the public key
   PEM from [SECURITY.md](../SECURITY.md#the-update-signature):

   ```sh
   openssl pkeyutl -verify -rawin -pubin -inkey shard-update.pub.pem -in update.txt -sigfile update.txt.sig
   ```

2. **The manifest is right.** It says the new version, has all four platform lines, and every
   URL points at this tag.

3. **The files match.** Download the packages and run `sha256sum -c --ignore-missing SHA256SUMS`
   (`shasum -a 256 -c --ignore-missing SHA256SUMS` on macOS). The sizes and hashes in
   `update.txt` should match the update builds.

4. **The packages work.** Open the disk image and launch Shard; on Windows run the Setup `.exe`
   and the zip's `Shard.exe`; install the `.deb` on Ubuntu; load the extension zip unpacked.

5. **Updates arrive.** On a machine with the previous version, open Limits and choose "Check
   now". It should download the new version and offer "Restart to update", and restarting should
   bring up the new version with your downloads intact.

## A dry run

Run the Release workflow by hand (Actions › Release › Run workflow) on any branch. It builds every
package and uploads them as workflow artifacts (`macos`, `windows`, `linux-x64`, `linux-arm64`,
`extension`), but skips the release job and the Chrome Web Store upload, which only run for tags.
Use it to test packaging changes before tagging.

## When something goes wrong

- **The `version` job fails.** The tag doesn't match `Cargo.toml`. Delete the tag
  (`git push origin :refs/tags/vX.Y.Z` and `git tag -d vX.Y.Z`), fix the version on `dev`, and
  start the checklist again.
- **A build job fails** (for example `hdiutil` failing even after its retries). Re-run the failed
  jobs from the workflow run's page. Nothing is published until every build job has succeeded.
- **The release job fails with "UPDATE_SIGNING_KEY isn't set".** Add the secret, then re-run
  the job.
- **A published release is broken.** Fix it on `dev` and release a new patch version. Don't
  replace assets on an existing release: installed copies may already have read its manifest,
  and the hashes in `update.txt` must stay true. If you must stop a bad update spreading, delete
  the release or mark it a prerelease, so `releases/latest` points at the previous one again.

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

## Secrets

Set these under Settings › Secrets and variables › Actions.

| Secret | Required | What it does |
| --- | --- | --- |
| `UPDATE_SIGNING_KEY` | Yes | The Ed25519 private key (PEM) that signs `update.txt`. The release job fails without it. |
| `MACOS_CERTIFICATE` | No | A Developer ID Application certificate as a `.p12`, base64-encoded. With it, `Shard.app` and the disk image are signed with the hardened runtime. |
| `MACOS_CERTIFICATE_PASSWORD` | With the certificate | The `.p12`'s password. |
| `APPLE_ID` | No | The Apple ID used to notarise. Notarising and stapling run only when both the certificate and this are set. |
| `APPLE_TEAM_ID` | With `APPLE_ID` | The team ID. |
| `APPLE_APP_PASSWORD` | With `APPLE_ID` | An app-specific password for that Apple ID. |
| `CWS_EXTENSION_ID` | No | The extension's Chrome Web Store item ID. |
| `CWS_CLIENT_ID`, `CWS_CLIENT_SECRET`, `CWS_REFRESH_TOKEN` | No | OAuth credentials for the Chrome Web Store API. The upload runs only when `CWS_REFRESH_TOKEN` is set, and only for tags. |
| `TAP_DEPLOY_KEY` | For Homebrew (being added) | A private SSH deploy key with write access to `manyeya/homebrew-tap`, so the workflow can push the updated cask. |
| `BUCKET_DEPLOY_KEY` | For Scoop (being added) | A private SSH deploy key with write access to `manyeya/scoop-bucket`, so the workflow can push the updated manifest. |

To turn the certificate into the secret: `base64 -i DeveloperID.p12 | pbcopy` on macOS.

To make a deploy key:

```sh
ssh-keygen -t ed25519 -N "" -C "shard release" -f tap-deploy-key
```

Add `tap-deploy-key.pub` to the tap repository under Settings › Deploy keys with "Allow write
access", and store `tap-deploy-key` (the private half) as `TAP_DEPLOY_KEY`. Do the same for the
Scoop bucket with `BUCKET_DEPLOY_KEY`. Use a separate key for each repository.

Without the Apple secrets, the app is signed ad hoc and Gatekeeper asks before its first launch;
without the Chrome Web Store secrets, the extension is only published as a zip. Neither stops a
release.

### Variables

Added with the installer: setup's extension step opens Shard's page in each browser's store, and
only offers the stores whose ID the build knows. Those IDs are compiled in from
`SHARD_CHROME_STORE_ID`, `SHARD_EDGE_STORE_ID` and `SHARD_FIREFOX_STORE_SLUG`, which the release
workflow passes from the repository's Actions variables (Settings › Secrets and variables ›
Actions › Variables). They aren't secret. Set each one once the extension is listed in that store;
until then, setup skips the extension step and people add the extension by hand.

## Homebrew and Scoop

Being added with the installer: the release workflow updates the Homebrew cask `shard` in
`manyeya/homebrew-tap` and the Scoop manifest `shard` in `manyeya/scoop-bucket` with the new
version, URLs and SHA-256 sums, using the deploy keys above. After a release, check that:

```sh
brew update && brew upgrade --cask shard      # or: brew install --cask manyeya/tap/shard
```

```powershell
scoop update; scoop update shard              # or: scoop bucket add manyeya https://github.com/manyeya/scoop-bucket; scoop install shard
```

install the new version. If a push failed, the workflow run shows it; fix the key and re-run that
job.

## The Chrome Web Store

Optional. When `CWS_REFRESH_TOKEN` and the other `CWS_*` secrets are set, the `extension` job of
a tagged release runs:

```sh
npx --yes chrome-webstore-upload-cli@3 upload --source shard-extension-X.Y.Z.zip --auto-publish
```

which uploads the zip to the item `CWS_EXTENSION_ID` and submits it for review. The item has to
exist first: create it once by uploading a zip by hand in the Chrome Web Store developer
dashboard. The credentials are an OAuth client ID and secret from a Google Cloud project with the
Chrome Web Store API enabled, and a refresh token for the account that owns the item; the
[chrome-webstore-upload](https://github.com/fregante/chrome-webstore-upload-keys) project explains
how to get them.

The store listing isn't live yet, so for now people install the extension unpacked.
