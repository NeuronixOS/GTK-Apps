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
import sys
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


# ---------------------------------------------------------------------------
# GTK4
# ---------------------------------------------------------------------------


def apply_chrome(profile: Optional[Profile] = None) -> None:
    import gi

    gi.require_version("Gtk", "4.0")
    gi.require_version("Gdk", "4.0")
    from gi.repository import Gdk, Gtk

    profile = profile or load_profile()
    settings = Gtk.Settings.get_default()
    if settings is not None:
        settings.set_property("gtk-application-prefer-dark-theme", profile.is_dark())
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
    _chrome_provider.load_from_data(chrome_css(profile).encode("utf-8"))


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
        headerbar.set_show_title_buttons(True)
        # Prefer set_titlebar when available; Adw.ApplicationWindow uses content.
        if hasattr(window, "set_titlebar"):
            try:
                window.set_titlebar(headerbar)
            except Exception:
                headerbar = None
    if headerbar is None:
        raise ValueError("No HeaderBar available; pass headerbar=...")

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
