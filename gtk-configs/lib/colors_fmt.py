"""Parse and format colors across Hypr / Fuzzel / Mako / CSS / JSON."""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Literal

ColorKind = Literal["hypr", "fuzzel", "hex", "css_rgba", "auto"]


@dataclass
class RGBA:
    r: float  # 0..1
    g: float
    b: float
    a: float = 1.0

    def to_bytes(self) -> tuple[int, int, int, int]:
        return (
            max(0, min(255, round(self.r * 255))),
            max(0, min(255, round(self.g * 255))),
            max(0, min(255, round(self.b * 255))),
            max(0, min(255, round(self.a * 255))),
        )

    @classmethod
    def from_bytes(cls, r: int, g: int, b: int, a: int = 255) -> RGBA:
        return cls(r / 255.0, g / 255.0, b / 255.0, a / 255.0)

    def hex6(self) -> str:
        r, g, b, _ = self.to_bytes()
        return f"#{r:02x}{g:02x}{b:02x}"

    def hex8(self) -> str:
        r, g, b, a = self.to_bytes()
        return f"#{r:02x}{g:02x}{b:02x}{a:02x}"


_HYPR = re.compile(
    r"rgba?\(\s*([0-9a-fA-F]{6})([0-9a-fA-F]{2})?\s*\)",
    re.IGNORECASE,
)
_HEX = re.compile(r"#([0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})\b")
_FUZZEL = re.compile(r"\b([0-9a-fA-F]{8})\b")
_CSS_RGBA = re.compile(
    r"rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)(?:\s*,\s*([0-9.]+))?\s*\)",
    re.IGNORECASE,
)


def _expand3(h: str) -> str:
    return "".join(c * 2 for c in h)


def parse_color(token: str, kind: ColorKind = "auto") -> RGBA | None:
    s = token.strip()
    if not s:
        return None

    if kind in ("hypr", "auto"):
        m = _HYPR.fullmatch(s) if kind == "hypr" else _HYPR.search(s)
        if m and (kind == "hypr" or s.startswith("rgb")):
            rgb = m.group(1)
            aa = m.group(2) or "ff"
            return RGBA.from_bytes(
                int(rgb[0:2], 16),
                int(rgb[2:4], 16),
                int(rgb[4:6], 16),
                int(aa, 16),
            )

    if kind in ("hex", "auto"):
        m = _HEX.fullmatch(s) if kind == "hex" else (_HEX.fullmatch(s) or None)
        if kind == "auto" and s.startswith("#"):
            m = _HEX.fullmatch(s)
        if m:
            h = m.group(1)
            if len(h) == 3:
                h = _expand3(h)
            if len(h) == 6:
                return RGBA.from_bytes(int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16))
            if len(h) == 8:
                return RGBA.from_bytes(
                    int(h[0:2], 16),
                    int(h[2:4], 16),
                    int(h[4:6], 16),
                    int(h[6:8], 16),
                )

    if kind in ("css_rgba", "auto"):
        m = _CSS_RGBA.fullmatch(s) if kind != "auto" else _CSS_RGBA.fullmatch(s)
        if m:
            a = float(m.group(4)) if m.group(4) is not None else 1.0
            return RGBA.from_bytes(int(m.group(1)), int(m.group(2)), int(m.group(3)), round(a * 255))

    if kind in ("fuzzel", "auto"):
        # bare RRGGBBAA (fuzzel)
        if re.fullmatch(r"[0-9a-fA-F]{8}", s):
            return RGBA.from_bytes(
                int(s[0:2], 16),
                int(s[2:4], 16),
                int(s[4:6], 16),
                int(s[6:8], 16),
            )
        if re.fullmatch(r"[0-9a-fA-F]{6}", s) and kind == "fuzzel":
            return RGBA.from_bytes(int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16))

    # auto: try hex without requiring fullmatch on longer strings
    if kind == "auto" and s.startswith("#"):
        m = _HEX.match(s)
        if m:
            return parse_color(m.group(0), "hex")

    return None


def format_color(rgba: RGBA, kind: ColorKind, *, had_alpha: bool | None = None) -> str:
    r, g, b, a = rgba.to_bytes()
    if kind == "hypr":
        if had_alpha is False or (had_alpha is None and a == 255):
            # Prefer rgba when alpha present; rgb when opaque and caller said no alpha
            if a == 255 and had_alpha is not True:
                return f"rgb({r:02x}{g:02x}{b:02x})"
        return f"rgba({r:02x}{g:02x}{b:02x}{a:02x})"
    if kind == "fuzzel":
        return f"{r:02x}{g:02x}{b:02x}{a:02x}"
    if kind == "css_rgba":
        if a == 255:
            return f"rgb({r}, {g}, {b})"
        return f"rgba({r}, {g}, {b}, {a / 255:.2f})"
    # hex
    if a == 255 and had_alpha is not True:
        return f"#{r:02x}{g:02x}{b:02x}"
    return f"#{r:02x}{g:02x}{b:02x}{a:02x}"


def detect_kind(token: str) -> ColorKind:
    s = token.strip()
    if s.startswith("rgb"):
        if _CSS_RGBA.fullmatch(s) and "," in s:
            return "css_rgba"
        return "hypr"
    if s.startswith("#"):
        return "hex"
    if re.fullmatch(r"[0-9a-fA-F]{8}", s):
        return "fuzzel"
    return "hex"


def rewrite_token(original: str, rgba: RGBA) -> str:
    """Replace a color token keeping its native format family."""
    kind = detect_kind(original)
    had_alpha = None
    if kind == "hypr":
        m = _HYPR.search(original.strip())
        had_alpha = bool(m and (m.group(0).lower().startswith("rgba") or m.group(2)))
    elif kind == "hex":
        m = _HEX.fullmatch(original.strip())
        had_alpha = bool(m and len(m.group(1)) == 8)
    elif kind == "fuzzel":
        had_alpha = True
    return format_color(rgba, kind, had_alpha=had_alpha)


# Patterns for scanning file contents
COLOR_SCAN = re.compile(
    r"""(?P<hypr>rgba?\(\s*[0-9a-fA-F]{6}(?:[0-9a-fA-F]{2})?\s*\))"""
    r"""|(?P<css>rgba?\(\s*\d+\s*,\s*\d+\s*,\s*\d+(?:\s*,\s*[0-9.]+)?\s*\))"""
    r"""|(?P<hex>#[0-9a-fA-F]{3,8})\b"""
    r"""|(?P<markup>color\s*=\s*'#(?:[0-9a-fA-F]{3,8})')""",
    re.IGNORECASE,
)


@dataclass
class ColorHit:
    path_key: str  # relative path string
    start: int
    end: int
    token: str
    kind: ColorKind
    context: str  # nearby line snippet


def scan_text(path_key: str, text: str, *, fuzzel_keys: bool = False) -> list[ColorHit]:
    hits: list[ColorHit] = []
    for m in COLOR_SCAN.finditer(text):
        if m.group("hypr"):
            token = m.group("hypr")
            kind: ColorKind = "hypr"
        elif m.group("css"):
            token = m.group("css")
            kind = "css_rgba"
        elif m.group("markup"):
            # extract #hex from color='#…'
            inner = re.search(r"#[0-9a-fA-F]{3,8}", m.group("markup"), re.I)
            if not inner:
                continue
            token = inner.group(0)
            kind = "hex"
            # adjust span to just the hex
            start = m.start() + inner.start()
            end = m.start() + inner.end()
            line = text.count("\n", 0, start) + 1
            hits.append(
                ColorHit(path_key, start, end, token, kind, f"L{line}: …{token}…")
            )
            continue
        else:
            token = m.group("hex")
            kind = "hex"
        line_no = text.count("\n", 0, m.start()) + 1
        hits.append(
            ColorHit(path_key, m.start(), m.end(), token, kind, f"L{line_no}: {token}")
        )

    if fuzzel_keys:
        # INI color values: key=RRGGBBAA
        for m in re.finditer(
            r"(?m)^([A-Za-z0-9_-]+)\s*=\s*([0-9a-fA-F]{8})\s*$", text
        ):
            token = m.group(2)
            start = m.start(2)
            end = m.end(2)
            # skip if already covered
            if any(h.start <= start < h.end for h in hits):
                continue
            line_no = text.count("\n", 0, start) + 1
            hits.append(
                ColorHit(
                    path_key, start, end, token, "fuzzel", f"L{line_no}: {m.group(1)}={token}"
                )
            )
    return hits
