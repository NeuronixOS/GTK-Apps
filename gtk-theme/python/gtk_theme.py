"""Shared color profiles for the GTK-Apps suite (Python).

Mirrors the Rust ``gtk-theme`` crate: same profile catalog, same
``~/.config/gtk-apps/theme.toml`` persistence, and chrome CSS so Python
apps stay visually consistent with the Rust suite.

GTK4 helpers: :func:`attach_profile_menu`, :func:`apply_chrome`
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Optional

_PROFILES_PATH = Path(__file__).resolve().parent.parent / "profiles.json"
_DEFAULT_ID = "gruvbox-dark"
_THEME_DIR = Path.home() / ".config" / "gtk-apps"
_THEME_PATH = _THEME_DIR / "theme.toml"
_CUSTOM_PROFILES_PATH = _THEME_DIR / "custom-profiles.json"
# GTK-Apps/gtk-theme/python/gtk_theme.py → suite root is parents[2]
_SUITE_ROOT = Path(__file__).resolve().parent.parent.parent
_THEME_EDITOR_RELEASE = (
    _SUITE_ROOT / "gtk-theme-editor" / "target" / "release" / "gtk-theme-editor"
)
_THEME_EDITOR_DEBUG = (
    _SUITE_ROOT / "gtk-theme-editor" / "target" / "debug" / "gtk-theme-editor"
)

OPEN_THEME_EDITOR_ACTION = "open-theme-editor"
OPEN_THEME_EDITOR_MENU_ACTION = "win.open-theme-editor"

ABOUT_ACTION = "about"
ABOUT_MENU_ACTION = "win.about"
SUITE_WEBSITE = "https://github.com/NeuronixOS/GTK-Apps"
SUITE_WEBSITE_LABEL = "github.com/NeuronixOS/GTK-Apps"
SUITE_AUTHOR = "Created by Kevin Hinds"

_profiles: list["Profile"] | None = None
_chrome_provider = None  # Gtk.CssProvider (4 or 3)
_watchers: list[Callable[["Profile"], None]] = []
_monitor = None
_last_id: str = ""
_custom_cache: list["Profile"] | None = None
_custom_mtime: float | None = None


@dataclass(frozen=True)
class Profile:
    id: str
    name: str
    foreground: str
    background: str
    palette: tuple[str, ...]

    def is_dark(self) -> bool:
        return _relative_luminance(self.background) < 0.45

    def surface_hex(self) -> str:
        return _mix_hex(self.background, self.foreground, 0.10)

    def surface_alt_hex(self) -> str:
        return _mix_hex(self.background, self.foreground, 0.18)

    def accent(self) -> str:
        """Suite accent (ANSI blue / palette[4]) — Adwaita --accent-blue."""
        return self.palette[4] if len(self.palette) > 4 else "#458588"


def builtin_profiles() -> list[Profile]:
    global _profiles
    if _profiles is None:
        data = json.loads(_PROFILES_PATH.read_text(encoding="utf-8"))
        _profiles = [
            Profile(
                id=p["id"],
                name=p["name"],
                foreground=p["foreground"],
                background=p["background"],
                palette=tuple(p["palette"]),
            )
            for p in data
        ]
    return _profiles


def _load_custom_profiles() -> list[Profile]:
    """User-saved profiles from ``~/.config/gtk-apps/custom-profiles.json``."""
    global _custom_cache, _custom_mtime
    try:
        mtime = _CUSTOM_PROFILES_PATH.stat().st_mtime
    except OSError:
        _custom_cache = []
        _custom_mtime = None
        return []
    if _custom_cache is not None and _custom_mtime == mtime:
        return _custom_cache
    try:
        data = json.loads(_CUSTOM_PROFILES_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        data = []
    out: list[Profile] = []
    for p in data:
        try:
            palette = tuple(p.get("palette") or [])
            if len(palette) < 16:
                palette = palette + ("#000000",) * (16 - len(palette))
            out.append(
                Profile(
                    id=p["id"],
                    name=p["name"],
                    foreground=p["foreground"],
                    background=p["background"],
                    palette=palette[:16],
                )
            )
        except (KeyError, TypeError):
            continue
    out.sort(key=lambda p: p.name.lower())
    _custom_cache = out
    _custom_mtime = mtime
    return out


def all_profiles() -> list[Profile]:
    """Built-in profiles followed by user-saved custom profiles."""
    return list(builtin_profiles()) + _load_custom_profiles()


def profile_by_id(profile_id: str) -> Optional[Profile]:
    for profile in all_profiles():
        if profile.id == profile_id:
            return profile
    return None


def default_profile() -> Profile:
    return profile_by_id(_DEFAULT_ID) or builtin_profiles()[0]


def load_theme_id() -> str:
    try:
        text = _THEME_PATH.read_text(encoding="utf-8")
    except OSError:
        return _DEFAULT_ID
    match = re.search(r'(?m)^\s*profile\s*=\s*"([^"]+)"\s*$', text)
    if match and profile_by_id(match.group(1)):
        return match.group(1)
    return _DEFAULT_ID


def save_theme_id(profile_id: str) -> None:
    if profile_by_id(profile_id) is None:
        return
    _THEME_DIR.mkdir(parents=True, exist_ok=True)
    _THEME_PATH.write_text(f'profile = "{profile_id}"\n', encoding="utf-8")


def load_profile() -> Profile:
    return profile_by_id(load_theme_id()) or default_profile()


def theme_editor_path() -> Optional[Path]:
    custom = os.environ.get("GTK_THEME_EDITOR")
    if custom:
        p = Path(custom)
        if p.is_file():
            return p
    for candidate in (_THEME_EDITOR_RELEASE, _THEME_EDITOR_DEBUG):
        if candidate.is_file():
            return candidate
    from shutil import which

    found = which("gtk-theme-editor")
    return Path(found) if found else None


def launch_theme_editor() -> None:
    """Spawn gtk-theme-editor so the user can create/edit a custom profile."""
    import subprocess

    path = theme_editor_path()
    if path is None:
        print("gtk-theme: gtk-theme-editor binary not found", file=sys.stderr)
        return
    try:
        subprocess.Popen([str(path)], start_new_session=True)
    except OSError as e:
        print(f"gtk-theme: failed to launch {path}: {e}", file=sys.stderr)


def chrome_css(profile: Profile) -> str:
    fg = profile.foreground
    bg = profile.background
    surface = profile.surface_hex()
    surface_alt = profile.surface_alt_hex()
    accent = profile.accent()
    accent_red = profile.palette[1] if len(profile.palette) > 1 else "#cc241d"
    border = f"alpha({fg}, 0.18)"
    hover = f"alpha({fg}, 0.10)"
    active = f"alpha({fg}, 0.16)"
    on_accent = "#fbf1c7" if _relative_luminance(accent) < 0.55 else "#1d2021"
    if profile.is_dark():
        wc_icon = f"alpha({fg}, 0.55)"
        wc_icon_hover = _mix_hex(fg, "#ffffff", 0.45)
    else:
        wc_icon = f"alpha({fg}, 0.50)"
        wc_icon_hover = _mix_hex(fg, "#000000", 0.35)

    return f"""
/* Suite accent → Adwaita --accent-blue / blue_3 (palette slot 4). */
@define-color accent_bg_color {accent};
@define-color accent_fg_color {on_accent};
@define-color accent_color {accent};
@define-color theme_selected_bg_color {accent};
@define-color theme_selected_fg_color {on_accent};
@define-color theme_unfocused_selected_bg_color {accent};
@define-color theme_unfocused_selected_fg_color {on_accent};
@define-color blue_3 {accent};

:root {{
  --accent-bg-color: {accent};
  --accent-fg-color: {on_accent};
  --accent-color: {accent};
  --accent-blue: {accent};
  --blue-3: {accent};
}}

window, window.csd, window.solid-csd {{
  background: {bg};
  background-color: {bg};
  background-image: none;
  color: {fg};
}}
window label, window .title {{ color: {fg}; }}

headerbar, headerbar.default-decoration, headerbar:backdrop,
.titlebar, .titlebar:backdrop,
menubar, popovermenubar, .menubar, .toolbar, toolbar,
actionbar, .meld-actionbar {{
  background: {surface};
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-bottom-color: {border};
  box-shadow: none;
}}
headerbar {{ border-bottom: 1px solid {border}; }}
/* gtk-meld file path bars — Adwaita tints ActionBar with accent (reads purple). */
actionbar > revealer > box,
.meld-actionbar > revealer > box {{
  background: {surface};
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-color: {border};
  box-shadow: none;
}}
headerbar *, menubar *, popovermenubar *, .toolbar *, toolbar *,
actionbar *, .meld-actionbar * {{ color: {fg}; }}
headerbar button, menubar button, button.flat, button.image-button {{
  color: {fg};
  background-color: transparent;
  background-image: none;
  border: none;
  box-shadow: none;
}}
headerbar button:hover, button.flat:hover, button.image-button:hover {{
  background-color: {hover};
  color: {fg};
}}

/* Window chrome (min / max / close) — match Rust gtk-theme: muted icons,
 * transparent chrome, hover only brightens the symbolic glyph. */
windowcontrols button,
windowcontrols button.minimize,
windowcontrols button.maximize,
windowcontrols button.close,
windowcontrols button:backdrop,
headerbar windowcontrols button,
.titlebar windowcontrols button {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  border: none;
  box-shadow: none;
  outline: none;
  color: {wc_icon};
}}
windowcontrols button image {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  color: {wc_icon};
  -gtk-icon-filter: none;
  opacity: 1;
}}
windowcontrols button:hover,
windowcontrols button:active,
windowcontrols button:checked,
windowcontrols button.minimize:hover,
windowcontrols button.minimize:active,
windowcontrols button.maximize:hover,
windowcontrols button.maximize:active,
windowcontrols button.close:hover,
windowcontrols button.close:active,
headerbar windowcontrols button:hover,
headerbar windowcontrols button:active,
.titlebar windowcontrols button:hover,
.titlebar windowcontrols button:active {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  border: none;
  box-shadow: none;
  outline: none;
  color: {wc_icon_hover};
}}
windowcontrols button:hover image,
windowcontrols button:active image,
headerbar windowcontrols button:hover image,
headerbar windowcontrols button:active image {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  color: {wc_icon_hover};
  -gtk-icon-filter: none;
  opacity: 1;
}}

button {{
  color: {fg};
  background: {surface_alt};
  background-color: {surface_alt};
  background-image: none;
  border: 1px solid {border};
  box-shadow: none;
}}
button label {{ color: {fg}; }}
button:hover {{
  background: {hover};
  background-color: {hover};
  color: {fg};
}}
button.suggested-action, button.suggested-action:hover {{
  background: {accent};
  background-color: {accent};
  color: {on_accent};
  border-color: {accent};
}}
button.suggested-action label {{ color: {on_accent}; }}
button.destructive-action, button.destructive-action:hover {{
  background: {accent_red};
  background-color: {accent_red};
  color: #ffffff;
  border-color: {accent_red};
}}
button.destructive-action label {{ color: #ffffff; }}

entry, searchentry, spinbutton, textview, textview text {{
  background-color: {bg};
  color: {fg};
  caret-color: {fg};
  border-color: {border};
}}
entry selection,
textview selection,
textview text selection,
.editor-view selection,
.editor-view text selection {{
  background-color: alpha({accent}, 0.45);
  color: {fg};
}}
listview, listbox, treeview, gridview, scrolledwindow {{
  background-color: {bg};
  color: {fg};
}}
listbox row:hover {{ background-color: {hover}; }}
listbox row:selected,
listbox.side-panel row:selected,
.side-panel listbox row:selected,
.navigation-sidebar > row:selected {{
  background-color: alpha({accent}, 0.35);
  color: {fg};
}}
notebook, notebook > stack {{ background-color: {bg}; color: {fg}; }}
notebook > header {{ background-color: {surface}; color: {fg}; }}
notebook tab {{ color: {fg}; }}
notebook tab:checked {{ background-color: {surface_alt}; }}
frame {{ border-color: {border}; color: {fg}; }}

/* Paint only `contents` (rounded). Filling bare `popover` draws a sharp
 * rectangle behind the menu — the bleed past the border. */
popover,
popover.background,
popover.menu {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  border: none;
  box-shadow: none;
}}
popover contents,
popover.menu contents,
.menu {{
  background: {surface};
  background-color: {surface};
  background-image: none;
  color: {fg};
  border: 1px solid {border};
  border-radius: 12px;
  box-shadow: none;
}}
popover > arrow {{
  background: {surface};
  background-color: {surface};
  background-image: none;
  border: none;
}}
popover contents > *,
popover contents scrolledwindow,
popover contents scrolledwindow > viewport,
popover contents viewport,
popover contents listview,
popover contents .view {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  border: none;
  box-shadow: none;
  color: {fg};
}}
popover contents modelbutton,
popover contents label {{
  color: {fg};
}}
popover.menu contents modelbutton,
popover contents modelbutton {{
  padding: 8px 12px;
  min-height: 32px;
  margin: 2px 4px;
  border-radius: 6px;
}}
popover contents modelbutton:hover {{
  background-color: {hover};
  background-image: none;
}}

checkbutton, radiobutton {{ color: {fg}; }}
dropdown, dropdown > button {{ color: {fg}; background-color: {surface_alt}; }}
menuitem {{ color: {fg}; }}
menu {{
  background-color: {surface};
  color: {fg};
  border: 1px solid {border};
  border-radius: 12px;
}}
menuitem:hover {{ background-color: {hover}; }}
scrollbar slider {{ background-color: alpha({fg}, 0.35); }}

/* gtk-meld SourceView + gutters (match suite profile, not stock Adwaita purple) */
textview.meld-monospace-font,
textview.meld-monospace-font text,
.sourceview {{
  background-color: {bg};
  background-image: none;
  color: {fg};
}}
link-map, action-gutter, meld-gutter-line-renderer, .sourcemap-container {{
  background-color: {surface};
  background-image: none;
  color: {fg};
}}
"""


def sourceview_scheme_candidates(profile_id: str) -> list[str]:
    """Preferred GtkSourceView scheme ids for a profile (first available wins)."""
    table = {
        "gruvbox-dark": ["gruvbox-dark", "oblivion", "Adwaita-dark", "classic"],
        "gruvbox-light": ["gruvbox-light", "solarized-light", "Adwaita", "classic"],
        "tokyo-night": ["tokyo-night", "Adwaita-dark", "classic"],
        "tokyo-night-storm": ["tokyo-night", "Adwaita-dark", "classic"],
        "dracula": ["dracula", "Adwaita-dark", "classic"],
        "nord": ["nord", "Adwaita-dark", "classic"],
        "catppuccin-mocha": ["catppuccin-mocha", "Adwaita-dark", "classic"],
        "catppuccin-frappe": ["catppuccin-mocha", "Adwaita-dark", "classic"],
        "catppuccin-latte": ["catppuccin-latte", "Adwaita", "classic"],
        "rose-pine": ["rose-pine", "Adwaita-dark", "classic"],
        "rose-pine-moon": ["rose-pine", "Adwaita-dark", "classic"],
        "rose-pine-dawn": ["rose-pine-dawn", "Adwaita", "classic"],
        "one-dark": ["one-dark", "oblivion", "Adwaita-dark", "classic"],
        "monokai": ["monokai", "oblivion", "Adwaita-dark", "classic"],
        "kanagawa": ["kanagawa", "Adwaita-dark", "classic"],
        "everforest-dark": ["everforest-dark", "Adwaita-dark", "classic"],
        "ayu-dark": ["ayu-dark", "Adwaita-dark", "classic"],
        "ayu-mirage": ["ayu-dark", "Adwaita-dark", "classic"],
        "night-owl": ["night-owl", "Adwaita-dark", "classic"],
        "palenight": ["palenight", "Adwaita-dark", "classic"],
        "material-darker": ["material-darker", "Adwaita-dark", "classic"],
        "cobalt2": ["cobalt", "Adwaita-dark", "classic"],
        "zenburn": ["zenburn", "oblivion", "Adwaita-dark", "classic"],
        "tomorrow-night": ["tomorrow-night", "Adwaita-dark", "classic"],
        "oceanic-next": ["oceanic-next", "Adwaita-dark", "classic"],
        "github-dark": ["github-dark", "Adwaita-dark", "classic"],
        "github-light": ["github-light", "Adwaita", "classic"],
        "solarized-dark": ["solarized-dark", "classic"],
        "solarized-light": ["solarized-light", "classic"],
        "synthwave-84": ["synthwave-84", "dracula", "Adwaita-dark", "classic"],
    }
    if profile_id in table:
        return table[profile_id]
    if any(x in profile_id for x in ("light", "latte", "dawn")):
        return ["Adwaita", "classic"]
    return ["Adwaita-dark", "classic"]


def resolve_sourceview_scheme(profile_id: str, is_available) -> str:
    """Resolve a SourceView scheme id using an availability predicate."""
    for scheme_id in sourceview_scheme_candidates(profile_id):
        if is_available(scheme_id):
            return scheme_id
    return "classic"


def apply_adw_color_scheme(profile: Optional[Profile] = None) -> None:
    """Force libadwaita light/dark to match the suite profile."""
    import gi

    gi.require_version("Adw", "1")
    from gi.repository import Adw

    profile = profile or load_profile()
    mgr = Adw.StyleManager.get_default()
    if profile.is_dark():
        mgr.set_color_scheme(Adw.ColorScheme.FORCE_DARK)
    else:
        mgr.set_color_scheme(Adw.ColorScheme.FORCE_LIGHT)


def meld_chunk_colours(profile: Profile) -> tuple[dict[str, str], dict[str, str]]:
    """Diff chunk fill/line colours derived from a suite profile palette.

    Returns ``(fill_hex_by_name, line_hex_by_name)`` for meld's
    :func:`meld.style.get_common_theme` keys.

    Transparent overlays so syntax text stays readable:
    green = add (insert), red = remove (delete), blue = changed (replace).
    """
    bg = profile.background
    green = profile.palette[2] if len(profile.palette) > 2 else "#98971a"
    red = profile.palette[1] if len(profile.palette) > 1 else "#cc241d"
    # Blue for "changed" — palette index 4 is typically cyan/blue in 16-colour sets.
    blue = profile.palette[4] if len(profile.palette) > 4 else "#458588"
    if len(profile.palette) > 10:
        green = profile.palette[10]
    if len(profile.palette) > 9:
        red = profile.palette[9]
    if len(profile.palette) > 12:
        blue = profile.palette[12]

    def _rgba(c: str, alpha: float) -> str:
        r, g, b = _parse_hex(c)
        return f"rgba({r}, {g}, {b}, {alpha:.2f})"

    fill_map = {
        "insert": _rgba(green, 0.10),
        "delete": _rgba(red, 0.10),
        "conflict": _rgba(red, 0.14),
        "replace": _rgba(blue, 0.10),
        "error": _rgba(blue, 0.14),
        "focus-highlight": profile.foreground,
        "current-chunk-highlight": _rgba(profile.foreground, 0.05),
        "overscroll": _rgba(profile.foreground, 0.04),
        "inline": _rgba(blue, 0.16),
        "dimmed": _mix_hex(bg, profile.foreground, 0.45),
    }
    line_map = {
        "insert": _rgba(green, 0.45),
        "delete": _rgba(red, 0.45),
        "conflict": _rgba(red, 0.55),
        "replace": _rgba(blue, 0.45),
        "error": _rgba(blue, 0.55),
    }
    return fill_map, line_map


def select_theme(
    profile_id: str,
    on_profile: Optional[Callable[[Profile], None]] = None,
    *,
    gtk_version: int = 4,
) -> None:
    profile = profile_by_id(profile_id)
    if profile is None:
        return
    save_theme_id(profile_id)
    if gtk_version >= 4:
        apply_chrome(profile)
    else:
        apply_chrome_gtk3(profile)
    if on_profile:
        on_profile(profile)
    _broadcast(profile, from_file=False)
    sync_desktop_theme(profile)


def sync_desktop_theme(profile: Optional[Profile] = None) -> bool:
    """Force-reset hyprbars + Waybar / mako / fuzzel / system GTK dialog colors."""
    profile = profile or load_profile()
    sync_hyprbars(profile)
    sync_shell_chrome(profile)
    return True


def sync_hyprbars(profile: Optional[Profile] = None) -> bool:
    """Rewrite hyprbars colors to match the GTK theme profile (best-effort).

    Bar / button fill use the elevated surface color (same idea as GTK
    headerbars). Title and glyphs use the profile foreground.
    """
    profile = profile or load_profile()
    return sync_hyprbars_colors(profile.surface_hex(), profile.foreground)


def sync_hyprbars_colors(bar_hex: str, text_hex: str) -> bool:
    bar_rgb = _hex_to_hypr_rgb(bar_hex)
    text_rgb = _hex_to_hypr_rgb(text_hex)
    if not bar_rgb or not text_rgb:
        return False
    # rgb(RRGGBB) → rgba(RRGGBBff)
    bar_rgba = f"rgba({bar_rgb[4:10]}ff)"

    # 1) Persist so the next login / reload keeps the colors.
    path = _hyprland_conf_path()
    if path is not None and path.is_file():
        try:
            original = path.read_text(encoding="utf-8")
        except OSError:
            original = None
        if original is not None:
            out = original
            out = _replace_hypr_assign(out, "bar_color", bar_rgba)
            out = _replace_hypr_assign(out, "col.text", text_rgb)
            out = _replace_hypr_assign(out, "inactive_button_color", bar_rgb)
            out = _rewrite_hyprbars_buttons(out, bar_rgb, text_rgb)
            if out != original:
                try:
                    path.write_text(out, encoding="utf-8")
                except OSError:
                    pass

    # 2) Live update: keywords take effect immediately for bar/title/inactive fill.
    #    Skip during session start — conf write is enough; hyprctl can race plugins.
    if not _theme_session_safe():
        _hyprctl_keyword("plugin:hyprbars:bar_color", bar_rgba)
        _hyprctl_keyword("plugin:hyprbars:col.text", text_rgb)
        _hyprctl_keyword("plugin:hyprbars:inactive_button_color", bar_rgb)
        # 3) Rebuild − □ × buttons from the rewritten hyprbars-button lines.
        _hyprctl_reload()
    return True


def _hyprland_conf_path() -> Optional[Path]:
    xdg = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    for cand in (xdg / "hypr" / "hyprland.conf", Path.home() / "configs" / "hypr" / "hyprland.conf"):
        if cand.is_file():
            return cand
    return None


def _hex_to_hypr_rgb(hex_color: str) -> Optional[str]:
    try:
        r, g, b = _parse_hex(hex_color)
    except Exception:
        return None
    return f"rgb({r:02x}{g:02x}{b:02x})"


def _replace_hypr_assign(text: str, key: str, value: str) -> str:
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    key_eq = f"{key} ="
    key_eq2 = f"{key}="
    for line in lines:
        bare = line.rstrip("\r\n")
        nl = line[len(bare) :]
        trimmed = bare.lstrip()
        indent = bare[: len(bare) - len(trimmed)]
        if trimmed.startswith(key_eq) or trimmed.startswith(key_eq2):
            out.append(f"{indent}{key} = {value}{nl or chr(10)}")
        else:
            out.append(line)
    return "".join(out)


def _rewrite_hyprbars_buttons(text: str, bar_rgb: str, text_rgb: str) -> str:
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    for line in lines:
        bare = line.rstrip("\r\n")
        nl = line[len(bare) :]
        if bare.lstrip().startswith("hyprbars-button"):
            out.append(_rewrite_one_hyprbars_button(bare, bar_rgb, text_rgb) + (nl or "\n"))
        else:
            out.append(line)
    return "".join(out)


def _rewrite_one_hyprbars_button(line: str, bar_rgb: str, text_rgb: str) -> str:
    indent = line[: len(line) - len(line.lstrip())]
    trimmed = line.lstrip()
    if "=" not in trimmed:
        return line
    rest = trimmed.split("=", 1)[1].strip()
    commas = [i for i, c in enumerate(rest) if c == ","]
    if len(commas) < 3:
        return line
    size = rest[commas[0] + 1 : commas[1]].strip()
    icon = rest[commas[1] + 1 : commas[2]].strip()
    after_icon = rest[commas[2] + 1 :].strip()
    lower = after_icon.lower()
    idx = max(lower.rfind(", rgb("), lower.rfind(",rgba("), lower.rfind(",rgb("))
    action = after_icon[:idx].strip() if idx >= 0 else after_icon
    return f"{indent}hyprbars-button = {bar_rgb}, {size}, {icon}, {action}, {text_rgb}"


def _hypr_env() -> dict[str, str]:
    env = os.environ.copy()
    if env.get("HYPRLAND_INSTANCE_SIGNATURE"):
        return env
    runtime = env.get("XDG_RUNTIME_DIR")
    if not runtime:
        return env
    hypr = Path(runtime) / "hypr"
    try:
        dirs = sorted(
            (p for p in hypr.iterdir() if p.is_dir()),
            key=lambda p: p.name,
        )
    except OSError:
        return env
    if dirs:
        env["HYPRLAND_INSTANCE_SIGNATURE"] = dirs[-1].name
    return env


def _hyprctl_keyword(key: str, value: str) -> None:
    if not shutil.which("hyprctl"):
        return
    try:
        subprocess.run(
            ["hyprctl", "keyword", key, value],
            env=_hypr_env(),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
            timeout=5,
        )
    except Exception:
        pass


def _theme_session_safe() -> bool:
    """True during Hyprland session bring-up — avoid reload / process thrash."""
    return bool(os.environ.get("NEURONIX_THEME_NO_HYPR_RELOAD"))


def _hyprctl_reload() -> None:
    # Session start sets NEURONIX_THEME_NO_HYPR_RELOAD so we never `hyprctl reload`
    # while plugins/portals are still coming up (that freezes Hyprland on re-login).
    if _theme_session_safe():
        return
    if not shutil.which("hyprctl"):
        return
    try:
        subprocess.run(
            ["hyprctl", "reload"],
            env=_hypr_env(),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
            timeout=5,
        )
    except Exception:
        pass


def sync_shell_chrome(profile: Optional[Profile] = None) -> bool:
    """Restyle Waybar / mako / fuzzel / Hyprland / system GTK dialogs to the profile."""
    profile = profile or load_profile()
    bg = profile.background
    fg = profile.foreground
    border = profile.surface_alt_hex()
    surface = profile.surface_hex()
    accent = profile.accent()
    _sync_waybar_style(bg, fg, border, surface)
    _sync_mako_colors(bg, fg, border)
    _sync_fuzzel_colors(bg, fg, border, surface, accent)
    _sync_hypr_window_borders(border, surface)
    _sync_gtk_user_css(profile)
    # On profile change: drop cached GTK dialogs / portal. During session start
    # (NEURONIX_THEME_NO_HYPR_RELOAD) skip — portal/waybar bring-up already races.
    if not _theme_session_safe():
        _restart_system_dialogs()
    return True


def _restart_system_dialogs() -> None:
    """Drop cached GTK dialog processes so the next open loads fresh user CSS."""
    relaunch_power = False
    if shutil.which("pgrep"):
        try:
            relaunch_power = (
                subprocess.run(
                    ["pgrep", "-f", "xfce4-power-manager-settings"],
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    check=False,
                    timeout=3,
                ).returncode
                == 0
            )
        except Exception:
            relaunch_power = False
    if not shutil.which("pkill"):
        return
    for pattern in (
        ["-x", "zenity"],
        ["-f", "xdg-desktop-portal-gtk"],
        ["-f", "xfce4-power-manager-settings"],
    ):
        try:
            subprocess.run(
                ["pkill", *pattern],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
                timeout=3,
            )
        except Exception:
            pass
    # Prefer a clean user-unit restart when available (auto-respawns the portal).
    if shutil.which("systemctl"):
        try:
            subprocess.run(
                ["systemctl", "--user", "restart", "xdg-desktop-portal-gtk.service"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
                timeout=8,
            )
        except Exception:
            pass
    # XFCE Power Manager settings caches GTK3 CSS — reopen if it was up.
    if relaunch_power and shutil.which("xfce4-power-manager-settings"):
        try:
            subprocess.Popen(
                ["xfce4-power-manager-settings"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
        except Exception:
            pass


_GTK_USER_CSS_BEGIN = "/* BEGIN gtk-theme:shell-chrome */"
_GTK_USER_CSS_END = "/* END gtk-theme:shell-chrome */"


def _system_dialog_css(profile: Profile) -> str:
    bg = profile.background
    fg = profile.foreground
    surface = profile.surface_hex()
    border = profile.surface_alt_hex()
    accent = profile.accent()
    on_accent = "#fbf1c7" if _relative_luminance(accent) < 0.55 else "#1d2021"
    hover = _mix_hex(bg, fg, 0.12)
    dim = _mix_hex(fg, bg, 0.35)
    return f"""{_GTK_USER_CSS_BEGIN}
/* Suite profile → zenity / FileChooser / XFCE Power Manager / GTK3 settings */
@define-color theme_bg_color {bg};
@define-color theme_fg_color {fg};
@define-color theme_base_color {bg};
@define-color theme_text_color {fg};
@define-color theme_selected_bg_color {accent};
@define-color theme_selected_fg_color {on_accent};
@define-color theme_unfocused_selected_bg_color {accent};
@define-color theme_unfocused_selected_fg_color {on_accent};
@define-color insensitive_bg_color {surface};
@define-color insensitive_fg_color {dim};
@define-color insensitive_base_color {surface};
@define-color borders {border};
@define-color unfocused_borders {border};
@define-color accent_bg_color {accent};
@define-color accent_fg_color {on_accent};
@define-color accent_color {accent};
@define-color blue_3 {accent};

/* Exclude DING desktop window — opaque chrome would cover the wallpaper */
window:not(.desktopwindow),
window.background:not(.desktopwindow),
dialog, messagedialog,
.background:not(.desktopwindow) {{
  background-color: {bg};
  background-image: none;
  color: {fg};
}}
window.desktopwindow, window.desktopwindow.background {{
  background-color: transparent;
  background-image: none;
}}
headerbar, headerbar.default-decoration, .titlebar,
headerbar:backdrop, .titlebar:backdrop {{
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-bottom: 1px solid {border};
  box-shadow: none;
}}
headerbar *, .titlebar * {{ color: {fg}; }}
button {{
  color: {fg};
  background-color: {hover};
  background-image: none;
  border-color: {border};
}}
button:hover {{
  background-color: {border};
}}
button:checked, button.suggested-action, button.suggested-action:hover,
button.suggested-action:active {{
  background-color: {accent};
  background-image: none;
  color: {on_accent};
  border-color: {accent};
}}
button.suggested-action label, button:checked label {{ color: {on_accent}; }}
*:selected, treeview:selected, treeview.view:selected,
treeview.view:selected:focus, list row:selected, listview row:selected,
.view:selected, .content-view:selected {{
  background-color: {accent};
  color: {on_accent};
}}
placessidebar, placessidebar list, placessidebar row,
.sidebar, stacksidebar {{
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-color: {border};
}}
placessidebar row:selected, .sidebar row:selected {{
  background-color: {accent};
  color: {on_accent};
}}
entry, searchbar entry, spinbutton, spinbutton entry {{
  background-color: {bg};
  color: {fg};
  border-color: {border};
}}
filechooser, .filechooser, notebook, paned, frame, frame > border,
scrolledwindow, viewport, .view, textview, textview text {{
  background-color: {bg};
  background-image: none;
  color: {fg};
}}
/* XFCE Power Manager + other GTK3 settings windows */
notebook > header {{
  background-color: {surface};
  background-image: none;
  border-color: {border};
}}
notebook > header > tabs > tab {{
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-color: {border};
  box-shadow: none;
}}
notebook > header > tabs > tab:hover {{
  background-color: {hover};
}}
notebook > header > tabs > tab:checked {{
  background-color: {bg};
  color: {fg};
  box-shadow: inset 0 -2px {accent};
}}
notebook > stack {{
  background-color: {bg};
  color: {fg};
}}
treeview, treeview.view, list, list row {{
  background-color: {bg};
  color: {fg};
}}
treeview header button {{
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-color: {border};
}}
label, .label {{ color: {fg}; }}
separator {{ background-color: {border}; }}
scale trough {{
  background-color: {border};
  background-image: none;
}}
scale highlight {{
  background-color: {accent};
  background-image: none;
}}
scale slider {{
  background-color: {accent};
  background-image: none;
  border-color: {accent};
}}
switch {{
  background-color: {border};
  background-image: none;
  border-color: {border};
}}
switch:checked {{
  background-color: {accent};
  border-color: {accent};
}}
switch slider {{
  background-color: {fg};
  background-image: none;
}}
check, radio {{
  background-color: {hover};
  border-color: {border};
  color: {on_accent};
}}
check:checked, radio:checked {{
  background-color: {accent};
  border-color: {accent};
  color: {on_accent};
}}
progressbar trough {{
  background-color: {border};
}}
progressbar progress {{
  background-color: {accent};
}}
combobox button.combo, combobox menu {{
  background-color: {hover};
  color: {fg};
  border-color: {border};
}}
menubar, menu, popover, popover.background {{
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-color: {border};
}}
menuitem:hover, modelbutton:hover {{
  background-color: {accent};
  color: {on_accent};
}}
scrollbar, scrollbar contents, scrollbar trough {{
  background-color: {bg};
}}
scrollbar slider {{
  background-color: {border};
}}
toolbar, .primary-toolbar, .inline-toolbar {{
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-color: {border};
}}
{_GTK_USER_CSS_END}
"""


def _upsert_managed_css_block(existing: str, block: str) -> str:
    start = existing.find(_GTK_USER_CSS_BEGIN)
    end = existing.find(_GTK_USER_CSS_END)
    if start >= 0 and end >= 0:
        end += len(_GTK_USER_CSS_END)
        after = existing[end:].lstrip("\r\n")
        out = existing[:start] + block.rstrip() + "\n"
        if after:
            out += "\n" + after
            if not after.endswith("\n"):
                out += "\n"
        return out
    out = existing.rstrip()
    if out:
        out += "\n\n"
    out += block.rstrip() + "\n"
    return out


def _sync_gtk_user_css(profile: Profile) -> None:
    # GTK3 gets full dialog chrome (zenity). GTK4 only defines colors + dialog
    # selectors — generic headerbar/window rules at USER priority override suite
    # chrome and stick until process restart (purple leftovers on light themes).
    gtk3 = _system_dialog_css(profile)
    gtk4 = _system_dialog_css_gtk4(profile)
    prefer = "1" if profile.is_dark() else "0"
    xdg = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    for ver, block in (("gtk-3.0", gtk3), ("gtk-4.0", gtk4)):
        d = xdg / ver
        try:
            d.mkdir(parents=True, exist_ok=True)
        except OSError:
            continue
        path = d / "gtk.css"
        try:
            existing = path.read_text(encoding="utf-8") if path.is_file() else ""
        except OSError:
            existing = ""
        try:
            path.write_text(_upsert_managed_css_block(existing, block), encoding="utf-8")
        except OSError:
            pass
        settings = d / "settings.ini"
        try:
            if settings.is_file():
                prev = settings.read_text(encoding="utf-8")
                lines = []
                found = False
                for line in prev.splitlines():
                    if line.lstrip().startswith("gtk-application-prefer-dark-theme"):
                        lines.append(f"gtk-application-prefer-dark-theme={prefer}")
                        found = True
                    else:
                        lines.append(line)
                if not found:
                    if "[Settings]" not in prev:
                        lines.insert(0, "[Settings]")
                    lines.append(f"gtk-application-prefer-dark-theme={prefer}")
                settings.write_text("\n".join(lines) + "\n", encoding="utf-8")
            else:
                settings.write_text(
                    f"[Settings]\ngtk-application-prefer-dark-theme={prefer}\n",
                    encoding="utf-8",
                )
        except OSError:
            pass
    _sync_gsettings_color_scheme(profile.is_dark())


def _system_dialog_css_gtk4(profile: Profile) -> str:
    bg = profile.background
    fg = profile.foreground
    surface = profile.surface_hex()
    border = profile.surface_alt_hex()
    accent = profile.accent()
    on_accent = "#fbf1c7" if _relative_luminance(accent) < 0.55 else "#1d2021"
    # Libadwaita (zenity 4+) ignores most widget selectors and reads these
    # named colors / CSS variables instead. Keep selectors narrow so suite apps
    # are not overridden at USER priority.
    return f"""{_GTK_USER_CSS_BEGIN}
/* Suite profile → libadwaita / zenity-4 dialogs (named colors + dialog only). */
@define-color theme_bg_color {bg};
@define-color theme_fg_color {fg};
@define-color theme_base_color {bg};
@define-color theme_text_color {fg};
@define-color theme_selected_bg_color {accent};
@define-color theme_selected_fg_color {on_accent};
@define-color theme_unfocused_selected_bg_color {accent};
@define-color theme_unfocused_selected_fg_color {on_accent};
@define-color accent_bg_color {accent};
@define-color accent_fg_color {on_accent};
@define-color accent_color {accent};
@define-color blue_3 {accent};
@define-color window_bg_color {bg};
@define-color window_fg_color {fg};
@define-color view_bg_color {bg};
@define-color view_fg_color {fg};
@define-color headerbar_bg_color {surface};
@define-color headerbar_fg_color {fg};
@define-color headerbar_backdrop_color {surface};
@define-color headerbar_border_color {border};
@define-color sidebar_bg_color {surface};
@define-color sidebar_fg_color {fg};
@define-color sidebar_backdrop_color {surface};
@define-color sidebar_border_color {border};
@define-color secondary_sidebar_bg_color {surface};
@define-color secondary_sidebar_fg_color {fg};
@define-color secondary_sidebar_backdrop_color {surface};
@define-color card_bg_color {surface};
@define-color card_fg_color {fg};
@define-color dialog_bg_color {bg};
@define-color dialog_fg_color {fg};

:root {{
  --window-bg-color: {bg};
  --window-fg-color: {fg};
  --view-bg-color: {bg};
  --view-fg-color: {fg};
  --headerbar-bg-color: {surface};
  --headerbar-fg-color: {fg};
  --headerbar-backdrop-color: {surface};
  --headerbar-border-color: {border};
  --sidebar-bg-color: {surface};
  --sidebar-fg-color: {fg};
  --sidebar-backdrop-color: {surface};
  --sidebar-border-color: {border};
  --secondary-sidebar-bg-color: {surface};
  --secondary-sidebar-fg-color: {fg};
  --secondary-sidebar-backdrop-color: {surface};
  --card-bg-color: {surface};
  --card-fg-color: {fg};
  --dialog-bg-color: {bg};
  --dialog-fg-color: {fg};
  --accent-bg-color: {accent};
  --accent-fg-color: {on_accent};
  --accent-color: {accent};
}}

window.messagedialog, window.dialog, window.filechooser,
.messagedialog, .dialog, .filechooser {{
  background-color: {bg};
  color: {fg};
}}
window.messagedialog headerbar, window.dialog headerbar, window.filechooser headerbar,
window.messagedialog .titlebar, window.dialog .titlebar, window.filechooser .titlebar {{
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-bottom: 1px solid {border};
}}
window.messagedialog button.suggested-action,
window.dialog button.suggested-action,
window.filechooser button.suggested-action {{
  background-color: {accent};
  color: {on_accent};
}}
{_GTK_USER_CSS_END}
"""


def _sync_gsettings_color_scheme(is_dark: bool) -> None:
    scheme = "prefer-dark" if is_dark else "prefer-light"
    theme = "Adwaita-dark" if is_dark else "Adwaita"
    if shutil.which("gsettings"):
        for args in (
            ["set", "org.gnome.desktop.interface", "color-scheme", scheme],
            ["set", "org.gnome.desktop.interface", "gtk-theme", theme],
        ):
            try:
                subprocess.run(
                    ["gsettings", *args],
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    check=False,
                    timeout=5,
                )
            except Exception:
                pass
    # XFCE Power Manager / xfsettingsd read this channel (may be empty on Hyprland).
    if shutil.which("xfconf-query"):
        try:
            r = subprocess.run(
                ["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
                timeout=5,
            )
            cmd = ["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName", "-s", theme]
            if r.returncode != 0:
                cmd = [
                    "xfconf-query",
                    "-c",
                    "xsettings",
                    "-n",
                    "-t",
                    "string",
                    "-p",
                    "/Net/ThemeName",
                    "-s",
                    theme,
                ]
            subprocess.run(
                cmd,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
                timeout=5,
            )
        except Exception:
            pass


def _config_candidates(rel: str) -> list[Path]:
    xdg = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    return [xdg / rel, Path.home() / "configs" / rel]


def _first_existing_config(rel: str) -> Optional[Path]:
    for p in _config_candidates(rel):
        if p.is_file():
            return p
    return None


def _rewrite_css_block(css: str, selector: str, body: str) -> str:
    start = css.find(selector)
    if start < 0:
        return css
    after = css[start:]
    brace = after.find("{")
    if brace < 0:
        return css
    end_rel = after[brace:].find("}")
    if end_rel < 0:
        return css
    end = start + brace + end_rel + 1
    return css[:start] + f"{selector} {{\n{body}\n}}" + css[end:]


def _all_existing_configs(rel: str) -> list[Path]:
    seen: set[Path] = set()
    out: list[Path] = []
    for p in _config_candidates(rel):
        if not p.is_file():
            continue
        try:
            key = p.resolve()
        except OSError:
            key = p
        if key in seen:
            continue
        seen.add(key)
        out.append(p)
    return out


def _rewrite_waybar_global_colors(css: str, fg: str, surface: str) -> str:
    lines = css.splitlines(keepends=True)
    out: list[str] = []
    for line in lines:
        bare = line.rstrip("\r\n")
        nl = line[len(bare) :]
        trimmed = bare.strip()
        indent = bare[: len(bare) - len(bare.lstrip())]
        if trimmed in ("color: #f5f5f5;", "color: #ffffff;", "color: #cccccc;", "color: #888888;"):
            if "#888888" in trimmed:
                dim = _mix_hex(fg, surface, 0.35)
            elif "#cccccc" in trimmed:
                dim = _mix_hex(fg, surface, 0.15)
            else:
                dim = fg
            out.append(f"{indent}color: {dim};{nl or chr(10)}")
        elif (
            trimmed.startswith("background: #2e2e2e")
            or trimmed.startswith("background-color: #2e2e2e")
            or trimmed.startswith("background: #0a0a0a")
        ):
            prop = "background-color" if trimmed.startswith("background-color") else "background"
            out.append(f"{indent}{prop}: {surface};{nl or chr(10)}")
        else:
            out.append(line)
    return "".join(out)


def _restart_waybar() -> None:
    # Session start already launched waybar; killall+respawn there freezes Hyprland.
    if _theme_session_safe():
        if shutil.which("killall"):
            try:
                subprocess.run(
                    ["killall", "-SIGUSR2", "waybar"],
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    check=False,
                    timeout=3,
                )
            except Exception:
                pass
        return
    try:
        subprocess.run(
            ["killall", "waybar"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
            timeout=5,
        )
    except Exception:
        pass
    time.sleep(0.15)
    try:
        subprocess.Popen(
            ["waybar"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except Exception:
        pass


def _sync_waybar_style(bg: str, fg: str, border: str, surface: str) -> None:
    paths = _all_existing_configs("waybar/style.css")
    if not paths:
        return
    for path in paths:
        try:
            original = path.read_text(encoding="utf-8")
        except OSError:
            continue
        out = original
        out = _rewrite_css_block(
            out,
            "window#waybar",
            f"  background-color: {bg};\n  color: {fg};\n  border-bottom: 2px solid {border};",
        )
        out = _rewrite_css_block(
            out,
            "#workspaces button.active",
            f"  color: {fg};\n  background: {surface};",
        )
        out = _rewrite_waybar_global_colors(out, fg, surface)
        if out != original:
            try:
                path.write_text(out, encoding="utf-8")
            except OSError:
                pass
    _restart_waybar()


def _replace_ini_assign(text: str, key: str, value: str) -> str:
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    prefix = f"{key}="
    for line in lines:
        bare = line.rstrip("\r\n")
        nl = line[len(bare) :]
        trimmed = bare.lstrip()
        indent = bare[: len(bare) - len(trimmed)]
        if trimmed.startswith(prefix):
            out.append(f"{indent}{key}={value}{nl or chr(10)}")
        else:
            out.append(line)
    return "".join(out)


def _sync_mako_colors(bg: str, fg: str, border: str) -> None:
    path = _first_existing_config("mako/config")
    if path is None:
        return
    try:
        original = path.read_text(encoding="utf-8")
    except OSError:
        return
    out = original
    out = _replace_ini_assign(out, "background-color", bg)
    out = _replace_ini_assign(out, "text-color", fg)
    out = _replace_ini_assign(out, "border-color", border)
    if out != original:
        try:
            path.write_text(out, encoding="utf-8")
        except OSError:
            return
    if shutil.which("makoctl"):
        try:
            subprocess.run(
                ["makoctl", "reload"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
                timeout=5,
            )
        except Exception:
            pass


def _bare_rgba(hex_color: str, alpha: str = "ff") -> str:
    h = hex_color.lstrip("#")
    if len(h) == 6:
        return f"{h}{alpha}"
    return h


def _sync_fuzzel_colors(bg: str, fg: str, border: str, surface: str, accent: str) -> None:
    path = _first_existing_config("fuzzel/fuzzel.ini")
    if path is None:
        return
    try:
        original = path.read_text(encoding="utf-8")
    except OSError:
        return
    out = original
    out = _replace_ini_assign(out, "background", _bare_rgba(bg, "f2"))
    out = _replace_ini_assign(out, "text", _bare_rgba(fg))
    out = _replace_ini_assign(out, "border", _bare_rgba(border))
    out = _replace_ini_assign(out, "selection", _bare_rgba(surface))
    out = _replace_ini_assign(out, "selection-text", _bare_rgba(fg))
    out = _replace_ini_assign(out, "match", _bare_rgba(accent))
    out = _replace_ini_assign(out, "selection-match", _bare_rgba(accent))
    if out != original:
        try:
            path.write_text(out, encoding="utf-8")
        except OSError:
            pass


def _hex_to_hypr_rgba(hex_color: str, alpha: str) -> Optional[str]:
    h = hex_color.lstrip("#")
    if len(h) != 6:
        return None
    return f"rgba({h.lower()}{alpha})"


def _sync_hypr_window_borders(border: str, surface: str) -> None:
    active = _hex_to_hypr_rgba(border, "aa")
    inactive = _hex_to_hypr_rgba(surface, "88")
    if not active or not inactive:
        return
    path = _hyprland_conf_path()
    if path is not None:
        try:
            original = path.read_text(encoding="utf-8")
        except OSError:
            original = None
        if original is not None:
            out = original
            out = _replace_hypr_assign(out, "col.active_border", active)
            out = _replace_hypr_assign(out, "col.inactive_border", inactive)
            out = _replace_hypr_assign(out, "panelBorderColor", active)
            inactive_ws = _hex_to_hypr_rgba(border, "ff")
            if inactive_ws:
                out = _replace_hypr_assign(out, "workspaceInactiveBorder", inactive_ws)
            if out != original:
                try:
                    path.write_text(out, encoding="utf-8")
                except OSError:
                    pass
    if not _theme_session_safe():
        _hyprctl_keyword("general:col.active_border", active)
        _hyprctl_keyword("general:col.inactive_border", inactive)


# ---------------------------------------------------------------------------
# GTK4
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# gtk-neurond autostart (suite-wide)
# ---------------------------------------------------------------------------


def _neuron_socket_path() -> Path:
    runtime = os.environ.get("XDG_RUNTIME_DIR", "").strip()
    if runtime:
        return Path(runtime) / "gtk-neuron.sock"
    cache = Path.home() / ".cache" / "gtk-apps"
    return cache / "gtk-neuron.sock"


def _neuron_socket_connectable() -> bool:
    sock = _neuron_socket_path()
    if not sock.exists():
        return False
    try:
        import socket

        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            s.settimeout(0.2)
            s.connect(str(sock))
            return True
        finally:
            s.close()
    except OSError:
        return False


def _find_neurond_binary() -> Optional[Path]:
    env = os.environ.get("GTK_NEUROND", "").strip()
    if env and Path(env).is_file():
        return Path(env)

    candidates = [
        _SUITE_ROOT / "gtk-neuron" / "target" / "release" / "gtk-neurond",
        _SUITE_ROOT / "gtk-neuron" / "target" / "debug" / "gtk-neurond",
        Path("/usr/local/lib/neuronix/gtk-apps/gtk-neuron/gtk-neurond"),
        Path("/usr/local/bin/gtk-neurond"),
    ]
    which = shutil.which("gtk-neurond")
    if which:
        candidates.append(Path(which))

    for c in candidates:
        if c.is_file() and os.access(c, os.X_OK):
            return c
    return None


def ensure_neuron_daemon() -> None:
    """Start gtk-neurond if it is not already listening (best-effort)."""
    if _neuron_socket_connectable():
        return
    sock = _neuron_socket_path()
    try:
        if sock.exists():
            sock.unlink()
    except OSError:
        pass
    exe = _find_neurond_binary()
    if exe is None:
        return
    try:
        sock.parent.mkdir(parents=True, exist_ok=True)
        subprocess.Popen(
            [str(exe), "--daemon"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError:
        return
    for _ in range(40):
        time.sleep(0.05)
        if _neuron_socket_connectable():
            return


# ---------------------------------------------------------------------------


def apply_chrome(profile: Optional[Profile] = None) -> None:
    import gi

    gi.require_version("Gtk", "4.0")
    gi.require_version("Gdk", "4.0")
    from gi.repository import Gdk, Gtk

    ensure_neuron_daemon()

    profile = profile or load_profile()
    hide_csd = hyprbars_active()
    settings = Gtk.Settings.get_default()
    if settings is not None:
        settings.set_property("gtk-application-prefer-dark-theme", profile.is_dark())
        if hide_csd:
            # Hyprbars draws − □ ×; hide GTK CSD window controls to avoid doubles.
            settings.set_property("gtk-decoration-layout", ":")
    try:
        apply_adw_color_scheme(profile)
    except Exception:
        pass

    display = Gdk.Display.get_default()
    if display is None:
        return

    global _chrome_provider
    priority = Gtk.STYLE_PROVIDER_PRIORITY_USER - 10
    if _chrome_provider is None:
        _chrome_provider = Gtk.CssProvider()
        Gtk.StyleContext.add_provider_for_display(
            display, _chrome_provider, priority
        )
    css = chrome_css(profile)
    if hide_csd:
        css = css + "\n" + _HIDE_CSD_WINDOWCONTROLS_CSS
    _chrome_provider.load_from_data(css.encode("utf-8"))


_HIDE_CSD_WINDOWCONTROLS_CSS = """
headerbar windowcontrols,
.titlebar windowcontrols,
windowcontrols {
  opacity: 0;
  min-width: 0;
  padding: 0;
  margin: 0;
}
headerbar windowcontrols button,
.titlebar windowcontrols button,
windowcontrols button {
  min-width: 0;
  min-height: 0;
  padding: 0;
  margin: 0;
  opacity: 0;
  border: none;
  background: none;
  background-image: none;
  box-shadow: none;
}
"""


def hyprbars_active() -> bool:
    """True when the Hyprland hyprbars plugin is loaded."""
    if not shutil.which("hyprctl"):
        return False
    try:
        out = subprocess.run(
            ["hyprctl", "plugin", "list"],
            env=_hypr_env(),
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
            timeout=5,
        )
    except Exception:
        return False
    return "plugin hyprbars" in (out.stdout or "").lower()


def headerbar_show_title_buttons() -> bool:
    """Prefer hiding GTK headerbar close/min/max when hyprbars owns them."""
    return not hyprbars_active()


def prepare_headerbar(headerbar) -> None:
    """Apply suite defaults (hide CSD buttons under hyprbars)."""
    try:
        headerbar.set_show_title_buttons(headerbar_show_title_buttons())
    except Exception:
        pass


def append_profile_menu(parent, action_name: str = "win.theme") -> None:
    """Append a Profile submenu of radio items (same shape as Rust gtk-theme).

    Ends with a **Custom…** item that activates ``win.open-theme-editor``
    (install via :func:`install_open_theme_editor_action`).
    """
    import gi

    gi.require_version("Gtk", "4.0")
    from gi.repository import Gio, GLib

    profiles_menu = Gio.Menu()

    builtin = Gio.Menu()
    for profile in builtin_profiles():
        item = Gio.MenuItem.new(profile.name, None)
        item.set_action_and_target_value(
            action_name, GLib.Variant.new_string(profile.id)
        )
        builtin.append_item(item)
    profiles_menu.append_section(None, builtin)

    builtins = {p.id for p in builtin_profiles()}
    customs = [p for p in all_profiles() if p.id not in builtins]
    if customs:
        custom_section = Gio.Menu()
        for profile in customs:
            item = Gio.MenuItem.new(profile.name, None)
            item.set_action_and_target_value(
                action_name, GLib.Variant.new_string(profile.id)
            )
            custom_section.append_item(item)
        profiles_menu.append_section("Saved", custom_section)

    editor = Gio.Menu()
    editor.append("Custom…", OPEN_THEME_EDITOR_MENU_ACTION)
    profiles_menu.append_section(None, editor)

    item = Gio.MenuItem.new("Profile", None)
    item.set_submenu(profiles_menu)
    parent.append_item(item)


def _menu_containing_section(menu, section_id: str):
    """Return the Gio.Menu that directly owns a section with the given id."""
    from gi.repository import Gio

    for idx in range(menu.get_n_items()):
        attr = menu.get_item_attribute_value(idx, "id")
        if attr is not None and attr.get_string() == section_id:
            return menu
        for link in (Gio.MENU_LINK_SECTION, Gio.MENU_LINK_SUBMENU):
            linked = menu.get_item_link(idx, link)
            if linked is not None:
                found = _menu_containing_section(linked, section_id)
                if found is not None:
                    return found
    return None


def integrate_profile_menu(
    window,
    menu_model,
    *,
    action_name: str = "theme",
    menu_action: str = "win.theme",
    section_id: str | None = "profile-section",
    before_section_id: str | None = None,
):
    """Apply suite chrome and fold Profile into an existing app menu.

    Prefer this over :func:`attach_profile_menu` when the window already has a
    hamburger / gear menu — that helper would pack a second MenuButton.

    If ``section_id`` is set and found (possibly nested), that section is
    replaced with a Profile submenu. Otherwise a Profile section is inserted
    before ``before_section_id`` (or appended).
    """
    import gi

    gi.require_version("Gtk", "4.0")
    from gi.repository import Gio, GLib

    apply_chrome(load_profile())
    action = install_profile_action(window, action_name)
    install_open_theme_editor_action(window)

    profile_items = Gio.Menu()
    append_profile_menu(profile_items, menu_action)

    parent = None
    if section_id:
        parent = _menu_containing_section(menu_model, section_id)
    if parent is not None and section_id:
        section = Gio.MenuItem.new_section(None, profile_items)
        section.set_attribute([("id", "s", section_id)])
        for idx in range(parent.get_n_items()):
            attr = parent.get_item_attribute_value(idx, "id")
            if attr is not None and attr.get_string() == section_id:
                parent.remove(idx)
                parent.insert_item(idx, section)
                break
    else:
        insert_at = menu_model.get_n_items()
        if before_section_id:
            for idx in range(menu_model.get_n_items()):
                attr = menu_model.get_item_attribute_value(idx, "id")
                if attr is not None and attr.get_string() == before_section_id:
                    insert_at = idx
                    break
        menu_model.insert_section(insert_at, None, profile_items)

    def _sync(profile: Profile) -> None:
        action.set_state(GLib.Variant.new_string(profile.id))

    watch_theme(_sync, gtk_version=4)
    return action


def show_about_dialog(
    parent=None,
    *,
    program_name: str,
    comments: str = "",
    version: str = "0.1.0",
) -> None:
    """Present a suite-standard About dialog (author + GTK-Apps repo link)."""
    import gi

    gi.require_version("Gtk", "4.0")
    from gi.repository import Gtk

    dialog = Gtk.AboutDialog()
    dialog.set_program_name(program_name)
    dialog.set_version(version)
    if comments:
        dialog.set_comments(comments)
    dialog.set_authors([SUITE_AUTHOR])
    dialog.set_website(SUITE_WEBSITE)
    dialog.set_website_label(SUITE_WEBSITE_LABEL)
    dialog.set_license_type(Gtk.License.GPL_3_0)
    dialog.set_modal(True)
    if parent is not None:
        dialog.set_transient_for(parent)
    dialog.present()


def install_about_action(
    window,
    *,
    program_name: str,
    comments: str = "",
    version: str = "0.1.0",
) -> None:
    """Install ``win.about`` (idempotent) that opens :func:`show_about_dialog`."""
    import gi

    gi.require_version("Gtk", "4.0")
    from gi.repository import Gio

    if window.lookup_action(ABOUT_ACTION) is not None:
        window.remove_action(ABOUT_ACTION)

    action = Gio.SimpleAction.new(ABOUT_ACTION, None)

    def _on_activate(_action, _param) -> None:
        show_about_dialog(
            window,
            program_name=program_name,
            comments=comments,
            version=version,
        )

    action.connect("activate", _on_activate)
    window.add_action(action)


def build_profile_only_menu(action_name: str = "win.theme"):
    """Flat Profile section for a hamburger MenuButton that only offers themes."""
    import gi

    gi.require_version("Gtk", "4.0")
    from gi.repository import Gio, GLib

    profiles = Gio.Menu()
    builtins = {p.id for p in builtin_profiles()}
    for profile in builtin_profiles():
        item = Gio.MenuItem.new(profile.name, None)
        item.set_action_and_target_value(
            action_name, GLib.Variant.new_string(profile.id)
        )
        profiles.append_item(item)
    for profile in all_profiles():
        if profile.id in builtins:
            continue
        item = Gio.MenuItem.new(profile.name, None)
        item.set_action_and_target_value(
            action_name, GLib.Variant.new_string(profile.id)
        )
        profiles.append_item(item)
    profiles.append("Custom…", OPEN_THEME_EDITOR_MENU_ACTION)
    menu = Gio.Menu()
    menu.append_section("Profile", profiles)
    about = Gio.Menu()
    about.append("About", ABOUT_MENU_ACTION)
    menu.append_section(None, about)
    return menu

def install_profile_action(window, action_name: str = "theme"):
    """Stateful string action that selects a suite profile."""
    import gi

    gi.require_version("Gtk", "4.0")
    from gi.repository import Gio, GLib

    # Replace any prior theme action so re-attach is safe.
    if window.lookup_action(action_name) is not None:
        window.remove_action(action_name)

    current = GLib.Variant.new_string(load_theme_id())
    action = Gio.SimpleAction.new_stateful(
        action_name, GLib.VariantType.new("s"), current
    )

    def _on_activate(_action, param) -> None:
        if param is None:
            return
        select_theme(param.get_string(), gtk_version=4)
        _action.set_state(param)

    action.connect("activate", _on_activate)
    window.add_action(action)
    return action


def install_open_theme_editor_action(window) -> None:
    """Install ``win.open-theme-editor`` (idempotent)."""
    import gi

    gi.require_version("Gtk", "4.0")
    from gi.repository import Gio

    if window.lookup_action(OPEN_THEME_EDITOR_ACTION) is not None:
        return
    action = Gio.SimpleAction.new(OPEN_THEME_EDITOR_ACTION, None)
    action.connect("activate", lambda *_: launch_theme_editor())
    window.add_action(action)


def _as_headerbar(widget):
    """Return widget if it is a Gtk or Adw HeaderBar."""
    if widget is None:
        return None
    import gi

    gi.require_version("Gtk", "4.0")
    from gi.repository import Gtk

    if isinstance(widget, Gtk.HeaderBar):
        return widget
    try:
        gi.require_version("Adw", "1")
        from gi.repository import Adw

        if isinstance(widget, Adw.HeaderBar):
            return widget
    except (ValueError, ImportError):
        pass
    return None


def attach_profile_menu(
    window,
    headerbar=None,
    *,
    create_headerbar: bool = True,
    about_name: str | None = None,
    about_comments: str = "",
    about_version: str = "0.1.0",
):
    """Apply chrome and add a hamburger Profile (+ About) menu to the header bar."""
    import gi

    gi.require_version("Gtk", "4.0")
    from gi.repository import GLib, Gtk

    apply_chrome(load_profile())

    headerbar = _as_headerbar(headerbar)
    if headerbar is None:
        headerbar = _as_headerbar(window.get_titlebar())
    if headerbar is None and create_headerbar:
        headerbar = Gtk.HeaderBar()
        # Prefer set_titlebar when available; Adw.ApplicationWindow uses content.
        if hasattr(window, "set_titlebar"):
            try:
                window.set_titlebar(headerbar)
            except Exception:
                headerbar = None
    if headerbar is None:
        raise ValueError("No HeaderBar available; pass headerbar=...")
    prepare_headerbar(headerbar)

    action = install_profile_action(window, "theme")
    install_open_theme_editor_action(window)
    program_name = about_name or window.get_title() or "GTK App"
    install_about_action(
        window,
        program_name=program_name,
        comments=about_comments,
        version=about_version,
    )
    menu = build_profile_only_menu("win.theme")

    button = Gtk.MenuButton()
    button.set_icon_name("open-menu-symbolic")
    button.set_tooltip_text("Menu")
    button.set_menu_model(menu)
    headerbar.pack_end(button)

    def _sync(profile: Profile) -> None:
        action.set_state(GLib.Variant.new_string(profile.id))

    watch_theme(_sync, gtk_version=4)
    return headerbar

# ---------------------------------------------------------------------------
# GTK3 (Services apps: shortcuts_viewer / shortcuts_list)
# ---------------------------------------------------------------------------


def apply_chrome_gtk3(profile: Optional[Profile] = None) -> None:
    import gi

    gi.require_version("Gtk", "3.0")
    gi.require_version("Gdk", "3.0")
    from gi.repository import Gdk, Gtk

    ensure_neuron_daemon()

    profile = profile or load_profile()
    settings = Gtk.Settings.get_default()
    if settings is not None:
        settings.set_property("gtk-application-prefer-dark-theme", profile.is_dark())

    screen = Gdk.Screen.get_default()
    if screen is None:
        return

    global _chrome_provider
    priority = Gtk.STYLE_PROVIDER_PRIORITY_USER - 10
    if _chrome_provider is None:
        _chrome_provider = Gtk.CssProvider()
        Gtk.StyleContext.add_provider_for_screen(
            screen, _chrome_provider, priority
        )
    # GTK3 CssProvider rejects GTK4-only properties and fails the whole sheet.
    _chrome_provider.load_from_data(_css_for_gtk3(chrome_css(profile)).encode("utf-8"))


def _css_for_gtk3(css: str) -> str:
    """Drop GTK4-only declarations so GTK3 can load the shared chrome CSS."""
    # `:root` is GTK4; GTK3 rejects it as an invalid pseudo-class and fails the sheet.
    css = re.sub(r"(?ms)^\s*:root\s*\{.*?^\}\s*", "", css)
    # `-gtk-icon-filter` is GTK4; GTK3 treats unknown properties as hard errors.
    css = re.sub(r"(?m)^\s*-gtk-icon-filter\s*:[^;]+;\s*$", "", css)
    return css


def attach_profile_menu_gtk3(window) -> None:
    """GTK3: apply chrome, hamburger Profile menu on a HeaderBar, watch theme.toml."""
    import gi

    gi.require_version("Gtk", "3.0")
    from gi.repository import Gtk

    apply_chrome_gtk3(load_profile())

    header = window.get_titlebar()
    if not isinstance(header, Gtk.HeaderBar):
        header = Gtk.HeaderBar()
        header.set_show_close_button(True)
        title = window.get_title() or "App"
        header.set_title(title)
        window.set_titlebar(header)

    menu = Gtk.Menu()
    current_id = load_theme_id()
    group = None
    for profile in all_profiles():
        item = Gtk.RadioMenuItem.new_with_label(group, profile.name)
        group = item.get_group()
        item.set_active(profile.id == current_id)

        def _activate(widget, pid=profile.id):
            if widget.get_active():
                select_theme(pid, gtk_version=3)

        item.connect("activate", _activate)
        menu.append(item)

    menu.append(Gtk.SeparatorMenuItem())
    custom_item = Gtk.MenuItem.new_with_label("Custom…")
    custom_item.connect("activate", lambda *_: launch_theme_editor())
    menu.append(custom_item)
    menu.show_all()

    button = Gtk.MenuButton()
    button.set_popup(menu)
    button.set_tooltip_text("Menu")
    image = Gtk.Image.new_from_icon_name("open-menu-symbolic", Gtk.IconSize.BUTTON)
    button.set_image(image)
    button.set_always_show_image(True)
    header.pack_end(button)
    button.show_all()

    watch_theme(lambda p: apply_chrome_gtk3(p), gtk_version=3)


def watch_theme(
    on_change: Callable[[Profile], None],
    *,
    gtk_version: int = 4,
) -> None:
    """Re-apply chrome and notify when ``theme.toml`` changes (or local select)."""
    global _monitor, _last_id
    _watchers.append(on_change)
    if _last_id == "":
        _last_id = load_theme_id()
    if _monitor is not None:
        return

    import gi

    gi.require_version("Gio", "2.0")
    from gi.repository import Gio

    if not _THEME_PATH.exists():
        save_theme_id(load_theme_id())
    file = Gio.File.new_for_path(str(_THEME_PATH))
    _monitor = file.monitor_file(Gio.FileMonitorFlags.NONE, None)

    def _changed(_mon, _file, _other, event):
        if event not in (
            Gio.FileMonitorEvent.CHANGES_DONE_HINT,
            Gio.FileMonitorEvent.CHANGED,
            Gio.FileMonitorEvent.CREATED,
        ):
            return
        profile_id = load_theme_id()
        if profile_id == _last_id:
            return
        profile = profile_by_id(profile_id)
        if profile is None:
            return
        if gtk_version >= 4:
            apply_chrome(profile)
        else:
            apply_chrome_gtk3(profile)
        _broadcast(profile, from_file=True)

    _monitor.connect("changed", _changed)


def _broadcast(profile: Profile, from_file: bool) -> None:
    global _last_id
    if from_file and _last_id == profile.id:
        return
    _last_id = profile.id
    for cb in list(_watchers):
        try:
            cb(profile)
        except Exception:
            pass


def _parse_hex(color: str) -> tuple[int, int, int]:
    color = color.lstrip("#")
    if len(color) == 3:
        color = "".join(c * 2 for c in color)
    return int(color[0:2], 16), int(color[2:4], 16), int(color[4:6], 16)


def _relative_luminance(color: str) -> float:
    r, g, b = (_c / 255.0 for _c in _parse_hex(color))

    def lin(c: float) -> float:
        return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4

    return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)


def _mix_hex(a: str, b: str, t: float) -> str:
    ar, ag, ab = _parse_hex(a)
    br, bg, bb = _parse_hex(b)
    r = round(ar + (br - ar) * t)
    g = round(ag + (bg - ag) * t)
    b_ = round(ab + (bb - ab) * t)
    return f"#{r:02x}{g:02x}{b_:02x}"
