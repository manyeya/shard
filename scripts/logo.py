"""Draw Shard's logo, the sliced S, from the display font's own S, and write what's made from it:
assets/logo/shard-mark.svg, shard-mark-mono.svg, shard-icon.svg, and src/logo.rs (the outline as
points, for the title bar's mark and the tray icon). Run it after changing the logo, then
scripts/icons.sh.

Needs fontTools: python3 -m venv .venv && .venv/bin/pip install fonttools
"""

from pathlib import Path

from fontTools.pens.basePen import BasePen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parent.parent
FONT = ROOT / "assets/fonts/ArchivoExpanded-ExtraBold.ttf"

# The S fills y 14..86 of a 100-unit square. It's cut into three bands, the middle slid right.
TOP, HEIGHT = 14.0, 72.0
BANDS = [(0.0, 0.30), (0.37, 0.63), (0.70, 1.0)]  # fractions of the S's height
SHIFT = 7.0  # the middle band, to the right
LIME, LIME_PALE, LIME_DEEP = "#a3e635", "#ecfccb", "#65a30d"


class Flatten(BasePen):
    """The outline as closed polygons, curves cut into short straight pieces."""

    def __init__(self, glyphs, steps=10):
        super().__init__(glyphs)
        self.contours, self.steps = [], steps

    def _moveTo(self, p):
        self.contours.append([p])

    def _lineTo(self, p):
        self.contours[-1].append(p)

    def _qCurveToOne(self, p1, p2):
        p0 = self.contours[-1][-1]
        for i in range(1, self.steps + 1):
            t = i / self.steps
            self.contours[-1].append(
                tuple((1 - t) ** 2 * a + 2 * (1 - t) * t * b + t**2 * c for a, b, c in zip(p0, p1, p2))
            )

    def _curveToOne(self, p1, p2, p3):
        p0 = self.contours[-1][-1]
        for i in range(1, self.steps + 1):
            t = i / self.steps
            self.contours[-1].append(
                tuple(
                    (1 - t) ** 3 * a + 3 * (1 - t) ** 2 * t * b + 3 * (1 - t) * t**2 * c + t**3 * d
                    for a, b, c, d in zip(p0, p1, p2, p3)
                )
            )

    def _closePath(self):
        if len(self.contours[-1]) > 1 and self.contours[-1][0] == self.contours[-1][-1]:
            self.contours[-1].pop()


font = TTFont(FONT)
glyphs = font.getGlyphSet()
glyph = glyphs[font.getBestCmap()[ord("S")]]

# Where the S sits in font units, to fit it to the square: centred, y flipped.
probe = Flatten(glyphs, steps=24)
glyph.draw(probe)
xs = [x for c in probe.contours for x, _ in c]
ys = [y for c in probe.contours for _, y in c]
scale = HEIGHT / (max(ys) - min(ys))
dx = 50 - (min(xs) + max(xs)) / 2 * scale
transform = (scale, 0, 0, -scale, dx, TOP + max(ys) * scale)

svg_pen = SVGPathPen(glyphs, ntos=lambda v: f"{v:.2f}".rstrip("0").rstrip("."))
glyph.draw(TransformPen(svg_pen, transform))
s_path = svg_pen.getCommands()

flat = Flatten(glyphs)
glyph.draw(TransformPen(flat, transform))

band_px = [(TOP + a * HEIGHT, TOP + b * HEIGHT) for a, b in BANDS]


def mark(fill_top, fill_mid, fill_low, ids):
    clips = "".join(
        f'<clipPath id="{ids}{i}"><rect x="-20" y="{a:.2f}" width="140" height="{b - a:.2f}"/></clipPath>'
        for i, (a, b) in enumerate(band_px)
    )
    body = "".join(
        f'<g clip-path="url(#{ids}{i})"{" transform=" + chr(34) + f"translate({SHIFT} 0)" + chr(34) if i == 1 else ""}>'
        f'<path d="{s_path}" fill="{fill}"/></g>'
        for i, fill in enumerate([fill_top, fill_mid, fill_low])
    )
    return clips, body


grad = (
    f'<linearGradient id="shard-lime" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{LIME_PALE}"/>'
    f'<stop offset="1" stop-color="{LIME}"/></linearGradient>'
    f'<linearGradient id="shard-lime-mid" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="{LIME}"/>'
    f'<stop offset="1" stop-color="{LIME_PALE}"/></linearGradient>'
)
note = "<!-- Shard: the display font's S, cut in three, the middle slice slid along like a part mid-download. Made by scripts/logo.py. -->"

# The transparent mark sits on light backgrounds too (browser toolbars), so it's a shade deeper.
deep = grad.replace(LIME_PALE, LIME).replace(f'offset="1" stop-color="{LIME}"', f'offset="1" stop-color="{LIME_DEEP}"')
clips, body = mark("url(#shard-lime)", "url(#shard-lime-mid)", "url(#shard-lime)", "shard-band")
(ROOT / "assets/logo/shard-mark.svg").write_text(
    f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">\n  {note}\n  <defs>{deep}{clips}</defs>\n  {body}\n</svg>\n'
)

clips, body = mark("currentColor", "currentColor", "currentColor", "shard-mono")
(ROOT / "assets/logo/shard-mark-mono.svg").write_text(
    f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="currentColor">\n  {note}\n  <defs>{clips}</defs>\n  {body}\n</svg>\n'
)

clips, body = mark("url(#shard-lime)", "url(#shard-lime-mid)", "url(#shard-lime)", "shard-icon")
(ROOT / "assets/logo/shard-icon.svg").write_text(
    f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  {note}
  <defs>
    <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1a1a1e"/><stop offset="1" stop-color="#0a0a0b"/></linearGradient>
    <radialGradient id="glow" cx="0.5" cy="0.5" r="0.5"><stop offset="0" stop-color="{LIME}" stop-opacity="0.28"/><stop offset="1" stop-color="{LIME}" stop-opacity="0"/></radialGradient>
    {grad}{clips}
  </defs>
  <rect x="100" y="100" width="824" height="824" rx="186" fill="url(#tile)"/>
  <rect x="101.5" y="101.5" width="821" height="821" rx="184.5" fill="none" stroke="#fff" stroke-opacity="0.09" stroke-width="3"/>
  <circle cx="512" cy="512" r="360" fill="url(#glow)"/>
  <g transform="translate(212 212) scale(6)">{body}</g>
</svg>
"""
)

contours = ",\n".join(
    "    &[" + ", ".join(f"({x:.2f}, {y:.2f})" for x, y in c) + "]" for c in flat.contours
)
bands = ", ".join(f"({a:.2f}, {b:.2f})" for a, b in band_px)
(ROOT / "src/logo.rs").write_text(
    f"""//! Shard's mark, the sliced S, as points in a 100-unit square: what the title bar paints and
//! the tray icon rasterises. Generated by scripts/logo.py from the display font; don't edit.

/// The S's outline: closed contours, filled even-odd.
pub const S: &[&[(f32, f32)]] = &[
{contours},
];

/// The three bands the S is cut into, (top, bottom); the middle one is drawn `SHIFT` to the right.
pub const BANDS: [(f32, f32); 3] = [{bands}];
pub const SHIFT: f32 = {SHIFT:.1f};
"""
)
print(f"{len(flat.contours)} contour(s), {sum(len(c) for c in flat.contours)} points")
