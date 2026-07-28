#!/usr/bin/env python3
"""Colors — GTK4 suite app with shared Profile theme menu."""

from __future__ import annotations

import math
import re
import sys
from pathlib import Path

import gi

gi.require_version("Gdk", "4.0")
gi.require_version("Gtk", "4.0")
from gi.repository import Gdk, GLib, Gtk

_THEME_PY = Path(__file__).resolve().parent.parent / "gtk-theme" / "python"
if str(_THEME_PY) not in sys.path:
    sys.path.insert(0, str(_THEME_PY))
import gtk_theme  # noqa: E402

APP_ID = "org.neuronix.GtkColors"

PALETTE_MODES = (
    ("similar", "5 similar (analogous hues)"),
    ("opposite", "5 opposite (around complement hue)"),
    ("complementary", "5 complementary (base + complement cluster)"),
)


def _bind_commit(entry: Gtk.Entry, callback) -> None:
    """Commit on Enter or focus leave (GTK4 replacement for focus-out-event)."""
    entry.connect("activate", lambda *_: callback())
    focus = Gtk.EventControllerFocus()
    focus.connect("leave", lambda *_: callback())
    entry.add_controller(focus)


class ColorsWindow(Gtk.ApplicationWindow):
    def __init__(self, app: Gtk.Application):
        super().__init__(application=app, title="Colors")
        self.set_default_size(600, 800)

        self.current_rgb = (255, 251, 0)
        self.updating_fields = False
        self.palette_rgb = [(0, 0, 0)] * 5
        self._palette_mode_ids = [m[0] for m in PALETTE_MODES]

        header = Gtk.HeaderBar()
        header.set_show_title_buttons(True)
        header.set_title_widget(Gtk.Label(label="Colors"))
        self.set_titlebar(header)
        gtk_theme.attach_profile_menu(
            self,
            header,
            about_name="GTK Colors",
            about_comments="Color picker and format converter for the Neuronix GTK-Apps suite.",
        )

        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_vexpand(True)
        self.set_child(scrolled)

        vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        vbox.set_margin_start(20)
        vbox.set_margin_end(20)
        vbox.set_margin_top(20)
        vbox.set_margin_bottom(20)
        scrolled.set_child(vbox)

        self.color_preview = Gtk.DrawingArea()
        self.color_preview.set_content_width(200)
        self.color_preview.set_content_height(100)
        self.color_preview.set_size_request(-1, 100)
        self.color_preview.set_draw_func(self.on_color_preview_draw)
        vbox.append(self.color_preview)

        color_button = Gtk.Button(label="GTK Color Picker")
        color_button.connect("clicked", self.on_color_picker_clicked)
        vbox.append(color_button)

        pal_frame = Gtk.Frame(label="Color palette (5 swatches)")
        pal_frame.set_margin_top(10)
        vbox.append(pal_frame)
        pal_vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        pal_vbox.set_margin_start(10)
        pal_vbox.set_margin_end(10)
        pal_vbox.set_margin_top(10)
        pal_vbox.set_margin_bottom(10)
        pal_frame.set_child(pal_vbox)

        mode_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        mode_row.append(Gtk.Label(label="Mode:"))
        mode_labels = [m[1] for m in PALETTE_MODES]
        self.palette_mode_dropdown = Gtk.DropDown.new_from_strings(mode_labels)
        self.palette_mode_dropdown.set_selected(0)
        self.palette_mode_dropdown.set_hexpand(True)
        self.palette_mode_dropdown.connect("notify::selected", self.on_palette_control_changed)
        mode_row.append(self.palette_mode_dropdown)
        pal_vbox.append(mode_row)

        sat_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.sat_label = Gtk.Label(label="Saturation: 100%")
        self.sat_label.set_halign(Gtk.Align.START)
        self.sat_label.set_size_request(160, -1)
        sat_row.append(self.sat_label)
        self.sat_adj = Gtk.Adjustment(
            value=100, lower=0, upper=200, step_increment=1, page_increment=10, page_size=0
        )
        self.sat_scale = Gtk.Scale(orientation=Gtk.Orientation.HORIZONTAL, adjustment=self.sat_adj)
        self.sat_scale.set_digits(0)
        self.sat_scale.set_draw_value(True)
        self.sat_scale.set_value_pos(Gtk.PositionType.RIGHT)
        self.sat_scale.set_hexpand(True)
        self.sat_scale.connect("value-changed", self.on_palette_control_changed)
        sat_row.append(self.sat_scale)
        pal_vbox.append(sat_row)

        light_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.light_label = Gtk.Label(label="Lightness: 0")
        self.light_label.set_halign(Gtk.Align.START)
        self.light_label.set_size_request(160, -1)
        light_row.append(self.light_label)
        self.light_adj = Gtk.Adjustment(
            value=0, lower=-50, upper=50, step_increment=1, page_increment=5, page_size=0
        )
        self.light_scale = Gtk.Scale(
            orientation=Gtk.Orientation.HORIZONTAL, adjustment=self.light_adj
        )
        self.light_scale.set_digits(0)
        self.light_scale.set_draw_value(True)
        self.light_scale.set_value_pos(Gtk.PositionType.RIGHT)
        self.light_scale.set_hexpand(True)
        self.light_scale.connect("value-changed", self.on_palette_control_changed)
        light_row.append(self.light_scale)
        pal_vbox.append(light_row)

        self.palette_swatches = []
        self.palette_hex_labels = []
        sw_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        for i in range(5):
            col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
            da = Gtk.DrawingArea()
            da.set_content_width(70)
            da.set_content_height(56)
            da.set_size_request(70, 56)
            da.set_draw_func(self._make_palette_swatch_draw(i))
            col.append(da)
            hx = Gtk.Label(label="#000000")
            hx.set_halign(Gtk.Align.CENTER)
            col.append(hx)
            self.palette_swatches.append(da)
            self.palette_hex_labels.append(hx)
            col.set_hexpand(True)
            sw_row.append(col)
        pal_vbox.append(sw_row)

        css_h = Gtk.Label()
        css_h.set_markup(
            "CSS custom properties (paste into <tt>:root { … }</tt> in your stylesheet):"
        )
        css_h.set_halign(Gtk.Align.START)
        pal_vbox.append(css_h)

        css_name_help = Gtk.Label()
        css_name_help.set_markup(
            "Name each color for export: type the variable name only (not the leading <tt>--</tt>).\n"
            "If you leave a field empty, the export uses <tt>swatch-1</tt> … <tt>swatch-5</tt> for those slots."
        )
        css_name_help.set_halign(Gtk.Align.START)
        css_name_help.set_xalign(0.0)
        pal_vbox.append(css_name_help)

        name_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        self.css_name_entries = []
        ph = (
            "e.g. primary",
            "e.g. accent",
            "e.g. surface",
            "e.g. muted",
            "e.g. border",
        )
        for i in range(5):
            coln = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
            lab = Gtk.Label()
            lab.set_markup(f"— {i + 1} —\n<small>name</small>")
            lab.set_halign(Gtk.Align.CENTER)
            coln.append(lab)
            ne = Gtk.Entry()
            ne.set_placeholder_text(ph[i])
            ne.set_width_chars(12)
            ne.set_tooltip_text(
                "This text becomes --your-name in the lines below. "
                "Use letters, numbers, and hyphens (e.g. light-blue, gray-bg)."
            )
            ne.connect("changed", self.on_css_name_changed)
            coln.append(ne)
            self.css_name_entries.append(ne)
            coln.set_hexpand(True)
            name_row.append(coln)
        pal_vbox.append(name_row)

        css_scrolled = Gtk.ScrolledWindow()
        css_scrolled.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        css_scrolled.set_min_content_height(120)
        self.css_text_view = Gtk.TextView()
        self.css_text_view.set_editable(False)
        self.css_text_view.set_monospace(True)
        self.css_text_view.set_wrap_mode(Gtk.WrapMode.NONE)
        self.css_text_buffer = self.css_text_view.get_buffer()
        self.css_text_view.set_left_margin(4)
        self.css_text_view.set_right_margin(4)
        css_scrolled.set_child(self.css_text_view)
        pal_vbox.append(css_scrolled)

        copy_css = Gtk.Button(label="Copy CSS to clipboard")
        copy_css.connect("clicked", self.on_copy_css_to_clipboard)
        pal_vbox.append(copy_css)

        info_frame = Gtk.Frame(label="Color Information")
        info_frame.set_margin_top(10)
        info_frame.set_vexpand(True)
        vbox.append(info_frame)

        info_vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=5)
        info_vbox.set_margin_start(10)
        info_vbox.set_margin_end(10)
        info_vbox.set_margin_top(10)
        info_vbox.set_margin_bottom(10)
        info_frame.set_child(info_vbox)

        rgb_label = Gtk.Label(label="RGB:")
        rgb_label.set_halign(Gtk.Align.START)
        info_vbox.append(rgb_label)

        rgb_entry_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=5)
        rgb_entry_box.set_margin_start(10)

        self.r_entry = Gtk.Entry()
        self.r_entry.set_width_chars(5)
        self.r_entry.set_placeholder_text("R (0-255)")
        _bind_commit(self.r_entry, self.on_rgb_entry_changed)
        rgb_entry_box.append(self.r_entry)

        self.g_entry = Gtk.Entry()
        self.g_entry.set_width_chars(5)
        self.g_entry.set_placeholder_text("G (0-255)")
        _bind_commit(self.g_entry, self.on_rgb_entry_changed)
        rgb_entry_box.append(self.g_entry)

        self.b_entry = Gtk.Entry()
        self.b_entry.set_width_chars(5)
        self.b_entry.set_placeholder_text("B (0-255)")
        _bind_commit(self.b_entry, self.on_rgb_entry_changed)
        rgb_entry_box.append(self.b_entry)
        info_vbox.append(rgb_entry_box)

        hex_label = Gtk.Label(label="Hex:")
        hex_label.set_halign(Gtk.Align.START)
        info_vbox.append(hex_label)

        hex_entry_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=5)
        hex_entry_box.set_margin_start(10)
        self.hex_entry = Gtk.Entry()
        self.hex_entry.set_width_chars(8)
        self.hex_entry.set_placeholder_text("#RRGGBB")
        self.hex_entry.set_hexpand(True)
        _bind_commit(self.hex_entry, self.on_hex_entry_changed)
        hex_entry_box.append(self.hex_entry)
        info_vbox.append(hex_entry_box)

        self.format_entries = {}
        formats = [
            ("HSL", "H S L (e.g., 60 100 50)"),
            ("HSV", "H S V (e.g., 60 100 100)"),
            ("CMYK", "C M Y K (e.g., 0 2 100 0)"),
            ("XYZ", "X Y Z (e.g., 95.05 100.00 10.87)"),
            ("CIELAB", "L* a* b* (e.g., 97.14 -21.55 94.48)"),
            ("HWB", "H W B (e.g., 60 0 0)"),
            ("CIELCh", "L* C* h° (e.g., 97.14 97.00 102.87)"),
            ("LMS", "L M S (e.g., 100.00 100.00 10.87)"),
            ("Hunter Lab", "L a b (e.g., 100.00 -10.78 47.24)"),
            ("RGB565", "Decimal or 0xHex (e.g., 65472 or 0xffc0)"),
        ]
        for fmt_name, placeholder in formats:
            label = Gtk.Label(label=f"{fmt_name}:")
            label.set_halign(Gtk.Align.START)
            info_vbox.append(label)
            entry_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=5)
            entry_box.set_margin_start(10)
            entry = Gtk.Entry()
            entry.set_width_chars(40)
            entry.set_placeholder_text(placeholder)
            entry.set_hexpand(True)
            _bind_commit(entry, lambda f=fmt_name: self.on_format_entry_changed(f))
            entry_box.append(entry)
            self.format_entries[fmt_name] = entry
            info_vbox.append(entry_box)

        self.update_color_display()

    def on_color_preview_draw(self, _area, cr, width, height, _data=None):
        r, g, b = self.current_rgb
        cr.set_source_rgb(r / 255.0, g / 255.0, b / 255.0)
        cr.rectangle(0, 0, width, height)
        cr.fill()

    def _make_palette_swatch_draw(self, index):
        def draw_swatch(_area, cr, width, height, _data=None):
            r, g, b = self.palette_rgb[index]
            cr.set_source_rgb(r / 255.0, g / 255.0, b / 255.0)
            cr.rectangle(0, 0, width, height)
            cr.fill()

        return draw_swatch

    def _palette_mode_id(self) -> str:
        idx = int(self.palette_mode_dropdown.get_selected())
        if 0 <= idx < len(self._palette_mode_ids):
            return self._palette_mode_ids[idx]
        return "similar"

    def on_palette_control_changed(self, *_args):
        v = int(self.sat_adj.get_value())
        self.sat_label.set_text(f"Saturation: {v}% of base")
        o = int(self.light_adj.get_value())
        sign = "+" if o > 0 else ""
        self.light_label.set_text(f"Lightness: {sign}{o} (HSL)")
        self.update_palette_display()

    def _palette_hues_for_mode(self, mode_id, base_h):
        base_h = base_h % 360.0
        step = 28.0
        if mode_id == "similar":
            return [(base_h + (i - 2) * step) % 360.0 for i in range(5)]
        if mode_id == "opposite":
            center = (base_h + 180.0) % 360.0
            return [(center + (i - 2) * step) % 360.0 for i in range(5)]
        c = (base_h + 180.0) % 360.0
        return [
            base_h,
            (base_h + step) % 360.0,
            (c - step) % 360.0,
            c,
            (c + step) % 360.0,
        ]

    def update_palette_display(self):
        mode_id = self._palette_mode_id()
        r0, g0, b0 = self.current_rgb
        h0, s0, l0 = self.rgb_to_hsl(r0, g0, b0)
        sat_mul = self.sat_adj.get_value() / 100.0
        s1 = max(0.0, min(100.0, s0 * sat_mul))
        l1 = max(0.0, min(100.0, l0 + self.light_adj.get_value()))
        hues = self._palette_hues_for_mode(mode_id, h0)
        for i, hu in enumerate(hues):
            self.palette_rgb[i] = self.hsl_to_rgb(hu, s1, l1)
        for i, da in enumerate(self.palette_swatches):
            rr, gg, bb = self.palette_rgb[i]
            self.palette_hex_labels[i].set_text(f"#{rr:02x}{gg:02x}{bb:02x}")
            da.queue_draw()
        self._refresh_css_export()

    def _sanitize_css_var_name(self, raw, fallback):
        s = (raw or "").strip()
        s = re.sub(r"\s+", "-", s)
        s = re.sub(r"[^A-Za-z0-9_-]", "", s)
        s = s.strip("-")
        if not s:
            return fallback
        if s[0].isdigit():
            s = "n-" + s
        return s

    def _refresh_css_export(self):
        if not hasattr(self, "css_text_buffer"):
            return
        lines = []
        indent = "      "
        for i in range(5):
            fb = f"swatch-{i + 1}"
            name = self._sanitize_css_var_name(self.css_name_entries[i].get_text(), fb)
            rr, gg, bb = self.palette_rgb[i]
            hx = f"#{rr:02x}{gg:02x}{bb:02x}"
            lines.append(f"{indent}--{name}: {hx};")
        self.css_text_buffer.set_text("\n".join(lines) + "\n")

    def on_css_name_changed(self, _entry):
        self._refresh_css_export()

    def on_copy_css_to_clipboard(self, _button):
        start = self.css_text_buffer.get_start_iter()
        end = self.css_text_buffer.get_end_iter()
        text = self.css_text_buffer.get_text(start, end, True)
        self.get_clipboard().set(text)


    # Color conversion helpers
    def rgb_to_hsl(self, r, g, b):
        """Convert RGB to HSL"""
        r, g, b = r / 255.0, g / 255.0, b / 255.0
        max_val = max(r, g, b)
        min_val = min(r, g, b)
        delta = max_val - min_val
        
        l = (max_val + min_val) / 2.0
        
        if delta == 0:
            h = s = 0
        else:
            s = delta / (1 - abs(2 * l - 1)) if l != 0.5 else delta
            
            if max_val == r:
                h = ((g - b) / delta) % 6
            elif max_val == g:
                h = (b - r) / delta + 2
            else:
                h = (r - g) / delta + 4
            h *= 60
        
        return (h, s * 100, l * 100)
    
    def rgb_to_hsv(self, r, g, b):
        """Convert RGB to HSV"""
        r, g, b = r / 255.0, g / 255.0, b / 255.0
        max_val = max(r, g, b)
        min_val = min(r, g, b)
        delta = max_val - min_val
        
        v = max_val * 100
        
        if delta == 0:
            h = s = 0
        else:
            s = (delta / max_val) * 100
            
            if max_val == r:
                h = ((g - b) / delta) % 6
            elif max_val == g:
                h = (b - r) / delta + 2
            else:
                h = (r - g) / delta + 4
            h *= 60
        
        return (h, s, v)
    
    def rgb_to_cmyk(self, r, g, b):
        """Convert RGB to CMYK"""
        r, g, b = r / 255.0, g / 255.0, b / 255.0
        k = 1 - max(r, g, b)
        if k == 1:
            return (0, 0, 0, 100)
        c = (1 - r - k) / (1 - k) * 100
        m = (1 - g - k) / (1 - k) * 100
        y = (1 - b - k) / (1 - k) * 100
        return (c, m, y, k * 100)
    
    def rgb_to_xyz(self, r, g, b):
        """Convert RGB to XYZ (D65 illuminant)"""
        r, g, b = r / 255.0, g / 255.0, b / 255.0
        
        r = r / 12.92 if r <= 0.04045 else ((r + 0.055) / 1.055) ** 2.4
        g = g / 12.92 if g <= 0.04045 else ((g + 0.055) / 1.055) ** 2.4
        b = b / 12.92 if b <= 0.04045 else ((b + 0.055) / 1.055) ** 2.4
        
        x = r * 0.4124564 + g * 0.3575761 + b * 0.1804375
        y = r * 0.2126729 + g * 0.7151522 + b * 0.0721750
        z = r * 0.0193339 + g * 0.1191920 + b * 0.9503041
        
        return (x * 100, y * 100, z * 100)
    
    def xyz_to_lab(self, x, y, z):
        """Convert XYZ to CIELAB"""
        xn, yn, zn = 95.047, 100.000, 108.883
        x, y, z = x / xn, y / yn, z / zn
        
        fx = x ** (1/3) if x > 0.008856 else (7.787 * x + 16/116)
        fy = y ** (1/3) if y > 0.008856 else (7.787 * y + 16/116)
        fz = z ** (1/3) if z > 0.008856 else (7.787 * z + 16/116)
        
        l = 116 * fy - 16
        a = 500 * (fx - fy)
        b = 200 * (fy - fz)
        
        return (l, a, b)
    
    def rgb_to_lab(self, r, g, b):
        """Convert RGB to CIELAB"""
        x, y, z = self.rgb_to_xyz(r, g, b)
        return self.xyz_to_lab(x, y, z)
    
    def rgb_to_hwb(self, r, g, b):
        """Convert RGB to HWB"""
        h, s, v = self.rgb_to_hsv(r, g, b)
        w = min(r, g, b) / 255.0 * 100
        b_val = (1 - max(r, g, b) / 255.0) * 100
        return (h, w, b_val)
    
    def lab_to_lch(self, l, a, b):
        """Convert CIELAB to CIELCh"""
        c = math.sqrt(a * a + b * b)
        h = math.atan2(b, a) * 180 / math.pi
        if h < 0:
            h += 360
        return (l, c, h)
    
    def rgb_to_lch(self, r, g, b):
        """Convert RGB to CIELCh"""
        l, a, b = self.rgb_to_lab(r, g, b)
        return self.lab_to_lch(l, a, b)
    
    def rgb_to_lms(self, r, g, b):
        """Convert RGB to LMS (cone response)"""
        x, y, z = self.rgb_to_xyz(r, g, b)
        l = x * 0.38971 + y * 0.68898 + z * -0.07868
        m = x * 0.22981 + y * 0.83842 + z * 0.04677
        s = x * 0.00000 + y * 0.00000 + z * 1.00000
        return (l, m, s)
    
    def xyz_to_hunter_lab(self, x, y, z):
        """Convert XYZ to Hunter Lab"""
        xn, yn, zn = 95.047, 100.000, 108.883
        ka = 175.0 / 198.04
        kb = 70.0 / 218.11
        
        l = 100 * math.sqrt(y / yn)
        a = ka * ((x / xn - y / yn) / math.sqrt(y / yn)) * 100
        b = kb * ((y / yn - z / zn) / math.sqrt(y / yn)) * 100
        
        return (l, a, b)
    
    def rgb_to_hunter_lab(self, r, g, b):
        """Convert RGB to Hunter Lab"""
        x, y, z = self.rgb_to_xyz(r, g, b)
        return self.xyz_to_hunter_lab(x, y, z)
    
    def rgb_to_rgb565(self, r, g, b):
        """Convert RGB888 to RGB565"""
        r = max(0, min(255, r))
        g = max(0, min(255, g))
        b = max(0, min(255, b))
        
        r5 = (r >> 3) & 0x1F
        g6 = (g >> 2) & 0x3F
        b5 = (b >> 3) & 0x1F
        
        rgb565 = (r5 << 11) | (g6 << 5) | b5
        return rgb565
    
    # Reverse conversion functions (other formats to RGB)
    def hsl_to_rgb(self, h, s, l):
        """Convert HSL to RGB"""
        h = h % 360
        s = s / 100.0
        l = l / 100.0
        
        c = (1 - abs(2 * l - 1)) * s
        x = c * (1 - abs((h / 60) % 2 - 1))
        m = l - c / 2
        
        if h < 60:
            r, g, b = c, x, 0
        elif h < 120:
            r, g, b = x, c, 0
        elif h < 180:
            r, g, b = 0, c, x
        elif h < 240:
            r, g, b = 0, x, c
        elif h < 300:
            r, g, b = x, 0, c
        else:
            r, g, b = c, 0, x
        
        return (int(round((r + m) * 255)), int(round((g + m) * 255)), int(round((b + m) * 255)))
    
    def hsv_to_rgb(self, h, s, v):
        """Convert HSV to RGB"""
        h = h % 360
        s = s / 100.0
        v = v / 100.0
        
        c = v * s
        x = c * (1 - abs((h / 60) % 2 - 1))
        m = v - c
        
        if h < 60:
            r, g, b = c, x, 0
        elif h < 120:
            r, g, b = x, c, 0
        elif h < 180:
            r, g, b = 0, c, x
        elif h < 240:
            r, g, b = 0, x, c
        elif h < 300:
            r, g, b = x, 0, c
        else:
            r, g, b = c, 0, x
        
        return (int(round((r + m) * 255)), int(round((g + m) * 255)), int(round((b + m) * 255)))
    
    def cmyk_to_rgb(self, c, m, y, k):
        """Convert CMYK to RGB"""
        c, m, y, k = c / 100.0, m / 100.0, y / 100.0, k / 100.0
        r = (1 - c) * (1 - k) * 255
        g = (1 - m) * (1 - k) * 255
        b = (1 - y) * (1 - k) * 255
        return (int(round(r)), int(round(g)), int(round(b)))
    
    def xyz_to_rgb(self, x, y, z):
        """Convert XYZ to RGB"""
        x, y, z = x / 100.0, y / 100.0, z / 100.0
        
        # Inverse sRGB matrix
        r = x * 3.2404542 + y * -1.5371385 + z * -0.4985314
        g = x * -0.9692660 + y * 1.8760108 + z * 0.0415560
        b = x * 0.0556434 + y * -0.2040259 + z * 1.0572252
        
        # Gamma correction
        r = 12.92 * r if r <= 0.0031308 else 1.055 * (r ** (1/2.4)) - 0.055
        g = 12.92 * g if g <= 0.0031308 else 1.055 * (g ** (1/2.4)) - 0.055
        b = 12.92 * b if b <= 0.0031308 else 1.055 * (b ** (1/2.4)) - 0.055
        
        r = max(0, min(255, int(round(r * 255))))
        g = max(0, min(255, int(round(g * 255))))
        b = max(0, min(255, int(round(b * 255))))
        
        return (r, g, b)
    
    def lab_to_xyz(self, l, a, b):
        """Convert CIELAB to XYZ"""
        xn, yn, zn = 95.047, 100.000, 108.883
        
        fy = (l + 16) / 116
        fx = a / 500 + fy
        fz = fy - b / 200
        
        x = fx ** 3 if fx ** 3 > 0.008856 else (fx - 16/116) / 7.787
        y = fy ** 3 if fy ** 3 > 0.008856 else (fy - 16/116) / 7.787
        z = fz ** 3 if fz ** 3 > 0.008856 else (fz - 16/116) / 7.787
        
        return (x * xn, y * yn, z * zn)
    
    def lab_to_rgb(self, l, a, b):
        """Convert CIELAB to RGB"""
        x, y, z = self.lab_to_xyz(l, a, b)
        return self.xyz_to_rgb(x, y, z)
    
    def hwb_to_rgb(self, h, w, b_val):
        """Convert HWB to RGB"""
        # HWB is similar to HSV, but with whiteness and blackness
        # Convert to HSV first
        v = 100 - b_val
        s = 100 - w if v > 0 else 0
        return self.hsv_to_rgb(h, s, v)
    
    def lch_to_lab(self, l, c, h):
        """Convert CIELCh to CIELAB"""
        h_rad = math.radians(h)
        a = c * math.cos(h_rad)
        b = c * math.sin(h_rad)
        return (l, a, b)
    
    def lch_to_rgb(self, l, c, h):
        """Convert CIELCh to RGB"""
        l_val, a, b = self.lch_to_lab(l, c, h)
        return self.lab_to_rgb(l_val, a, b)
    
    def lms_to_xyz(self, l, m, s):
        """Convert LMS to XYZ"""
        # Inverse Hunt-Pointer-Estevez matrix
        x = l * 1.91019 + m * -1.11214 + s * 0.20195
        y = l * 0.37095 + m * 0.62905 + s * 0.00000
        z = l * 0.00000 + m * 0.00000 + s * 1.00000
        return (x, y, z)
    
    def lms_to_rgb(self, l, m, s):
        """Convert LMS to RGB"""
        x, y, z = self.lms_to_xyz(l, m, s)
        return self.xyz_to_rgb(x, y, z)
    
    def hunter_lab_to_xyz(self, l, a, b):
        """Convert Hunter Lab to XYZ"""
        xn, yn, zn = 95.047, 100.000, 108.883
        ka = 175.0 / 198.04
        kb = 70.0 / 218.11
        
        y = ((l / 100) ** 2) * yn
        x = ((a / (ka * 100)) * math.sqrt(y / yn) + y / yn) * xn
        z = (y / yn - (b / (kb * 100)) * math.sqrt(y / yn)) * zn
        
        return (x, y, z)
    
    def hunter_lab_to_rgb(self, l, a, b):
        """Convert Hunter Lab to RGB"""
        x, y, z = self.hunter_lab_to_xyz(l, a, b)
        return self.xyz_to_rgb(x, y, z)
    
    def rgb565_to_rgb(self, rgb565):
        """Convert RGB565 to RGB888"""
        r5 = (rgb565 >> 11) & 0x1F
        g6 = (rgb565 >> 5) & 0x3F
        b5 = rgb565 & 0x1F
        
        r = (r5 << 3) | (r5 >> 2)
        g = (g6 << 2) | (g6 >> 4)
        b = (b5 << 3) | (b5 >> 2)
        
        return (r, g, b)
    

    def update_color_display(self):
        """Update all color information displays"""
        if self.updating_fields:
            return

        self.updating_fields = True
        r, g, b = self.current_rgb

        self.r_entry.set_text(str(r))
        self.g_entry.set_text(str(g))
        self.b_entry.set_text(str(b))

        hex_color = f"#{r:02x}{g:02x}{b:02x}"
        self.hex_entry.set_text(hex_color)

        hsl = self.rgb_to_hsl(r, g, b)
        self.format_entries["HSL"].set_text(f"{hsl[0]:.1f} {hsl[1]:.1f} {hsl[2]:.1f}")

        hsv = self.rgb_to_hsv(r, g, b)
        self.format_entries["HSV"].set_text(f"{hsv[0]:.1f} {hsv[1]:.1f} {hsv[2]:.1f}")

        cmyk = self.rgb_to_cmyk(r, g, b)
        self.format_entries["CMYK"].set_text(
            f"{cmyk[0]:.1f} {cmyk[1]:.1f} {cmyk[2]:.1f} {cmyk[3]:.1f}"
        )

        xyz = self.rgb_to_xyz(r, g, b)
        self.format_entries["XYZ"].set_text(f"{xyz[0]:.2f} {xyz[1]:.2f} {xyz[2]:.2f}")

        lab = self.rgb_to_lab(r, g, b)
        self.format_entries["CIELAB"].set_text(f"{lab[0]:.2f} {lab[1]:.2f} {lab[2]:.2f}")

        hwb = self.rgb_to_hwb(r, g, b)
        self.format_entries["HWB"].set_text(f"{hwb[0]:.1f} {hwb[1]:.1f} {hwb[2]:.1f}")

        lch = self.rgb_to_lch(r, g, b)
        self.format_entries["CIELCh"].set_text(f"{lch[0]:.2f} {lch[1]:.2f} {lch[2]:.1f}")

        lms = self.rgb_to_lms(r, g, b)
        self.format_entries["LMS"].set_text(f"{lms[0]:.2f} {lms[1]:.2f} {lms[2]:.2f}")

        hunter_lab = self.rgb_to_hunter_lab(r, g, b)
        self.format_entries["Hunter Lab"].set_text(
            f"{hunter_lab[0]:.2f} {hunter_lab[1]:.2f} {hunter_lab[2]:.2f}"
        )

        rgb565 = self.rgb_to_rgb565(r, g, b)
        self.format_entries["RGB565"].set_text(f"{rgb565} (0x{rgb565:04x})")

        self.color_preview.queue_draw()
        self.updating_fields = False
        self.update_palette_display()

    def on_rgb_entry_changed(self, *_args):
        if self.updating_fields:
            return
        try:
            r = int(self.r_entry.get_text() or 0)
            g = int(self.g_entry.get_text() or 0)
            b = int(self.b_entry.get_text() or 0)
            r = max(0, min(255, r))
            g = max(0, min(255, g))
            b = max(0, min(255, b))
            self.current_rgb = (r, g, b)
            self.update_color_display()
        except ValueError:
            self.update_color_display()

    def on_hex_entry_changed(self, *_args):
        if self.updating_fields:
            return
        try:
            hex_text = self.hex_entry.get_text().strip()
            if hex_text.startswith("#"):
                hex_text = hex_text[1:]
            if len(hex_text) == 6:
                r = int(hex_text[0:2], 16)
                g = int(hex_text[2:4], 16)
                b = int(hex_text[4:6], 16)
                self.current_rgb = (r, g, b)
                self.update_color_display()
        except (ValueError, IndexError):
            self.update_color_display()

    def on_format_entry_changed(self, format_name):
        if self.updating_fields:
            return
        try:
            entry = self.format_entries[format_name]
            text = entry.get_text().strip()
            if not text:
                return
            values = re.findall(r"[-+]?\d*\.?\d+", text)

            if format_name == "HSL":
                if len(values) >= 3:
                    h, s, l = float(values[0]), float(values[1]), float(values[2])
                    self.current_rgb = self.hsl_to_rgb(h, s, l)
                    self.update_color_display()
            elif format_name == "HSV":
                if len(values) >= 3:
                    h, s, v = float(values[0]), float(values[1]), float(values[2])
                    self.current_rgb = self.hsv_to_rgb(h, s, v)
                    self.update_color_display()
            elif format_name == "CMYK":
                if len(values) >= 4:
                    c, m, y, k = (
                        float(values[0]),
                        float(values[1]),
                        float(values[2]),
                        float(values[3]),
                    )
                    self.current_rgb = self.cmyk_to_rgb(c, m, y, k)
                    self.update_color_display()
            elif format_name == "XYZ":
                if len(values) >= 3:
                    x, y, z = float(values[0]), float(values[1]), float(values[2])
                    self.current_rgb = self.xyz_to_rgb(x, y, z)
                    self.update_color_display()
            elif format_name == "CIELAB":
                if len(values) >= 3:
                    l, a, b = float(values[0]), float(values[1]), float(values[2])
                    self.current_rgb = self.lab_to_rgb(l, a, b)
                    self.update_color_display()
            elif format_name == "HWB":
                if len(values) >= 3:
                    h, w, b = float(values[0]), float(values[1]), float(values[2])
                    self.current_rgb = self.hwb_to_rgb(h, w, b)
                    self.update_color_display()
            elif format_name == "CIELCh":
                if len(values) >= 3:
                    l, c, h = float(values[0]), float(values[1]), float(values[2])
                    self.current_rgb = self.lch_to_rgb(l, c, h)
                    self.update_color_display()
            elif format_name == "LMS":
                if len(values) >= 3:
                    l, m, s = float(values[0]), float(values[1]), float(values[2])
                    self.current_rgb = self.lms_to_rgb(l, m, s)
                    self.update_color_display()
            elif format_name == "Hunter Lab":
                if len(values) >= 3:
                    l, a, b = float(values[0]), float(values[1]), float(values[2])
                    self.current_rgb = self.hunter_lab_to_rgb(l, a, b)
                    self.update_color_display()
            elif format_name == "RGB565":
                text = text.replace("(", "").replace(")", "").strip()
                parts = re.split(r"0[xX]", text, maxsplit=1)
                if len(parts) > 1:
                    hex_part = parts[1].split()[0]
                    rgb565 = int(hex_part, 16)
                else:
                    rgb565 = int(float(values[0]))
                rgb565 = max(0, min(65535, rgb565))
                self.current_rgb = self.rgb565_to_rgb(rgb565)
                self.update_color_display()
        except (ValueError, IndexError):
            pass

    def on_color_picker_clicked(self, _widget):
        dialog = Gtk.ColorDialog()
        dialog.set_title("Choose a color")
        dialog.set_with_alpha(False)
        r, g, b = self.current_rgb
        initial = Gdk.RGBA()
        initial.red = r / 255.0
        initial.green = g / 255.0
        initial.blue = b / 255.0
        initial.alpha = 1.0

        def on_done(dlg, result):
            try:
                color = dlg.choose_rgba_finish(result)
            except GLib.Error:
                return
            self.updating_fields = False
            self.current_rgb = (
                int(round(color.red * 255)),
                int(round(color.green * 255)),
                int(round(color.blue * 255)),
            )
            self.update_color_display()

        dialog.choose_rgba(self, initial, None, on_done)


class ColorsApp(Gtk.Application):
    def __init__(self):
        super().__init__(application_id=APP_ID)

    def do_activate(self):
        win = self.props.active_window
        if win is None:
            win = ColorsWindow(self)
        win.present()


def main():
    app = ColorsApp()
    return app.run(sys.argv)


if __name__ == "__main__":
    raise SystemExit(main())
