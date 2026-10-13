#!/bin/sh
# Render assets/logo/*.svg into every icon the platforms want, straight from the vector at each
# size: assets/icons/Shard.icns (macOS), shard.ico (Windows), png/ (Linux) and extension/icons/.
# The outputs are committed; run this again after changing the logo.
# Needs CHROME (Chrome for Testing or Chromium, used headless to rasterise), python3, and on macOS iconutil.
set -eu
: "${CHROME:?set CHROME to a Chrome for Testing or Chromium binary}"
root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/assets/icons"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$out/png" "$root/extension/icons"

# render <svg> <size> <png>: the SVG drawn at exactly size×size on a transparent background.
# Inline, not an <img>: an image loads after first paint, and the screenshot can beat it. Drawn at
# the page's centre, because sips crops from the centre (it ignores a --cropOffset of 0 0).
render() {
	at=$(((1100 - $2) / 2))
	{
		printf '<html><body style="margin:0"><style>svg{position:absolute;left:%spx;top:%spx;width:%spx;height:%spx}</style>' $at $at "$2" "$2"
		cat "$1"
		printf '</body></html>'
	} > "$work/page.html"
	"$CHROME" --headless --disable-gpu --hide-scrollbars --default-background-color=00000000 \
		--virtual-time-budget=2000 --window-size=1100,1100 --screenshot="$work/full.png" \
		"file://$work/page.html" 2>/dev/null
	sips -c "$2" "$2" "$work/full.png" --out "$3" >/dev/null
}

icon="$root/assets/logo/shard-icon.svg"
mark="$root/assets/logo/shard-mark.svg"

for s in 16 24 32 48 64 128 256 512 1024; do
	render "$icon" $s "$out/png/shard-$s.png"
done
for s in 16 32 48 128; do
	render "$mark" $s "$root/extension/icons/shard-$s.png"
done

# macOS: an .iconset of the standard sizes, packed by iconutil.
set_dir="$work/Shard.iconset"
mkdir -p "$set_dir"
for s in 16 32 128 256 512; do
	cp "$out/png/shard-$s.png" "$set_dir/icon_${s}x${s}.png"
	cp "$out/png/shard-$((s * 2)).png" "$set_dir/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$set_dir" -o "$out/Shard.icns"

# Windows: an .ico whose entries are the PNGs themselves (Vista and later read PNG entries).
python3 - "$out/shard.ico" "$out"/png/shard-16.png "$out"/png/shard-24.png "$out"/png/shard-32.png \
	"$out"/png/shard-48.png "$out"/png/shard-64.png "$out"/png/shard-128.png "$out"/png/shard-256.png <<'EOF'
import struct, sys
out, *files = sys.argv[1:]
blobs = [open(f, "rb").read() for f in files]
sizes = [struct.unpack(">II", b[16:24])[0] for b in blobs]  # width from each PNG's IHDR
offset = 6 + 16 * len(blobs)
head, body = struct.pack("<HHH", 0, 1, len(blobs)), b""
for size, blob in zip(sizes, blobs):
    # Width and height bytes: 0 means 256.
    head += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(blob), offset + len(body))
    body += blob
open(out, "wb").write(head + body)
EOF
echo "icons written to $out and $root/extension/icons"
