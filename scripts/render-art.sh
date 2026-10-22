#!/bin/sh
# Render packaging/art/art.html into the release and store artwork: the disk image's background
# (1x and 2x), the store tiles and screenshots, and GitHub's social preview. The screenshots show
# the site's live console, captured first. Needs agent-browser and python3.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
port=8797
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$root" >/dev/null 2>&1 & server=$!
export AGENT_BROWSER_SESSION=shard-art
trap 'agent-browser close >/dev/null 2>&1 || true; kill $server 2>/dev/null || true' EXIT
sleep 1
base="http://127.0.0.1:$port"
mkdir -p "$root/packaging/macos" "$root/packaging/stores"

# The site's console a few seconds into its simulated download.
agent-browser set viewport 1400 1300 2 >/dev/null
agent-browser open "$base/site/" >/dev/null
# The site only draws what's on screen, so put the console at the top: hide what's above it.
agent-browser eval "for (const el of document.querySelectorAll('nav, .hero')) el.style.display = 'none'; scrollTo(0, 0)" >/dev/null
sleep 7
agent-browser screenshot ".console-wrap .glass" "$root/packaging/art/console.png" >/dev/null

# render <piece> <width> <height> <scale> <file>
render() {
	agent-browser set viewport "$(( $2 > 500 ? $2 : 500 ))" "$3" "$4" >/dev/null
	agent-browser open "$base/packaging/art/art.html?$1" >/dev/null
	sleep 1
	agent-browser screenshot "#$1" "$5" >/dev/null
	echo "$5"
}
render dmg 1000 480 1 "$root/packaging/macos/dmg-background.png"
render dmg 1000 480 2 "$root/packaging/macos/dmg-background@2x.png"
render social 1280 640 1 "$root/packaging/stores/social-preview.png"
render promo-small 440 280 1 "$root/packaging/stores/promo-small.png"
render marquee 1400 560 1 "$root/packaging/stores/promo-marquee.png"
render shot-1 1280 800 1 "$root/packaging/stores/screenshot-1.png"
render shot-2 1280 800 1 "$root/packaging/stores/screenshot-2.png"
