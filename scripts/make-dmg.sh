#!/bin/sh
# Make Shard's disk image: Shard.app beside an Applications link, on the background from
# scripts/render-art.sh (at 1x and 2x). create-dmg has Finder lay the window out itself, which is
# the only way current versions of Finder reliably show the background.
#   scripts/make-dmg.sh <Shard.app> <out.dmg>      needs create-dmg (brew install create-dmg)
set -eu
app=${1:?usage: scripts/make-dmg.sh <Shard.app> <out.dmg>}
out=${2:?usage: scripts/make-dmg.sh <Shard.app> <out.dmg>}
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/src"
cp -R "$app" "$work/src/Shard.app"
tiffutil -cathidpicheck "$root/packaging/macos/dmg-background.png" "$root/packaging/macos/dmg-background@2x.png" -out "$work/background.tiff" >/dev/null
# hdiutil sometimes fails with "Resource busy" on hosted runners; a retry gets past it.
for attempt in 1 2 3; do
	rm -f "$out"
	create-dmg --volname Shard --volicon "$root/assets/icons/Shard.icns" --background "$work/background.tiff" \
		--window-pos 240 160 --window-size 660 420 --icon-size 112 --text-size 13 \
		--icon Shard.app 170 205 --hide-extension Shard.app --app-drop-link 490 205 \
		--no-internet-enable "$out" "$work/src" && break
	echo "create-dmg failed (attempt $attempt); trying again" >&2
	sleep 5
done
[ -f "$out" ]
