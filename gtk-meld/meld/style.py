# Copyright (C) 2002-2006 Stephen Kennedy <stevek@gnome.org>
# Copyright (C) 2009 Vincent Legoll <vincent.legoll@gmail.com>
# Copyright (C) 2012-2026 Kai Willadsen <kai.willadsen@gmail.com>
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 2 of the License, or (at
# your option) any later version.
#
# This program is distributed in the hope that it will be useful, but
# WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
# General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <http://www.gnu.org/licenses/>.

import enum
from typing import Mapping, Tuple

from gi.repository import Adw, Gdk, Gtk, GtkSource, Pango

from meld.conf import _
from meld.settings import get_meld_settings

style_scheme: GtkSource.StyleScheme | None = None
base_style_scheme: GtkSource.StyleScheme | None = None

# When set by gtk-theme suite Profile sync, overrides meld-base/meld-dark
# chunk colours so diffs match the selected GTK-Apps profile.
_suite_fill_colours: dict[str, Gdk.RGBA] | None = None
_suite_line_colours: dict[str, Gdk.RGBA] | None = None


class MeldStyleScheme(enum.Enum):
    base = "meld-base"
    dark = "meld-dark"


def set_suite_chunk_colours(
    fill_hex: Mapping[str, str] | None,
    line_hex: Mapping[str, str] | None,
) -> None:
    """Install (or clear) suite Profile–derived diff chunk colours."""
    global _suite_fill_colours, _suite_line_colours

    if fill_hex is None or line_hex is None:
        _suite_fill_colours = None
        _suite_line_colours = None
        return

    def _rgba_map(src: Mapping[str, str]) -> dict[str, Gdk.RGBA]:
        out: dict[str, Gdk.RGBA] = {}
        for key, value in src.items():
            colour = Gdk.RGBA()
            if not colour.parse(value):
                colour.parse("#888888")
            out[key] = colour
        return out

    _suite_fill_colours = _rgba_map(fill_hex)
    _suite_line_colours = _rgba_map(line_hex)


def adapt_style_scheme(style_scheme: GtkSource.StyleScheme) -> GtkSource.StyleScheme:
    adw_manager = Adw.StyleManager.get_default()

    desired_variant = "dark" if adw_manager.get_dark() else "light"
    variant = style_scheme.get_metadata("variant")
    other_variant = style_scheme.get_metadata(f"{desired_variant}-variant")
    # If we have no variant data, or the variant matches, use the scheme
    if not variant or variant == desired_variant or not other_variant:
        return style_scheme

    source_manager = GtkSource.StyleSchemeManager.get_default()
    variant_scheme = source_manager.get_scheme(other_variant)
    return variant_scheme or style_scheme


def should_use_dark(style_scheme: GtkSource.StyleScheme) -> bool:
    # If the style scheme has variant metadata, we will trust that
    variant = style_scheme.get_metadata("variant")
    if variant:
        return variant == "dark"

    # Get our text background colour by checking the 'text' style of
    # the user's selected style scheme, falling back to the GTK+ theme
    # background if there is no style scheme background set.
    style = style_scheme.get_style("text") if style_scheme else None
    if style:
        background = style.props.background
        rgba = Gdk.RGBA()
        rgba.parse(background)
    else:
        # This case will only be hit for GtkSourceView style schemes
        # that don't set a text background, like the "Classic" scheme.
        from meld.sourceview import MeldSourceView

        stylecontext = MeldSourceView().get_style_context()
        background_set, rgba = stylecontext.lookup_color("theme_bg_color")
        if not background_set:
            rgba = Gdk.RGBA(1, 1, 1, 1)

    # This heuristic is absolutely dire. I made it up. There's
    # literally no basis to this.
    return (rgba.red + rgba.green + rgba.blue) < 1.0


def set_base_style_scheme(
    new_style_scheme: GtkSource.StyleScheme,
) -> GtkSource.StyleScheme:
    global base_style_scheme
    global style_scheme

    style_scheme = new_style_scheme
    use_dark = should_use_dark(style_scheme)

    manager = GtkSource.StyleSchemeManager.get_default()
    base_scheme_name = MeldStyleScheme.dark if use_dark else MeldStyleScheme.base
    base_style_scheme = manager.get_scheme(base_scheme_name.value)
    base_schemes = (MeldStyleScheme.dark.value, MeldStyleScheme.base.value)
    if style_scheme and style_scheme.props.id in base_schemes:
        style_scheme = base_style_scheme

    return base_style_scheme


def init_sourceview_style_context():
    def on_setting_changed(meld_settings, key):
        if key != "font":
            return

        css_provider.load_from_string(
            f".meld-monospace-font {{"
            f"  font-family: {meld_settings.font.get_family()};"
            f"  font-size: {max(1, meld_settings.font.get_size() / Pango.SCALE)}pt;"
            f"}}"
        )

    css_provider = Gtk.CssProvider()
    Gtk.StyleContext.add_provider_for_display(
        Gdk.Display.get_default(), css_provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
    )

    meld_settings = get_meld_settings()
    meld_settings.connect("changed", on_setting_changed)
    on_setting_changed(meld_settings, "font")


def colour_lookup_with_fallback(name: str, attribute: str) -> Gdk.RGBA:
    # Suite Profile overrides (gtk-theme) win over meld-base / meld-dark purple.
    _ensure_suite_chunk_colours()
    suite = _suite_colour_for(name, attribute)
    if suite is not None:
        return suite

    style = style_scheme.get_style(name) if style_scheme else None
    style_attr = getattr(style.props, attribute) if style else None
    if not style or not style_attr:
        try:
            style = base_style_scheme.get_style(name)
            style_attr = getattr(style.props, attribute)
        except AttributeError:
            pass

    if not style_attr:
        import sys

        err_msg = _("Couldn’t find color scheme details for {}; this is a bad install")
        print(err_msg.format(f"{name}-{attribute}"), file=sys.stderr)
        sys.exit(1)

    colour = Gdk.RGBA()
    colour.parse(style_attr)
    return colour


def _suite_colour_for(name: str, attribute: str) -> Gdk.RGBA | None:
    if _suite_fill_colours is None or _suite_line_colours is None:
        return None
    key = name.removeprefix("meld:")
    if attribute == "line-background" and key in _suite_line_colours:
        return _suite_line_colours[key].copy()
    if attribute in ("background", "foreground") and key in _suite_fill_colours:
        # focus-highlight maps from foreground in get_common_theme
        if key == "current-line-highlight" and attribute == "background":
            return _suite_fill_colours.get("current-chunk-highlight", _suite_fill_colours["insert"]).copy()
        if key == "syncpoint-outline" and attribute == "foreground":
            return _suite_fill_colours["focus-highlight"].copy()
        if key == "inline":
            return _suite_fill_colours.get("inline", _suite_fill_colours["replace"]).copy()
        if key == "dimmed" and attribute == "foreground":
            return _suite_fill_colours.get("dimmed", _suite_fill_colours["focus-highlight"]).copy()
        return _suite_fill_colours.get(key, _suite_fill_colours["replace"]).copy()
    return None


ColourMap = Mapping[str, Gdk.RGBA]


def _ensure_suite_chunk_colours() -> bool:
    """Pull chunk colours from the live suite Profile whenever possible."""
    global _suite_fill_colours, _suite_line_colours
    try:
        import sys
        from pathlib import Path

        # Ensure gtk-theme is importable from a target/ install.
        here = Path(__file__).resolve()
        for rel in (
            here.parents[2] / "gtk-theme" / "python",  # source: …/gtk-meld/meld
            here.parents[6] / "gtk-theme" / "python",  # target: …/gtk-meld/target/lib/…/meld
        ):
            if (rel / "gtk_theme.py").is_file() and str(rel) not in sys.path:
                sys.path.insert(0, str(rel))
                break

        import gtk_theme

        fill_hex, line_hex = gtk_theme.meld_chunk_colours(gtk_theme.load_profile())
        set_suite_chunk_colours(fill_hex, line_hex)
        return True
    except Exception:
        return _suite_fill_colours is not None


def get_common_theme() -> Tuple[ColourMap, ColourMap]:
    # Always refresh from the suite Profile so gutters/chunks never keep
    # stock Meld purple/blue after a Profile change (or a missed startup hook).
    _ensure_suite_chunk_colours()

    if _suite_fill_colours is not None and _suite_line_colours is not None:
        return _suite_fill_colours, _suite_line_colours

    lookup = colour_lookup_with_fallback
    fill_colours = {
        "insert": lookup("meld:insert", "background"),
        "delete": lookup("meld:insert", "background"),
        "conflict": lookup("meld:conflict", "background"),
        "replace": lookup("meld:replace", "background"),
        "error": lookup("meld:error", "background"),
        "focus-highlight": lookup("meld:current-line-highlight", "foreground"),
        "current-chunk-highlight": lookup("meld:current-chunk-highlight", "background"),
        "overscroll": lookup("meld:overscroll", "background"),
    }
    line_colours = {
        "insert": lookup("meld:insert", "line-background"),
        "delete": lookup("meld:insert", "line-background"),
        "conflict": lookup("meld:conflict", "line-background"),
        "replace": lookup("meld:replace", "line-background"),
        "error": lookup("meld:error", "line-background"),
    }
    return fill_colours, line_colours
