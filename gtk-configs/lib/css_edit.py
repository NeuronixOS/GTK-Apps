"""Minimal CSS property get/set for known selectors (comment-preserving)."""

from __future__ import annotations

import re

from . import colors_fmt


def _split_selector_key(spec: str) -> tuple[str, str]:
    """'window#waybar|background-color' → selector, prop."""
    if "|" not in spec:
        return "*", spec
    sel, prop = spec.split("|", 1)
    return sel.strip(), prop.strip()


def _find_block(text: str, selector: str) -> tuple[int, int, int] | None:
    """
    Return (block_open_brace, content_start, content_end) for first matching selector block.
    Handles simple selectors; first occurrence wins.
    """
    # Escape and allow flexible whitespace in selector
    parts = re.split(r"\s+", selector.strip())
    sel_re = r"\s+".join(re.escape(p) for p in parts)
    pat = re.compile(rf"(?m)^(?P<sel>{sel_re})\s*\{{", re.MULTILINE)
    m = pat.search(text)
    if not m:
        # try without ^ for nested-ish
        pat = re.compile(rf"(?P<sel>{sel_re})\s*\{{")
        m = pat.search(text)
    if not m:
        return None
    brace = text.find("{", m.start())
    if brace < 0:
        return None
    depth = 0
    i = brace
    while i < len(text):
        c = text[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return brace, brace + 1, i
        i += 1
    return None


def get_prop(text: str, spec: str, *, border: bool = False) -> str | None:
    sel, prop = _split_selector_key(spec)
    loc = _find_block(text, sel)
    if not loc:
        return None
    _, start, end = loc
    body = text[start:end]
    if border and prop.startswith("border"):
        # border-bottom: 2px solid #444444;
        m = re.search(
            rf"(?m)^\s*{re.escape(prop)}\s*:\s*[^;]*?(#[0-9a-fA-F]{{3,8}}|rgba?\([^)]+\))",
            body,
        )
        if m:
            return m.group(1)
        return None
    m = re.search(rf"(?m)^\s*{re.escape(prop)}\s*:\s*(.*?)\s*;", body)
    if not m:
        return None
    return m.group(1).strip()


def set_prop(text: str, spec: str, value: str, *, border: bool = False) -> str:
    sel, prop = _split_selector_key(spec)
    loc = _find_block(text, sel)
    if not loc:
        return text
    brace, start, end = loc
    body = text[start:end]

    if border and prop.startswith("border"):
        # Replace only the color token inside the border-* property
        def repl_line(m: re.Match[str]) -> str:
            line = m.group(0)
            # find color token
            cm = re.search(r"#[0-9a-fA-F]{3,8}|rgba?\([^)]+\)", line)
            if not cm:
                return line
            rgba = colors_fmt.parse_color(value, "auto")
            if rgba is None:
                newc = value
            else:
                newc = colors_fmt.rewrite_token(cm.group(0), rgba)
            return line[: cm.start()] + newc + line[cm.end() :]

        new_body, n = re.subn(
            rf"(?m)^\s*{re.escape(prop)}\s*:\s*[^;]*;",
            repl_line,
            body,
            count=1,
        )
        if n:
            return text[:start] + new_body + text[end:]
        return text

    # Normalize color values when old looks like color
    m = re.search(rf"(?m)^(\s*{re.escape(prop)}\s*:\s*)(.*?)(\s*;)", body)
    if not m:
        # append property before closing
        indent = "  "
        insert = f"{indent}{prop}: {value};\n"
        return text[:end] + insert + text[end:]

    old_val = m.group(2).strip()
    new_val = value
    if colors_fmt.parse_color(old_val, "auto") or colors_fmt.parse_color(value, "auto"):
        rgba = colors_fmt.parse_color(value, "auto")
        if rgba is not None:
            if colors_fmt.parse_color(old_val, "auto"):
                new_val = colors_fmt.rewrite_token(old_val, rgba)
            else:
                new_val = colors_fmt.format_color(rgba, "hex")

    new_body = body[: m.start()] + m.group(1) + new_val + m.group(3) + body[m.end() :]
    return text[:start] + new_body + text[end:]
