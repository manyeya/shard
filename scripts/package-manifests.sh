#!/bin/sh
# Write the package-manager manifests for a release, from its files in <dist>:
#   <out>/Casks/shard.rb                         Homebrew cask, for manyeya/homebrew-tap
#   <out>/bucket/shard.json                      Scoop manifest, for manyeya/scoop-bucket
#   <out>/winget/manifests/m/manyeya/Shard/<v>/  winget manifests, for microsoft/winget-pkgs
#   scripts/package-manifests.sh <version> <dist> <out>
set -eu
version=${1:?usage: scripts/package-manifests.sh <version> <dist> <out>}
dist=${2:?}
out=${3:?}
repo=https://github.com/manyeya/shard
url="$repo/releases/download/v$version"
dmg="Shard-$version-macos-universal.dmg"
zip="Shard-$version-windows-x64.zip"
setup="Shard-Setup-$version-windows-x64.exe"
sha() { shasum -a 256 "$dist/$1" | cut -d' ' -f1; }
summary="Download manager that fetches every file in parallel parts and resumes after anything"

mkdir -p "$out/Casks" "$out/bucket"
cat > "$out/Casks/shard.rb" <<RUBY
# Written by scripts/package-manifests.sh in manyeya/shard for each release; edits here are lost.
cask "shard" do
  version "$version"
  sha256 "$(sha "$dmg")"

  url "$url/Shard-#{version}-macos-universal.dmg"
  name "Shard"
  desc "$summary"
  homepage "https://manyeya.github.io/shard/"

  livecheck do
    url :url
    strategy :github_latest
  end

  auto_updates true
  depends_on macos: ">= :big_sur"

  app "Shard.app"

  uninstall launchctl: "dev.shard.app",
            quit:      "dev.shard.app"

  zap trash: [
    "~/Library/Application Support/shard",
    "~/Library/LaunchAgents/dev.shard.app.plist",
  ]

  caveats <<~EOS
    Shard isn't notarised by Apple yet. If macOS won't open it, open System Settings,
    then Privacy & Security, and choose Open Anyway beside Shard.
  EOS
end
RUBY

cat > "$out/bucket/shard.json" <<JSON
{
  "version": "$version",
  "description": "$summary",
  "homepage": "https://manyeya.github.io/shard/",
  "license": "MIT OR Apache-2.0",
  "architecture": {
    "64bit": {
      "url": "$url/$zip",
      "hash": "$(sha "$zip")"
    }
  },
  "bin": [["Shard.exe", "shard"]],
  "shortcuts": [["Shard.exe", "Shard"]],
  "checkver": { "github": "$repo" },
  "autoupdate": {
    "architecture": {
      "64bit": { "url": "$repo/releases/download/v\$version/Shard-\$version-windows-x64.zip" }
    }
  }
}
JSON

w="$out/winget/manifests/m/manyeya/Shard/$version"
mkdir -p "$w"
cat > "$w/manyeya.Shard.yaml" <<YAML
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.version.1.9.0.schema.json
PackageIdentifier: manyeya.Shard
PackageVersion: $version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.9.0
YAML
cat > "$w/manyeya.Shard.installer.yaml" <<YAML
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.installer.1.9.0.schema.json
PackageIdentifier: manyeya.Shard
PackageVersion: $version
InstallerType: exe
Scope: user
InstallModes:
  - interactive
  - silent
InstallerSwitches:
  Silent: --silent
  SilentWithProgress: --silent
UpgradeBehavior: install
ProductCode: Shard
ReleaseDate: $(date -u +%Y-%m-%d)
Installers:
  - Architecture: x64
    InstallerUrl: $url/$setup
    InstallerSha256: $(sha "$setup" | tr 'a-f' 'A-F')
ManifestType: installer
ManifestVersion: 1.9.0
YAML
cat > "$w/manyeya.Shard.locale.en-US.yaml" <<YAML
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.defaultLocale.1.9.0.schema.json
PackageIdentifier: manyeya.Shard
PackageVersion: $version
PackageLocale: en-US
Publisher: The Shard Authors
PublisherUrl: $repo
PublisherSupportUrl: $repo/issues
PackageName: Shard
PackageUrl: https://manyeya.github.io/shard/
License: MIT OR Apache-2.0
LicenseUrl: $repo/blob/main/LICENSE-MIT
ShortDescription: $summary
Description: Shard splits every download into parts fetched over parallel connections, resumes after pauses and crashes, and takes downloads over from the browser extension.
Moniker: shard
Tags:
  - download-manager
  - downloader
  - segmented-download
ReleaseNotesUrl: $repo/releases/tag/v$version
ManifestType: defaultLocale
ManifestVersion: 1.9.0
YAML
find "$out" -type f | sort
