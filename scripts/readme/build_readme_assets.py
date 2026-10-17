"""Build the animated README artwork in docs/readme/ (light + dark variants).

GitHub shows README images through <img>, which blocks scripts and every
external fetch. So each SVG here is fully self-contained: SMIL animation, and
the brand fonts (Fraunces, Manrope, JetBrains Mono, all SIL OFL 1.1) subset to
the glyphs that file uses and embedded as base64 WOFF.

Usage:
    python scripts/readme/build_readme_assets.py <fonts-dir>

<fonts-dir> must hold the variable fonts from github.com/google/fonts:
    fraunces.ttf  <- ofl/fraunces/Fraunces[SOFT,WONK,opsz,wght].ttf
    manrope.ttf   <- ofl/manrope/Manrope[wght].ttf
    mono.ttf      <- ofl/jetbrainsmono/JetBrainsMono[wght].ttf
"""

from __future__ import annotations

import base64
import io
import math
import random
import sys
from dataclasses import dataclass, field
from pathlib import Path
from xml.sax.saxutils import escape

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "docs" / "readme"

# Static instances cut from the variable fonts: (file, axis pins).
FONT_SPECS = {
    "display": ("fraunces.ttf", {"wght": 600, "opsz": 72, "SOFT": 0, "WONK": 0}),
    "body": ("manrope.ttf", {"wght": 500}),
    "bold": ("manrope.ttf", {"wght": 700}),
    "mono": ("mono.ttf", {"wght": 500}),
}
FALLBACK = {
    "display": "Georgia, serif",
    "body": "'Segoe UI', system-ui, sans-serif",
    "bold": "'Segoe UI', system-ui, sans-serif",
    "mono": "Consolas, monospace",
}
INSTANCES: dict[str, bytes] = {}
METRICS: dict[str, TTFont] = {}

# Brand palette ("Quiet Ink + Signal Blue"). The pill stays dark in both themes,
# exactly like the app's theme-invariant pill.
BLUE = "#1B8EFF"
CYAN = "#7ED8FF"
CYAN_SOFT = "#A7E8FF"
NAVY = "#0A2A8C"
BLUE_700 = "#0032B8"
LIME_TOP = "#D9F99D"
LIME = "#BEF264"

THEMES = {
    "dark": {
        "bg_top": "#030617",
        "bg_bottom": "#020B33",
        "glow": NAVY,
        "glow_op": 0.75,
        "card": "#0A1233",
        "card2": "#0E1A45",
        "card_edge": "#081440",
        "rim": "#2D63E0",
        "line": "#7ED8FF",
        "line_op": 0.16,
        "text": "#F4F7FF",
        "muted": "#A3B1CF",
        "soft": "#6F7FA3",
        "dot": "#7ED8FF",
        "dot_op": 0.5,
        "lit": CYAN_SOFT,
        "accent_text": CYAN_SOFT,
        "chip": "#0B1640",
        "shadow_op": 0.55,
        "key_top": "#16214A",
        "key_top2": "#101A3D",
        "key_side": "#070D26",
        "tile_from": "#0F2A7A",
        "tile_to": "#123E9E",
        "icon": CYAN_SOFT,
        "strike": "#6F7FA3",
    },
    "light": {
        "bg_top": "#FAFCFF",
        "bg_bottom": "#E8F1FF",
        "glow": CYAN,
        "glow_op": 0.45,
        "card": "#FFFFFF",
        "card2": "#F4F7FC",
        "card_edge": "#C9D6EC",
        "rim": "#8DBBFF",
        "line": "#0A2A8C",
        "line_op": 0.12,
        "text": "#09090B",
        "muted": "#475569",
        "soft": "#71717A",
        "dot": "#1B8EFF",
        "dot_op": 0.42,
        "lit": "#0F5FD7",
        "accent_text": BLUE_700,
        "chip": "#FFFFFF",
        "shadow_op": 0.16,
        "key_top": "#FFFFFF",
        "key_top2": "#F1F4FA",
        "key_side": "#CBD4E3",
        "tile_from": "#E3EEFF",
        "tile_to": "#D3E8FF",
        "icon": "#0F5FD7",
        "strike": "#94A3B8",
    },
}

EASE = "0.22 1 0.36 1"  # the pill's capsule easing (pill.css)

ICONS = {  # lucide (ISC) 24x24 stroke icons
    "shield": ['<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/>', '<path d="m9 12 2 2 4-4"/>'],
    "gpu": ['<path d="M2 21V3"/>', '<path d="M2 5h18a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H2.26"/>', '<path d="M7 17v3a1 1 0 0 0 1 1h5a1 1 0 0 0 1-1v-3"/>', '<circle cx="16" cy="11" r="2"/>', '<circle cx="8" cy="11" r="2"/>'],
    "window": ['<rect x="2" y="4" width="20" height="16" rx="2"/>', '<path d="M10 4v4"/>', '<path d="M2 8h20"/>', '<path d="M6 4v4"/>'],
    "mic": ['<path d="M12 19v3"/>', '<path d="M19 10v2a7 7 0 0 1-14 0v-2"/>', '<rect x="9" y="2" width="6" height="13" rx="3"/>'],
    "pilcrow": ['<path d="M13 4v16"/>', '<path d="M17 4v16"/>', '<path d="M19 4H9.5a4.5 4.5 0 0 0 0 9H13"/>'],
    "book": ['<path d="M10 2v8l3-3 3 3V2"/>', '<path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H19a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6.5a1 1 0 0 1 0-5H20"/>'],
    "sparkles": ['<path d="M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594z"/>', '<path d="M20 2v4"/>', '<path d="M22 4h-4"/>', '<circle cx="4" cy="20" r="2"/>'],
    "branch": ['<path d="M15 6a9 9 0 0 0-9 9V3"/>', '<circle cx="18" cy="6" r="3"/>', '<circle cx="6" cy="18" r="3"/>'],
    "cloud_off": ['<path d="M10.94 5.274A7 7 0 0 1 15.71 10h1.79a4.5 4.5 0 0 1 4.222 6.057"/>', '<path d="M18.796 18.81A4.5 4.5 0 0 1 17.5 19H9A7 7 0 0 1 5.79 5.78"/>', '<path d="m2 2 20 20"/>'],
    "download": ['<path d="M12 15V3"/>', '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>', '<path d="m7 10 5 5 5-5"/>'],
}


# --------------------------------------------------------------------------- fonts


def load_fonts(font_dir: Path) -> None:
    for key, (name, axes) in FONT_SPECS.items():
        font = instancer.instantiateVariableFont(TTFont(font_dir / name), axes)
        buf = io.BytesIO()
        font.save(buf)
        INSTANCES[key] = buf.getvalue()
        METRICS[key] = TTFont(io.BytesIO(INSTANCES[key]))


def measure(font: str, text: str, size: float, spacing: float = 0) -> float:
    """Advance width in px (kerning ignored; close enough for layout)."""
    f = METRICS[font]
    cmap = f.getBestCmap()
    hmtx = f["hmtx"]
    upm = f["head"].unitsPerEm
    width = sum(hmtx[cmap.get(ord(ch), ".notdef")][0] for ch in text)
    return width * size / upm + spacing * max(len(text) - 1, 0)


def font_face(key: str, chars: set[str]) -> str:
    opts = subset.Options()
    opts.flavor = "woff"
    opts.layout_features = ["kern", "liga", "calt"]
    opts.name_IDs = ["*"]
    opts.notdef_outline = True
    font = TTFont(io.BytesIO(INSTANCES[key]))
    sub = subset.Subsetter(opts)
    sub.populate(text="".join(sorted(chars)) + " ")
    sub.subset(font)
    font.flavor = "woff"
    buf = io.BytesIO()
    font.save(buf)
    data = base64.b64encode(buf.getvalue()).decode()
    return f"@font-face{{font-family:vw-{key};src:url(data:font/woff;base64,{data}) format('woff');}}"


# --------------------------------------------------------------------------- svg builder


def n(v: float) -> str:
    """Compact number formatting."""
    s = f"{v:.2f}".rstrip("0").rstrip(".")
    return "0" if s == "-0" else s


@dataclass
class Svg:
    w: int
    h: int
    title: str
    parts: list[str] = field(default_factory=list)
    defs: list[str] = field(default_factory=list)
    chars: dict[str, set[str]] = field(default_factory=lambda: {k: set() for k in FONT_SPECS})

    def add(self, *items: str) -> None:
        self.parts.extend(items)

    def text(
        self,
        x: float,
        y: float,
        s: str,
        font: str = "body",
        size: float = 16,
        fill: str = "#000",
        anchor: str = "start",
        spacing: float = 0,
        extra: str = "",
        inner: str = "",
    ) -> str:
        self.chars[font].update(s)
        ls = f' letter-spacing="{n(spacing)}"' if spacing else ""
        return (
            f'<text x="{n(x)}" y="{n(y)}" font-family="vw-{font}, {FALLBACK[font]}" font-size="{n(size)}" '
            f'fill="{fill}" text-anchor="{anchor}"{ls} {extra}>{escape(s)}{inner}</text>'
        )

    def render(self) -> str:
        faces = "".join(font_face(k, v) for k, v in self.chars.items() if v)
        return (
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{self.w}" height="{self.h}" '
            f'viewBox="0 0 {self.w} {self.h}" fill="none" role="img" aria-label="{escape(self.title)}">'
            f"<title>{escape(self.title)}</title><defs><style>{faces}</style>{''.join(self.defs)}</defs>"
            f"{''.join(self.parts)}</svg>\n"
        )


def anim(attr: str, values: list, times: list[float], dur: float, ease: bool = True, extra: str = "") -> str:
    vals = ";".join(v if isinstance(v, str) else n(v) for v in values)
    kt = ";".join(n(t) if t not in (0, 1) else str(int(t)) for t in times)
    spline = ""
    if ease:
        spline = f' calcMode="spline" keySplines="{";".join([EASE] * (len(values) - 1))}"'
    return (
        f'<animate attributeName="{attr}" values="{vals}" keyTimes="{kt}" dur="{n(dur)}s" '
        f'repeatCount="indefinite"{spline} {extra}/>'
    )


def visible(windows: list[tuple[float, float]], fade: float = 0.015) -> tuple[list[float], list[float]]:
    """keyTimes/values for an opacity track that is 1 inside each window."""
    pts: list[tuple[float, float]] = []
    for s, e in windows:
        if s > 0:
            pts += [(max(s - fade, 0), 0), (s, 1)]
        else:
            pts += [(0, 1)]
        if e < 1:
            pts += [(e, 1), (min(e + fade, 1), 0)]
        else:
            pts += [(1, 1)]
    if pts[0][0] > 0:
        pts.insert(0, (0, 0))
    if pts[-1][0] < 1:
        pts.append((1, pts[-1][1]))
    times, values = [], []
    for t, v in pts:
        if times and t <= times[-1]:
            t = times[-1] + 1e-4
        times.append(round(t, 4))
        values.append(v)
    times[-1] = 1
    return times, values


def opacity_track(windows: list[tuple[float, float]], dur: float, fade: float = 0.015, peak: float = 1) -> str:
    times, values = visible(windows, fade)
    return anim("opacity", [v * peak for v in values], times, dur, ease=False)


def icon(name: str, x: float, y: float, size: float, color: str, width: float = 2) -> str:
    s = size / 24
    body = "".join(ICONS[name])
    return (
        f'<g transform="translate({n(x)} {n(y)}) scale({n(s)})" stroke="{color}" stroke-width="{n(width)}" '
        f'stroke-linecap="round" stroke-linejoin="round" fill="none">{body}</g>'
    )


def logo(x: float, y: float, size: float) -> str:
    """The VoiceWave mark (6 bars) in brand blue -> cyan."""
    s = size / 24
    bars = [(2, 10, 14), (6, 7, 17), (10, 3, 21), (14, 3, 21), (18, 7, 17), (22, 10, 14)]
    paths = "".join(
        f'<path d="M{bx} {y1}V{y2}" stroke="{BLUE if bx < 12 else CYAN}"/>' for bx, y1, y2 in bars
    )
    return (
        f'<g transform="translate({n(x)} {n(y)}) scale({n(s)})" stroke-width="3" stroke-linecap="round">{paths}</g>'
    )


def wrap(font: str, text: str, size: float, width: float) -> list[str]:
    lines, cur = [], ""
    for word in text.split():
        trial = f"{cur} {word}".strip()
        if measure(font, trial, size) <= width or not cur:
            cur = trial
        else:
            lines.append(cur)
            cur = word
    if cur:
        lines.append(cur)
    return lines


# --------------------------------------------------------------------------- shared pieces


@dataclass
class Timeline:
    """Normalized pill timeline: idle -> listening -> transcribing -> inserted -> idle."""

    listen: float
    transcribe: float
    insert: float
    rest: float
    step: float = 0.03


PILL_SIZES = {"idle": (44, 14), "listening": (96, 26), "transcribing": (64, 22), "inserted": (54, 18)}


def pill_defs(svg: Svg, pid: str) -> None:
    svg.defs.append(
        f'<linearGradient id="{pid}-body" x1="0" y1="0" x2="1" y2="1">'
        '<stop offset="0" stop-color="#18181B" stop-opacity="0.97"/><stop offset="1" stop-color="#27272A" stop-opacity="0.98"/></linearGradient>'
        f'<linearGradient id="{pid}-bar" x1="0" y1="0" x2="0" y2="1">'
        f'<stop offset="0" stop-color="{BLUE}"/><stop offset="0.55" stop-color="{CYAN}"/><stop offset="1" stop-color="{CYAN_SOFT}"/></linearGradient>'
        f'<filter id="{pid}-shadow" x="-50%" y="-120%" width="200%" height="340%">'
        '<feDropShadow dx="0" dy="6" stdDeviation="7" flood-color="#000" flood-opacity="0.45"/></filter>'
        f'<filter id="{pid}-glow" x="-60%" y="-200%" width="220%" height="500%"><feGaussianBlur stdDeviation="9"/></filter>'
    )


def pill(svg: Svg, pid: str, cx: float, cy: float, scale: float, tl: Timeline, dur: float) -> str:
    """The real app pill (pill.css geometry), scaled up and animated through one dictation."""
    pill_defs(svg, pid)
    st = tl.step
    times = [0, tl.listen, tl.listen + st, tl.transcribe, tl.transcribe + st, tl.insert, tl.insert + st, tl.rest, tl.rest + st, 1]
    order = ["idle", "idle", "listening", "listening", "transcribing", "transcribing", "inserted", "inserted", "idle", "idle"]
    ws = [PILL_SIZES[s][0] for s in order]
    hs = [PILL_SIZES[s][1] for s in order]
    lift = {"idle": 0, "listening": -6, "transcribing": -5, "inserted": -3}
    ys = [lift[s] - PILL_SIZES[s][1] / 2 for s in order]
    shape = (
        anim("width", ws, times, dur)
        + anim("height", hs, times, dur)
        + anim("x", [-w / 2 for w in ws], times, dur)
        + anim("y", ys, times, dur)
        + anim("rx", [h / 2 for h in hs], times, dur)
    )
    listening = [(tl.listen + st, tl.transcribe)]
    glow = (
        f'<rect x="-48" y="-19" width="96" height="26" rx="13" fill="{BLUE}" filter="url(#{pid}-glow)" opacity="0">'
        f"{opacity_track(listening, dur, peak=0.7)}</rect>"
    )
    body = (
        f'<rect x="-22" y="-7" width="44" height="14" rx="7" fill="url(#{pid}-body)" stroke="#E4E4E7" '
        f'stroke-opacity="0.22" stroke-width="0.6" filter="url(#{pid}-shadow)">{shape}</rect>'
    )
    # Waveform: 12 centred bars, each with its own rhythm under a centre-heavy envelope.
    rng = random.Random(7)
    bars = []
    for i in range(12):
        bx = -22 + i * 4
        env = 0.45 + 0.55 * math.sin(math.pi * (i + 0.5) / 12)
        seq = [max(0.18, min(1, env * rng.uniform(0.25, 1.0))) for _ in range(6)]
        seq.append(seq[0])
        vals = ";".join(f"1 {n(v)}" for v in seq)
        bars.append(
            f'<g transform="translate({n(bx + 1)} -6)"><rect x="-1" y="-8" width="2" height="16" rx="1" fill="url(#{pid}-bar)">'
            f'<animateTransform attributeName="transform" type="scale" values="{vals}" dur="{n(rng.uniform(0.9, 1.5))}s" repeatCount="indefinite"/>'
            "</rect></g>"
        )
    wave = f'<g opacity="0">{"".join(bars)}{opacity_track(listening, dur)}</g>'
    spinner = (
        f'<g opacity="0" transform="translate(0 -5)"><circle r="4.4" stroke="#52525B" stroke-width="1.5"/>'
        f'<circle r="4.4" stroke="{CYAN_SOFT}" stroke-width="1.5" stroke-linecap="round" stroke-dasharray="7 21">'
        '<animateTransform attributeName="transform" type="rotate" values="0;360" dur="0.8s" repeatCount="indefinite"/></circle>'
        f"{opacity_track([(tl.transcribe + st, tl.insert)], dur)}</g>"
    )
    check = (
        f'<g opacity="0" transform="translate(0 -3)"><path d="M-4 0l2.6 2.6L3.8 -3" stroke="{CYAN}" stroke-width="1.6" '
        f'stroke-linecap="round" stroke-linejoin="round"/>{opacity_track([(tl.insert + st, tl.rest)], dur)}</g>'
    )
    return f'<g transform="translate({n(cx)} {n(cy)}) scale({n(scale)})">{glow}{body}{wave}{spinner}{check}</g>'


def status_captions(svg: Svg, cx: float, y: float, tl: Timeline, dur: float, t: dict, items: list[tuple[str, tuple[float, float]]], size: float = 12.5) -> str:
    out = []
    for label, window in items:
        out.append(
            f'<g opacity="0">{svg.text(cx, y, label, "mono", size, t["accent_text"], "middle", spacing=2.2)}'
            f"{opacity_track([window], dur, fade=0.012)}</g>"
        )
    return "".join(out)


def ring_card(svg: Svg, cid: str, x: float, y: float, w: float, h: float, r: float, t: dict, speed: float = 9) -> str:
    """Card with the brand's revolving blue ring: a lit arc orbiting the border.

    The app does this with a rotating conic gradient; SVG has no conic
    gradients, so the arc is a dash travelling round the rect's perimeter.
    """
    perimeter = 2 * (w + h) - 8 * r + 2 * math.pi * r
    svg.defs.append(f'<filter id="{cid}-blur" x="-10%" y="-10%" width="120%" height="120%"><feGaussianBlur stdDeviation="4"/></filter>')
    geo = f'x="{n(x)}" y="{n(y)}" width="{n(w)}" height="{n(h)}" rx="{n(r)}"'
    spin = f'<animate attributeName="stroke-dashoffset" values="0;{n(-perimeter)}" dur="{n(speed)}s" repeatCount="indefinite"/>'

    def arc(frac: float, color: str, width: float, extra: str = "") -> str:
        dash = perimeter * frac
        return (
            f'<rect {geo} stroke="{color}" stroke-width="{n(width)}" stroke-linecap="round" '
            f'stroke-dasharray="{n(dash)} {n(perimeter - dash)}" {extra}>{spin}</rect>'
        )

    return (
        f'<rect {geo} stroke="{BLUE}" stroke-opacity="0.22" stroke-width="1.2"/>'
        + arc(0.24, BLUE, 6, f'filter="url(#{cid}-blur)" opacity="0.85"')
        + arc(0.2, BLUE, 1.8)
        + arc(0.09, CYAN, 2)
    )


def frame(svg: Svg, t: dict, fid: str, glow_at: tuple[float, float, float] | None = None, radius: float = 24) -> None:
    """Rounded backdrop shared by every banner: ink-to-navy (dark) or paper-to-ice (light)."""
    w, h = svg.w, svg.h
    svg.defs.append(
        f'<clipPath id="{fid}-clip"><rect width="{w}" height="{h}" rx="{radius}"/></clipPath>'
        f'<linearGradient id="{fid}-bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{t["bg_top"]}"/>'
        f'<stop offset="1" stop-color="{t["bg_bottom"]}"/></linearGradient>'
    )
    svg.add(f'<g clip-path="url(#{fid}-clip)"><rect width="{w}" height="{h}" fill="url(#{fid}-bg)"/>')
    if glow_at:
        gx, gy, gr = glow_at
        svg.defs.append(
            f'<radialGradient id="{fid}-glow" gradientUnits="userSpaceOnUse" cx="{n(gx)}" cy="{n(gy)}" r="{n(gr)}">'
            f'<stop offset="0" stop-color="{t["glow"]}" stop-opacity="{t["glow_op"]}"/>'
            f'<stop offset="1" stop-color="{t["glow"]}" stop-opacity="0"/></radialGradient>'
        )
        svg.add(f'<rect width="{w}" height="{h}" fill="url(#{fid}-glow)"/>')


def close_frame(svg: Svg, t: dict, radius: float = 24) -> None:
    svg.add(
        "</g>"
        f'<rect x="0.5" y="0.5" width="{svg.w - 1}" height="{svg.h - 1}" rx="{radius - 0.5}" '
        f'stroke="{t["line"]}" stroke-opacity="{t["line_op"]}"/>'
    )


# --------------------------------------------------------------------------- 1. hero


def hero(theme: str) -> Svg:
    t = THEMES[theme]
    svg = Svg(1200, 560, "VoiceWave: private dictation for Windows. Hold Ctrl + Win, speak, and your words appear in any app.")
    dur = 9.0
    tl = Timeline(listen=0.1, transcribe=0.5, insert=0.62, rest=0.9)
    frame(svg, t, "h", glow_at=(600, 470, 620))

    # Perspective dot floor (the website hero's dotted field, laid flat).
    dots = []
    horizon, near = 352, 560
    zs = [1.05 * 1.16**k for k in range(18)]
    for z in zs:
        sy = horizon + 205 / z
        if sy > near + 6:
            continue
        r = max(0.55, 2.5 / z**0.7)
        fade = min(1, (sy - horizon) / 26)
        op = max(0.06, min(0.9, 1.15 / z**0.55)) * fade
        for col in range(-40, 41):
            sx = 600 + col * 78 / z
            if -10 < sx < 1210:
                dots.append(f'<circle cx="{n(sx)}" cy="{n(sy)}" r="{n(r)}" opacity="{n(op)}"/>')
    svg.defs.append(f'<g id="h-dots">{"".join(dots)}</g>')

    # Voice ripples: expanding floor ellipses that light the dots they cross.
    rings = []
    for k in range(3):
        s = tl.listen + 0.04 + k * 0.11
        e = s + 0.36
        rings.append(
            '<ellipse cx="600" cy="372" rx="10" ry="3" stroke="#fff" stroke-width="30" filter="url(#h-soft)" opacity="0">'
            + anim("rx", [10, 10, 860, 860], [0, s, e, 1], dur, ease=False)
            + anim("ry", [3, 3, 240, 240], [0, s, e, 1], dur, ease=False)
            + anim("opacity", [0, 0, 1, 0, 0], [0, s, s + 0.02, e, 1], dur, ease=False)
            + "</ellipse>"
        )
    svg.defs.append(
        '<filter id="h-soft" x="-20%" y="-60%" width="140%" height="220%"><feGaussianBlur stdDeviation="10"/></filter>'
        f'<mask id="h-ripple" maskUnits="userSpaceOnUse" x="0" y="0" width="1200" height="560">{"".join(rings)}</mask>'
        f'<radialGradient id="h-pool" cx="0.5" cy="0.5" r="0.5"><stop offset="0" stop-color="{BLUE}" stop-opacity="0.55"/>'
        f'<stop offset="1" stop-color="{BLUE}" stop-opacity="0"/></radialGradient>'
        f'<linearGradient id="h-horizon" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="{CYAN}" stop-opacity="0"/>'
        f'<stop offset="0.5" stop-color="{CYAN}" stop-opacity="0.55"/><stop offset="1" stop-color="{CYAN}" stop-opacity="0"/></linearGradient>'
    )
    svg.add(
        f'<use href="#h-dots" fill="{t["dot"]}" opacity="{t["dot_op"]}"/>',
        f'<g mask="url(#h-ripple)"><use href="#h-dots" fill="{t["lit"]}" transform="translate(0 0)"/></g>',
        f'<rect x="0" y="{horizon - 1}" width="1200" height="2" fill="url(#h-horizon)"/>',
        '<ellipse cx="600" cy="372" rx="250" ry="46" fill="url(#h-pool)" opacity="0.35">'
        + anim("opacity", [0.35, 0.35, 0.9, 0.9, 0.35, 0.35], [0, tl.listen, tl.listen + 0.04, tl.transcribe, tl.transcribe + 0.06, 1], dur, ease=False)
        + "</ellipse>",
    )

    # Wordmark, headline, subline.
    word = "VoiceWave"
    ww = 30 + 12 + measure("bold", word, 24)
    x0 = 600 - ww / 2
    svg.add(logo(x0, 46, 30), svg.text(x0 + 42, 70, word, "bold", 24, t["text"]))
    svg.add(svg.text(600, 168, "Private Dictation.", "display", 92, t["text"], "middle", spacing=-1.5))
    svg.add(
        svg.text(600, 222, "Hold Ctrl + Win, speak, let go. Your words appear in any app,", "body", 21, t["muted"], "middle"),
        svg.text(600, 252, "transcribed on your own PC and never sent to the cloud.", "body", 21, t["muted"], "middle"),
    )

    svg.add(pill(svg, "hp", 600, 322, 2.5, tl, dur))
    svg.add(
        status_captions(
            svg, 600, 382, tl, dur, t,
            [
                ("HOLD CTRL + WIN", (0, tl.listen)),
                ("LISTENING", (tl.listen + 0.02, tl.transcribe)),
                ("TRANSCRIBING ON YOUR PC", (tl.transcribe + 0.02, tl.insert)),
                ("INSERTED", (tl.insert + 0.02, tl.rest)),
                ("HOLD CTRL + WIN", (tl.rest + 0.02, 1)),
            ],
        )
    )
    close_frame(svg, t)
    return svg


# --------------------------------------------------------------------------- 2. demo


def ortho(yaw: float, pitch: float, roll: float = 0) -> tuple[float, float, float, float, tuple[float, float]]:
    """Orthographic projection of the z=0 plane after yaw (Y), pitch (X), roll (Z).

    Returns the affine (a, b, c, d) for SVG's matrix() plus the screen direction
    of the plane's back face, used to extrude a slab edge.
    """
    cy_, sy_ = math.cos(math.radians(yaw)), math.sin(math.radians(yaw))
    cp, sp = math.cos(math.radians(pitch)), math.sin(math.radians(pitch))
    cr, sr = math.cos(math.radians(roll)), math.sin(math.radians(roll))

    def project(x: float, y: float, z: float) -> tuple[float, float]:
        x, z = x * cy_ + z * sy_, -x * sy_ + z * cy_
        y, z = y * cp - z * sp, y * sp + z * cp
        x, y = x * cr - y * sr, x * sr + y * cr
        return x, y

    ax, ay = project(1, 0, 0)
    bx, by = project(0, 1, 0)
    back = project(0, 0, -1)
    return ax, ay, bx, by, back


def keycap(svg: Svg, x: float, y: float, w: float, label: str, t: dict, press: tuple[float, float], dur: float, glyph: str = "") -> str:
    """A 3D keycap that presses down while the shortcut is held."""
    h, depth = 84, 14
    p0, p1 = press
    down = anim("transform", ["0 0", "0 0", f"0 {depth - 4}", f"0 {depth - 4}", "0 0", "0 0"], [0, p0, p0 + 0.02, p1, p1 + 0.02, 1], dur)
    down = down.replace("<animate ", '<animateTransform type="translate" ').replace('attributeName="transform"', 'attributeName="transform"')
    face = (
        f'<rect x="{n(x)}" y="{n(y)}" width="{n(w)}" height="{h}" rx="16" fill="url(#k-top)" stroke="{t["line"]}" stroke-opacity="{t["line_op"] * 1.6}"/>'
        f'<rect x="{n(x)}" y="{n(y)}" width="{n(w)}" height="{h}" rx="16" stroke="{BLUE}" stroke-width="2" opacity="0">'
        f"{opacity_track([press], dur, fade=0.02)}</rect>"
    )
    if glyph == "win":
        gx, gy, s = x + w / 2 - 15, y + h / 2 - 15, 14
        tiles = "".join(
            f'<rect x="{n(gx + dx)}" y="{n(gy + dy)}" width="{s}" height="{s}" rx="2.5" fill="{t["text"]}"/>'
            for dx in (0, s + 2) for dy in (0, s + 2)
        )
        face += tiles
    else:
        face += svg.text(x + w / 2, y + h / 2 + 9, label, "bold", 25, t["text"], "middle")
    return (
        f'<rect x="{n(x)}" y="{n(y + depth)}" width="{n(w)}" height="{h}" rx="16" fill="{t["key_side"]}"/>'
        f'<rect x="{n(x - 6)}" y="{n(y + depth + 10)}" width="{n(w + 12)}" height="{h - 10}" rx="20" fill="#000" opacity="{n(t["shadow_op"] * 0.5)}" filter="url(#d-shadow)"/>'
        f'<g>{face}{down}</g>'
    )


def demo(theme: str) -> Svg:
    t = THEMES[theme]
    svg = Svg(1200, 640, "Hold Ctrl + Win and speak; VoiceWave types into the app you are using, and spoken commands like new paragraph become formatting.")
    dur = 11.0
    tl = Timeline(listen=0.08, transcribe=0.5, insert=0.6, rest=0.9)
    frame(svg, t, "d", glow_at=(850, 560, 560))
    svg.defs.append(
        '<filter id="d-shadow" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="14"/></filter>'
        f'<linearGradient id="k-top" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{t["key_top"]}"/><stop offset="1" stop-color="{t["key_top2"]}"/></linearGradient>'
    )

    # Left column: the shortcut and what you say.
    press = (tl.listen, tl.transcribe)
    svg.add(svg.text(64, 92, "HOLD TO TALK", "mono", 13, t["accent_text"], spacing=2.4))
    svg.add(keycap(svg, 64, 116, 132, "Ctrl", t, press, dur))
    svg.add(svg.text(222, 170, "+", "body", 30, t["soft"], "middle"))
    svg.add(keycap(svg, 248, 116, 112, "", t, press, dur, glyph="win"))
    svg.add(svg.text(64, 262, "Double-tap for hands-free. Escape cancels.", "body", 16, t["muted"]))

    bx, by, bw, bh = 64, 296, 380, 250
    svg.add(
        f'<rect x="{bx}" y="{by}" width="{bw}" height="{bh}" rx="20" fill="{t["card"]}" stroke="{t["line"]}" stroke-opacity="{t["line_op"]}"/>',
        svg.text(bx + 24, by + 38, "YOU SAY", "mono", 12, t["soft"], spacing=2.2),
    )
    words = ["ship", "the", "build", "tonight", "#new paragraph", "thanks", "everyone"]
    wx, wy = bx + 24, by + 84
    start, span = tl.listen + 0.03, tl.transcribe - tl.listen - 0.08
    for i, raw in enumerate(words):
        cmd = raw.startswith("#")
        label = raw.lstrip("#")
        size = 24
        width = measure("body", label, size) + (26 if cmd else 0)
        if wx + width > bx + bw - 24:
            wx, wy = bx + 24, wy + 50
        appear = start + span * i / len(words)
        track = anim("opacity", [0, 0, 1, 1, 0, 0], [0, appear, appear + 0.02, 0.93, 0.96, 1], dur, ease=False)
        if cmd:
            svg.add(
                f'<g opacity="0"><rect x="{n(wx)}" y="{n(wy - 27)}" width="{n(width)}" height="38" rx="19" fill="url(#d-cmd)"/>'
                f'{svg.text(wx + 13, wy, label, "body", size, "#FFFFFF")}{track}</g>'
            )
        else:
            svg.add(f'<g opacity="0">{svg.text(wx, wy, label, "body", size, t["text"])}{track}</g>')
        wx += width + 9
    svg.defs.append(
        f'<linearGradient id="d-cmd" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="{NAVY}"/>'
        f'<stop offset="0.42" stop-color="#0F5FD7"/><stop offset="1" stop-color="{BLUE}"/></linearGradient>'
    )
    svg.add(svg.text(bx + 24, by + bh - 22, "Spoken commands become formatting.", "body", 14.5, t["soft"]))

    # Right column: an app window in orthographic 3D, receiving the text.
    a, b, c, d, back = ortho(yaw=-24, pitch=14, roll=-2)
    ww, wh = 600, 380
    ox, oy = 548, 92
    m = f"matrix({n(a)} {n(b)} {n(c)} {n(d)} {n(ox)} {n(oy)})"
    # Extrude a slab edge along the plane's back-face direction; the rim
    # brightens toward the front so the thickness reads as lit glass.
    slab = "".join(
        f'<g transform="translate({n(back[0] * k)} {n(back[1] * k)})"><rect width="{ww}" height="{wh}" rx="18" '
        f'fill="{t["card_edge"] if k > 2 else t["rim"]}" transform="{m}"/></g>'
        for k in range(18, 0, -2)
    )
    svg.add(
        f'<g transform="translate(24 40)" opacity="{n(t["shadow_op"])}"><rect width="{ww}" height="{wh}" rx="22" fill="#000" '
        f'transform="{m}" filter="url(#d-shadow)"/></g>',
        slab,
    )
    win: list[str] = [
        f'<rect width="{ww}" height="{wh}" rx="18" fill="{t["card"]}" stroke="{t["line"]}" stroke-opacity="{t["line_op"] * 1.4}"/>',
        f'<path d="M0 18a18 18 0 0 1 18-18h{ww - 36}a18 18 0 0 1 18 18v28H0z" fill="{t["card2"]}"/>',
        f'<path d="M0 46H{ww}" stroke="{t["line"]}" stroke-opacity="{t["line_op"]}"/>',
    ]
    for i, col in enumerate(("#FF5F57", "#FEBC2E", "#28C840")):
        win.append(f'<circle cx="{24 + i * 18}" cy="23" r="5.5" fill="{col}" opacity="0.85"/>')
    win.append(svg.text(ww / 2, 28, "#launch  ·  Team chat", "bold", 14, t["muted"], "middle"))
    # Earlier messages (quiet placeholders).
    for row, (name, line_w) in enumerate((("Maya", 300), ("Dev", 220))):
        yy = 86 + row * 70
        win.append(f'<circle cx="40" cy="{yy}" r="16" fill="url(#d-cmd)" opacity="{0.35 + row * 0.2}"/>')
        win.append(svg.text(68, yy - 4, name, "bold", 15, t["text"]))
        win.append(f'<rect x="68" y="{yy + 6}" width="{line_w}" height="9" rx="4.5" fill="{t["soft"]}" opacity="0.28"/>')
    # Compose box with the typed result.
    cy0 = 236
    win.append(f'<rect x="24" y="{cy0}" width="{ww - 48}" height="{wh - cy0 - 24}" rx="14" fill="{t["card2"]}" stroke="{BLUE}" stroke-opacity="0.45"/>')
    lines = [("Ship the build tonight.", tl.insert + 0.02, tl.insert + 0.12, cy0 + 40), ("Thanks, everyone.", tl.insert + 0.14, tl.insert + 0.2, cy0 + 96)]
    caret_x, caret_y, caret_t = [44, 44], [cy0 + 22, cy0 + 22], [0, tl.insert + 0.02]
    for k, (txt, s, e, ly) in enumerate(lines):
        lw = measure("body", txt, 22)
        cid = f"d-type{k}"
        svg.defs.append(
            f'<clipPath id="{cid}"><rect x="40" y="{ly - 26}" width="0" height="36">'
            + anim("width", [0, 0, lw + 6, lw + 6, 0], [0, s, e, 0.95, 1], dur, ease=False)
            + "</rect></clipPath>"
        )
        win.append(f'<g clip-path="url(#{cid})">{svg.text(44, ly, txt, "body", 22, t["text"])}</g>')
        caret_t += [s, e]
        caret_x += [44, 44 + lw + 3]
        caret_y += [ly - 18, ly - 18]
    caret_t += [0.95, 1]
    caret_x += [caret_x[-1], 44]
    caret_y += [caret_y[-1], cy0 + 22]
    # SMIL needs strictly increasing keyTimes; nudge the jump between lines.
    fixed_t = []
    for tt in caret_t:
        fixed_t.append(tt if not fixed_t or tt > fixed_t[-1] else fixed_t[-1] + 0.001)
    win.append(
        f'<rect x="44" y="{cy0 + 22}" width="2" height="24" rx="1" fill="{BLUE}">'
        + anim("x", caret_x, fixed_t, dur, ease=False)
        + anim("y", caret_y, fixed_t, dur, ease=False)
        + '<animate attributeName="opacity" values="1;1;0;0" keyTimes="0;0.5;0.5;1" dur="1s" repeatCount="indefinite"/></rect>'
    )
    svg.add(f'<g transform="{m}">{"".join(win)}</g>')

    # The pill floats in front of the window, as it does above the taskbar.
    svg.add(pill(svg, "dp", 852, 566, 2.1, tl, dur))
    svg.add(
        status_captions(
            svg, 852, 616, tl, dur, t,
            [
                ("LISTENING", (tl.listen + 0.02, tl.transcribe)),
                ("TRANSCRIBING ON YOUR GPU", (tl.transcribe + 0.02, tl.insert)),
                ("INSERTED AT YOUR CURSOR", (tl.insert + 0.02, tl.rest)),
            ],
            size=12,
        )
    )
    close_frame(svg, t)
    return svg


# --------------------------------------------------------------------------- 3. features

FEATURES = [
    ("shield", "Never leaves your PC", "Speech is transcribed on this machine. No account, no upload. Offline after one model download.", True),
    ("gpu", "Fast on your GPU", "whisper.cpp on Vulkan for NVIDIA, AMD and Intel graphics cards. No CUDA install. CPU everywhere else.", True),
    ("window", "Works in any app", "Editors, browsers, chat, email and terminals. Types directly, with a clipboard fallback.", False),
    ("mic", "Hold or hands-free", "Hold Ctrl + Win while you talk, or double-tap to keep listening. Escape cancels.", False),
    ("pilcrow", "Spoken commands", 'Say "new line", "new paragraph" or "bullet point" and get formatting instead of words.', False),
    ("book", "Dictionary & snippets", "Teach it names and jargon. A spoken trigger inserts your saved text exactly.", False),
    ("sparkles", "AI polish, on-device", "Optional local Qwen2.5 model removes fillers and fixes punctuation. Off by default.", False),
    ("branch", "Free & open source", "Apache-2.0. History, stats, dark mode and signed auto-updates included.", False),
]


def features(theme: str) -> Svg:
    t = THEMES[theme]
    cols, cw, ch, gap, top, left = 4, 270, 262, 20, 24, 30
    rows = math.ceil(len(FEATURES) / cols)
    svg = Svg(1200, top * 2 + rows * ch + (rows - 1) * gap, "VoiceWave features")
    svg.defs.append(
        f'<linearGradient id="f-tile" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{t["tile_from"]}"/>'
        f'<stop offset="1" stop-color="{t["tile_to"]}"/></linearGradient>'
        '<filter id="f-shadow" x="-20%" y="-20%" width="140%" height="160%"><feDropShadow dx="0" dy="6" stdDeviation="10" flood-color="#0A2A8C" flood-opacity="0.10"/></filter>'
    )
    for i, (ic, title, body, flagship) in enumerate(FEATURES):
        r, col = divmod(i, cols)
        x, y = left + col * (cw + gap), top + r * (ch + gap)
        svg.add(
            f'<rect x="{x}" y="{y}" width="{cw}" height="{ch}" rx="20" fill="{t["card"]}" stroke="{t["line"]}" '
            f'stroke-opacity="{t["line_op"]}" filter="url(#f-shadow)"/>'
        )
        if flagship:
            svg.add(ring_card(svg, f"f-ring{i}", x, y, cw, ch, 20, t, speed=8 + i * 2))
        svg.add(
            f'<rect x="{x + 24}" y="{y + 24}" width="52" height="52" rx="15" fill="url(#f-tile)"/>',
            icon(ic, x + 37, y + 37, 26, t["icon"], 1.9),
            svg.text(x + 24, y + 116, title, "display", 23, t["text"], spacing=-0.2),
        )
        for k, line in enumerate(wrap("body", body, 15.5, cw - 48)):
            svg.add(svg.text(x + 24, y + 150 + k * 24, line, "body", 15.5, t["muted"]))
    return svg


# --------------------------------------------------------------------------- 4. privacy


def privacy(theme: str) -> Svg:
    t = THEMES[theme]
    svg = Svg(1200, 360, "Audio and text stay on your PC: microphone, on-device speech engine, your app. Nothing is sent to a cloud service.")
    dur = 4.0
    frame(svg, t, "p", glow_at=(470, 330, 520))
    bx, by, bw, bh = 34, 42, 846, 276
    svg.add(
        f'<rect x="{bx}" y="{by}" width="{bw}" height="{bh}" rx="24" fill="{t["card"]}" fill-opacity="0.55" stroke="{BLUE}" '
        f'stroke-opacity="0.7" stroke-width="1.6" stroke-dasharray="7 7">'
        '<animate attributeName="stroke-dashoffset" values="0;-28" dur="2.4s" repeatCount="indefinite"/></rect>'
    )
    label = "YOUR PC"
    lw = measure("mono", label, 12, 2.2) + 28
    svg.add(
        f'<rect x="{bx + 28}" y="{by - 14}" width="{n(lw)}" height="28" rx="14" fill="url(#p-chip)"/>',
        svg.text(bx + 28 + lw / 2, by + 5, label, "mono", 12, "#FFFFFF", "middle", spacing=2.2),
    )
    svg.defs.append(
        f'<linearGradient id="p-chip" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="{NAVY}"/>'
        f'<stop offset="0.42" stop-color="#0F5FD7"/><stop offset="1" stop-color="{BLUE}"/></linearGradient>'
        f'<linearGradient id="p-tile" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{t["tile_from"]}"/>'
        f'<stop offset="1" stop-color="{t["tile_to"]}"/></linearGradient>'
    )
    nodes = [
        (190, "mic", "Microphone", "Audio stays in memory"),
        (457, "gpu", "Speech engine", "GPU via Vulkan, or CPU"),
        (724, "window", "Your app", "Text at your cursor"),
    ]
    cy = 178
    for k in range(2):
        x1, x2 = nodes[k][0] + 92, nodes[k + 1][0] - 92
        svg.add(f'<path d="M{x1} {cy}H{x2}" stroke="{BLUE}" stroke-opacity="0.35" stroke-width="2" stroke-dasharray="2 6" stroke-linecap="round"/>')
        for j in range(2):
            begin = n(-(j * dur / 2) - k * 0.5)
            svg.add(
                f'<circle r="5" fill="{CYAN}"><animateMotion path="M{x1} {cy}H{x2}" dur="{n(dur / 2)}s" begin="{begin}s" repeatCount="indefinite"/>'
                f'<animate attributeName="opacity" values="0;1;1;0" keyTimes="0;0.15;0.85;1" dur="{n(dur / 2)}s" begin="{begin}s" repeatCount="indefinite"/></circle>'
            )
    for x, ic, title, sub in nodes:
        ring = ring_card(svg, "p-ring", x - 80, cy - 80, 160, 112, 22, t, speed=7) if ic == "gpu" else ""
        svg.add(
            f'<rect x="{x - 80}" y="{cy - 80}" width="160" height="112" rx="22" fill="{t["card"]}" stroke="{t["line"]}" stroke-opacity="{t["line_op"] * 1.5}"/>',
            ring,
            f'<rect x="{x - 26}" y="{cy - 50}" width="52" height="52" rx="15" fill="url(#p-tile)"/>',
            icon(ic, x - 13, cy - 37, 26, t["icon"], 1.9),
            svg.text(x, cy + 64, title, "display", 21, t["text"], "middle"),
            svg.text(x, cy + 90, sub, "body", 14.5, t["muted"], "middle"),
        )
    # Outside the boundary: the cloud path that is never used.
    gx = 1040
    svg.add(
        f'<path d="M{bx + bw} {cy - 30}H{gx - 64}" stroke="{t["soft"]}" stroke-opacity="0.5" stroke-width="2" stroke-dasharray="2 7" stroke-linecap="round"/>',
        f'<circle cx="{bx + bw}" cy="{cy - 30}" r="15" fill="{t["card"]}" stroke="{t["soft"]}" stroke-opacity="0.6"/>',
        f'<path d="M{bx + bw - 5} {cy - 35}l10 10m0 -10l-10 10" stroke="{t["muted"]}" stroke-width="2" stroke-linecap="round"/>',
        icon("cloud_off", gx - 30, cy - 66, 60, t["soft"], 1.4),
        svg.text(gx, cy + 26, "Cloud speech services", "body", 15, t["muted"], "middle"),
        svg.text(gx, cy + 48, "never used", "bold", 15, t["text"], "middle"),
    )
    close_frame(svg, t)
    return svg


# --------------------------------------------------------------------------- 5. polish


def polish(theme: str) -> Svg:
    t = THEMES[theme]
    svg = Svg(1200, 300, "Optional AI polish runs on your PC: it removes filler words and fixes punctuation.")
    dur = 8.0
    frame(svg, t, "q", glow_at=(1000, 300, 520))
    svg.add(ring_card(svg, "q-ring", 24, 24, 1152, 252, 22, t, speed=12))
    svg.add(svg.text(64, 78, "YOU SAID", "mono", 12, t["soft"], spacing=2.2))
    raw = ["so", "um", "I", "think", "we", "should", "uh", "move", "the", "demo", "to", "Friday"]
    fillers = {0, 1, 6}
    x, y, size = 64, 124, 30
    strike_s, strike_e = 0.18, 0.3
    for i, wtxt in enumerate(raw):
        w = measure("body", wtxt, size)
        if i in fillers:
            svg.add(
                f'<g>{svg.text(x, y, wtxt, "body", size, t["accent_text"])}'
                + anim("opacity", [1, 1, 0.35, 0.35, 1], [0, strike_e, strike_e + 0.05, 0.94, 1], dur, ease=False)
                + "</g>"
                f'<path d="M{n(x - 2)} {y - 10}h{n(w + 4)}" stroke="{t["strike"]}" stroke-width="2.5" stroke-linecap="round" '
                f'stroke-dasharray="{n(w + 4)}" stroke-dashoffset="{n(w + 4)}">'
                + anim("stroke-dashoffset", [w + 4, w + 4, 0, 0, w + 4], [0, strike_s, strike_e, 0.94, 1], dur)
                + "</path>"
            )
        else:
            svg.add(svg.text(x, y, wtxt, "body", size, t["text"]))
        x += w + measure("body", " ", size)

    svg.add(f'<path d="M64 156H1136" stroke="{t["line"]}" stroke-opacity="{t["line_op"]}"/>')
    svg.add(svg.text(64, 194, "VOICEWAVE TYPED", "mono", 12, t["soft"], spacing=2.2))
    out = "I think we should move the demo to Friday."
    ow = measure("display", out, 34)
    svg.defs.append(
        '<clipPath id="q-type"><rect x="60" y="200" width="0" height="56">'
        + anim("width", [0, 0, ow + 10, ow + 10, 0], [0, 0.38, 0.62, 0.94, 1], dur, ease=False)
        + "</rect></clipPath>"
    )
    svg.add(f'<g clip-path="url(#q-type)">{svg.text(64, 240, out, "display", 34, t["text"], spacing=-0.3)}</g>')
    chip = "Runs on your PC  ·  Optional"
    cw = measure("bold", chip, 13) + 30
    svg.add(
        f'<rect x="{n(1136 - cw)}" y="58" width="{n(cw)}" height="30" rx="15" fill="url(#q-chip)"/>',
        svg.text(1136 - cw / 2, 78, chip, "bold", 13, "#FFFFFF", "middle"),
    )
    svg.defs.append(
        f'<linearGradient id="q-chip" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="{NAVY}"/>'
        f'<stop offset="0.42" stop-color="#0F5FD7"/><stop offset="1" stop-color="{BLUE}"/></linearGradient>'
    )
    close_frame(svg, t)
    return svg


# --------------------------------------------------------------------------- 6. download button


def download_button() -> Svg:
    """Lime is reserved for download buttons (brand rule)."""
    label = "Download for Windows"
    w, h = 340, 64
    svg = Svg(w, h, label)
    svg.defs.append(
        f'<linearGradient id="b-lime" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{LIME_TOP}"/><stop offset="1" stop-color="{LIME}"/></linearGradient>'
        '<linearGradient id="b-shine" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#fff" stop-opacity="0"/>'
        '<stop offset="0.5" stop-color="#fff" stop-opacity="0.55"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>'
        f'<clipPath id="b-clip"><rect width="{w}" height="{h}" rx="16"/></clipPath>'
    )
    tw = 24 + 12 + measure("bold", label, 20)
    x0 = (w - tw) / 2
    svg.add(
        f'<g clip-path="url(#b-clip)"><rect width="{w}" height="{h}" fill="url(#b-lime)"/>'
        f'<rect x="-120" y="0" width="90" height="{h}" fill="url(#b-shine)" transform="skewX(-20)">'
        f'<animate attributeName="x" values="-160;-160;{w + 60}" keyTimes="0;0.6;1" dur="4s" repeatCount="indefinite"/></rect></g>',
        f'<rect x="0.5" y="0.5" width="{w - 1}" height="{h - 1}" rx="15.5" stroke="#4D7C0F" stroke-opacity="0.25"/>',
        icon("download", x0, h / 2 - 12, 24, "#09090B", 2.2),
        svg.text(x0 + 36, h / 2 + 7, label, "bold", 20, "#09090B"),
    )
    return svg


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    load_fonts(Path(sys.argv[1]))
    OUT.mkdir(parents=True, exist_ok=True)
    builders = {"hero": hero, "demo": demo, "features": features, "privacy": privacy, "polish": polish}
    for name, build in builders.items():
        for theme in THEMES:
            path = OUT / f"{name}-{theme}.svg"
            path.write_text(build(theme).render(), encoding="utf-8")
            print(f"{path.relative_to(ROOT)}  {path.stat().st_size / 1024:.0f} KB")
    path = OUT / "download.svg"
    path.write_text(download_button().render(), encoding="utf-8")
    print(f"{path.relative_to(ROOT)}  {path.stat().st_size / 1024:.0f} KB")


if __name__ == "__main__":
    main()
