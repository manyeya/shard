#!/bin/sh
# Package the browser extension for the stores: shard-extension-<version>-chrome.zip (Chrome Web
# Store and Edge Add-ons) and shard-extension-<version>-firefox.zip (Firefox Add-ons), from the one
# source in extension/. Firefox runs the same background script as an event page instead of a
# service worker, and needs an add-on id.
#   scripts/package-extension.sh <version> [out-dir]
set -eu
version=${1:?usage: scripts/package-extension.sh <version> [out-dir]}
out=$(mkdir -p "${2:-.}" && cd "${2:-.}" && pwd)
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
for browser in chrome firefox; do
	mkdir -p "$work/$browser"
	cp -R "$root/extension/background.js" "$root/extension/icons" "$work/$browser/"
	python3 - "$root/extension/manifest.json" "$work/$browser/manifest.json" "$version" "$browser" <<'PY'
import json, sys
src, dst, version, browser = sys.argv[1:]
m = json.load(open(src))
m["version"] = version
if browser == "firefox":
    m["background"] = {"scripts": ["background.js"]}
    m.pop("minimum_chrome_version")
    m["browser_specific_settings"] = {"gecko": {
        "id": "shard-integration@manyeya.github.io",
        "strict_min_version": "128.0",
        # What it reads goes to the Shard app on the same computer, and nowhere else.
        "data_collection_permissions": {"required": ["none"]},
    }}
json.dump(m, open(dst, "w"), indent=2)
open(dst, "a").write("\n")
PY
	(cd "$work/$browser" && zip -qrX "$out/shard-extension-$version-$browser.zip" manifest.json background.js icons)
done
echo "$out/shard-extension-$version-chrome.zip"
echo "$out/shard-extension-$version-firefox.zip"
