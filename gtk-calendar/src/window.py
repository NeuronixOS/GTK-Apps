"""Main gtk-calendar window: month grid, agenda, Google/.ics sources."""

from __future__ import annotations

import calendar as pycal
import html
import sys
import threading
from datetime import date, datetime, timedelta
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, Gio, GLib, Gtk, Pango

from . import google_api
from .ics import load_ics_source, normalize_ics_url
from .models import LOCAL_TZ, CalendarEvent, CalendarSource, as_local, _clock
from .paths import cache_dir
from .store import Store


def _ensure_gtk_theme_on_path() -> None:
    here = Path(__file__).resolve().parent
    cands: list[Path] = []
    p = here
    for _ in range(8):
        cands.append(p / "python")
        cands.append(p / "gtk-theme" / "python")
        if p.parent == p:
            break
        p = p.parent
    cands.extend(
        (
            Path("/usr/local/lib/neuronix/gtk-apps/gtk-theme/python"),
            Path("/usr/local/lib/neuronix/gtk-apps/python"),
            Path("/usr/share/neuronix/gtk-theme/python"),
        )
    )
    for d in cands:
        try:
            if (d / "gtk_theme.py").is_file():
                s = str(d)
                if s not in sys.path:
                    sys.path.insert(0, s)
                return
        except OSError:
            continue


_ensure_gtk_theme_on_path()
import gtk_theme  # noqa: E402


WEEKDAY_MON = ("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun")
WEEKDAY_SUN = ("Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat")


def _alert(parent: Gtk.Window, title: str, detail: str) -> None:
    dialog = Gtk.AlertDialog()
    dialog.set_modal(True)
    dialog.set_message(title)
    dialog.set_detail(detail)
    dialog.set_buttons(["OK"])
    dialog.show(parent)


class CalendarWindow(Gtk.ApplicationWindow):
    def __init__(self, app: Gtk.Application):
        super().__init__(application=app, title="Calendar")
        self.add_css_class("gtk-calendar")
        self.set_default_size(1280, 820)
        self.store = Store()
        today = date.today()
        self.view_year = today.year
        self.view_month = today.month
        self._week_anchor = today
        self.selected = today
        self._clicked_day: date | None = None
        self._view_guard = False
        self.events: list[CalendarEvent] = []
        self._events_by_day: dict[date, list[CalendarEvent]] = {}
        self._busy = False
        self._refresh_gen = 0
        self._css = Gtk.CssProvider()
        self._day_buttons: list[Gtk.Button] = []

        header = Gtk.HeaderBar()
        header.set_show_title_buttons(True)
        self._title = Gtk.Label(label="Calendar")
        self._title.add_css_class("title")
        header.set_title_widget(self._title)
        self.set_titlebar(header)
        gtk_theme.attach_profile_menu(
            self,
            header,
            about_name="GTK Calendar",
            about_comments=(
                "Month calendar for the Neuronix GTK-Apps suite. "
                "Sign in to Google Calendar or add .ics files and webcal URLs."
            ),
        )

        prev_btn = Gtk.Button.new_from_icon_name("go-previous-symbolic")
        prev_btn.connect("clicked", lambda *_: self._shift_view(-1))
        header.pack_start(prev_btn)
        self._prev_btn = prev_btn
        next_btn = Gtk.Button.new_from_icon_name("go-next-symbolic")
        next_btn.connect("clicked", lambda *_: self._shift_view(1))
        header.pack_start(next_btn)
        self._next_btn = next_btn
        today_btn = Gtk.Button(label="Today")
        today_btn.connect("clicked", lambda *_: self.go_today())
        header.pack_start(today_btn)

        view_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        view_box.add_css_class("linked")
        self._month_btn = Gtk.ToggleButton(label="Month")
        self._week_btn = Gtk.ToggleButton(label="Week")
        self._week_btn.set_group(self._month_btn)
        self._month_btn.set_active(self.store.view_mode != "week")
        self._week_btn.set_active(self.store.view_mode == "week")
        self._month_btn.connect("toggled", self._on_view_toggled)
        self._week_btn.connect("toggled", self._on_view_toggled)
        view_box.append(self._month_btn)
        view_box.append(self._week_btn)
        header.pack_start(view_box)
        self._sync_nav_tooltips()

        refresh_btn = Gtk.Button.new_from_icon_name("view-refresh-symbolic")
        refresh_btn.set_tooltip_text("Refresh calendars")
        refresh_btn.connect("clicked", lambda *_: self.refresh_async())
        header.pack_end(refresh_btn)
        sources_btn = Gtk.Button(label="Calendars")
        sources_btn.add_css_class("suggested-action")
        sources_btn.connect("clicked", lambda *_: self.open_sources())
        header.pack_end(sources_btn)

        Gtk.StyleContext.add_provider_for_display(
            self.get_display(),
            self._css,
            Gtk.STYLE_PROVIDER_PRIORITY_USER,
        )
        self._apply_app_css(gtk_theme.load_profile())
        gtk_theme.watch_theme(self._apply_app_css, gtk_version=4)

        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.set_child(root)

        paned = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL)
        paned.set_wide_handle(True)
        paned.set_hexpand(True)
        paned.set_vexpand(True)
        paned.set_shrink_start_child(False)
        paned.set_shrink_end_child(False)
        paned.set_resize_start_child(False)
        paned.set_resize_end_child(True)
        paned.connect("map", lambda *_: paned.set_position(420))
        root.append(paned)

        month_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        month_wrap.set_margin_start(8)
        month_wrap.set_margin_end(12)
        month_wrap.set_margin_top(8)
        month_wrap.set_margin_bottom(8)
        month_wrap.set_hexpand(True)
        month_wrap.set_vexpand(True)

        self._weekday_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        self._weekday_box.set_homogeneous(True)
        month_wrap.append(self._weekday_box)
        self._rebuild_weekdays()

        self._grid = Gtk.Grid()
        self._grid.set_column_homogeneous(True)
        self._grid.set_row_homogeneous(True)
        self._grid.set_valign(Gtk.Align.FILL)
        self._grid.set_halign(Gtk.Align.FILL)
        self._grid.set_column_spacing(0)
        self._grid.set_row_spacing(0)
        self._grid.set_hexpand(True)
        self._grid.set_vexpand(True)
        month_wrap.append(self._grid)
        self._build_day_buttons()

        self._status = Gtk.Label(label="Add a Google or .ics calendar to see events.")
        self._status.set_halign(Gtk.Align.START)
        self._status.set_wrap(True)
        self._status.set_xalign(0)
        month_wrap.append(self._status)

        agenda_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        agenda_wrap.set_margin_start(16)
        agenda_wrap.set_margin_end(8)
        agenda_wrap.set_margin_top(16)
        agenda_wrap.set_margin_bottom(12)
        agenda_wrap.set_size_request(340, -1)
        agenda_wrap.set_vexpand(True)
        month_wrap.set_vexpand(True)
        paned.set_start_child(agenda_wrap)
        paned.set_end_child(month_wrap)

        self._agenda_heading = Gtk.Label()
        self._agenda_heading.set_halign(Gtk.Align.START)
        self._agenda_heading.set_wrap(True)
        self._agenda_heading.set_xalign(0)
        self._agenda_heading.add_css_class("title-4")
        agenda_wrap.append(self._agenda_heading)

        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_vexpand(True)
        scrolled.set_hexpand(True)
        scrolled.set_propagate_natural_height(False)
        agenda_wrap.append(scrolled)
        self._agenda_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        self._agenda_box.set_margin_end(6)
        scrolled.set_child(self._agenda_box)

        self._rebuild_month()
        self._load_cached_sync()
        self.refresh_async()
        GLib.timeout_add_seconds(30, self._tick_now)

    def _apply_app_css(self, profile) -> None:
        surface = profile.surface_hex()
        surface_alt = profile.surface_alt_hex()
        accent = profile.accent()
        fg = profile.foreground
        bg = profile.background
        muted = profile.surface_alt_hex()
        font_px = int(getattr(self.store, "event_font_px", 20) or 20)
        self._accent = accent
        css = f"""
        window.gtk-calendar,
        window.gtk-calendar > headerbar,
        window.gtk-calendar > headerbar:backdrop,
        window.gtk-calendar .titlebar,
        window.gtk-calendar box,
        window.gtk-calendar paned,
        window.gtk-calendar grid,
        window.gtk-calendar scrolledwindow,
        window.gtk-calendar viewport,
        window.gtk-calendar .event-card,
        window.gtk-calendar .event-card:hover,
        window.gtk-calendar .event-card:active {{
          background: {bg};
          background-color: {bg};
          background-image: none;
        }}
        button.cal-day {{
          padding: 0;
          margin: 0;
          border-radius: 0;
          background: {bg};
          background-color: {bg};
          background-image: none;
          border: none;
          box-shadow: none;
          min-height: 0;
          min-width: 0;
        }}
        button.cal-day:hover,
        button.cal-day:active,
        button.cal-day:focus {{
          background: {bg};
          background-color: {bg};
          border: none;
          box-shadow: none;
        }}
        button.cal-day box,
        button.cal-day scrolledwindow,
        button.cal-day viewport {{
          background: none;
          background-color: transparent;
          background-image: none;
          padding: 0;
          margin: 0;
          min-width: 0;
        }}
        button.cal-day.cal-today,
        button.cal-day.cal-today:hover,
        button.cal-day.cal-today:active,
        button.cal-day.cal-today:focus {{
          border: none;
          box-shadow: inset 0 0 0 3px {accent};
        }}
        button.cal-day.cal-picked,
        button.cal-day.cal-picked:hover,
        button.cal-day.cal-picked:active,
        button.cal-day.cal-picked:focus {{
          border: none;
          box-shadow: inset 0 0 0 2px {accent};
        }}
        button.cal-day.cal-today.cal-picked,
        button.cal-day.cal-today.cal-picked:hover,
        button.cal-day.cal-today.cal-picked:active {{
          border: none;
          box-shadow: inset 0 0 0 3px {accent};
        }}
        button.cal-day.cal-today label.cal-day-num {{
          font-weight: 800;
        }}
        button.cal-day.cal-picked label.cal-day-num {{
          font-weight: 700;
        }}
        button.cal-day.outside {{
          opacity: 0.42;
        }}
        button.cal-day.outside.cal-today,
        button.cal-day.outside.cal-picked {{
          opacity: 1;
        }}
        label.cal-dow {{
          color: {muted};
          font-weight: 600;
          font-size: 12px;
        }}
        .cal-dot {{
          min-width: 7px;
          min-height: 7px;
          border-radius: 99px;
          padding: 0;
        }}
        .event-card {{
          background: {bg};
          background-color: {bg};
          border-radius: 10px;
          padding: 8px 10px;
        }}
        .event-swatch {{
          min-width: 8px;
          min-height: 28px;
          border-radius: 4px;
        }}
        label.cal-day-num {{
          font-weight: 700;
          font-size: 13px;
          padding: 2px 6px 0 6px;
        }}
        label.cal-event-snip {{
          font-size: {font_px}px;
          margin: 0;
          padding: 0;
        }}
        label.cal-event-time {{
          font-size: {font_px}px;
          font-weight: 700;
          margin: 0;
          padding: 0;
        }}
        label.cal-event-name {{
          font-size: {font_px}px;
          font-weight: 400;
          margin: 0;
          padding: 0;
        }}
        window.gtk-calendar.cal-week-view label.cal-day-num {{
          font-size: 18px;
        }}
        window.gtk-calendar button.cal-day box.cal-span-bar {{
          padding: 1px 6px;
          margin: 0;
          min-height: {font_px + 6}px;
          background: none;
          background-color: transparent;
        }}
        window.gtk-calendar button.cal-day box.cal-span-start {{
          border: none;
          border-radius: 6px 0 0 6px;
          box-shadow: inset 2px 0 0 #ffffff, inset 0 2px 0 #ffffff, inset 0 -2px 0 #ffffff;
        }}
        window.gtk-calendar button.cal-day box.cal-span-mid {{
          border: none;
          border-radius: 0;
          box-shadow: inset 0 2px 0 #ffffff, inset 0 -2px 0 #ffffff;
        }}
        window.gtk-calendar button.cal-day box.cal-span-end {{
          border: none;
          border-radius: 0 6px 6px 0;
          box-shadow: inset -2px 0 0 #ffffff, inset 0 2px 0 #ffffff, inset 0 -2px 0 #ffffff;
        }}
        .cal-span-gap {{
          min-height: {font_px + 8}px;
        }}
        window.gtk-calendar box.cal-now {{
          margin: 4px 0;
        }}
        window.gtk-calendar box.cal-now-line {{
          min-height: 2px;
          background: {accent};
          background-color: {accent};
          background-image: none;
        }}
        window.gtk-calendar label.cal-now-label {{
          color: {accent};
          font-weight: 800;
          font-size: 12px;
        }}
        window.gtk-calendar.cal-week-view label.cal-now-label,
        window.gtk-calendar button.cal-day label.cal-now-label {{
          font-size: {max(11, min(font_px, 16))}px;
        }}
        """
        self._css.load_from_data(css.encode("utf-8"))
        if self._day_buttons:
            self._rebuild_month()

    def _rebuild_weekdays(self) -> None:
        child = self._weekday_box.get_first_child()
        while child is not None:
            nxt = child.get_next_sibling()
            self._weekday_box.remove(child)
            child = nxt
        names = WEEKDAY_SUN if self.store.week_start == "sunday" else WEEKDAY_MON
        for name in names:
            lbl = Gtk.Label(label=name)
            lbl.add_css_class("cal-dow")
            lbl.set_halign(Gtk.Align.CENTER)
            self._weekday_box.append(lbl)

    def _build_day_buttons(self) -> None:
        child = self._grid.get_first_child()
        while child is not None:
            nxt = child.get_next_sibling()
            self._grid.remove(child)
            child = nxt
        self._day_buttons = []
        rows = 1 if self.store.view_mode == "week" else 6
        for row in range(rows):
            for col in range(7):
                btn = Gtk.Button()
                btn.add_css_class("cal-day")
                btn.set_hexpand(True)
                btn.set_vexpand(True)
                btn.set_valign(Gtk.Align.FILL)
                inner = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
                inner.set_valign(Gtk.Align.FILL)
                inner.set_hexpand(True)
                inner.set_vexpand(True)
                num = Gtk.Label()
                num.set_halign(Gtk.Align.START)
                num.add_css_class("cal-day-num")
                allday_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
                allday_box.set_halign(Gtk.Align.FILL)
                allday_box.set_hexpand(True)
                events_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
                events_box.set_halign(Gtk.Align.FILL)
                events_box.set_hexpand(True)
                events_box.set_valign(Gtk.Align.START)
                scroll = Gtk.ScrolledWindow()
                scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
                scroll.set_vexpand(True)
                scroll.set_hexpand(True)
                scroll.set_propagate_natural_height(False)
                scroll.set_child(events_box)
                inner.append(num)
                inner.append(allday_box)
                inner.append(scroll)
                btn.set_child(inner)
                btn._cal_num = num  # type: ignore[attr-defined]
                btn._cal_allday = allday_box  # type: ignore[attr-defined]
                btn._cal_events = events_box  # type: ignore[attr-defined]
                btn._cal_date = None  # type: ignore[attr-defined]
                btn.connect("clicked", self._on_day_clicked)
                self._grid.attach(btn, col, row, 1, 1)
                self._day_buttons.append(btn)

    def _ensure_day_buttons(self) -> None:
        need = 7 if self.store.view_mode == "week" else 42
        if len(self._day_buttons) != need:
            self._build_day_buttons()

    def _first_weekday(self) -> int:
        return pycal.SUNDAY if self.store.week_start == "sunday" else pycal.MONDAY

    def _week_start_of(self, day: date) -> date:
        if self.store.week_start == "sunday":
            offset = (day.weekday() + 1) % 7
        else:
            offset = day.weekday()
        return day - timedelta(days=offset)

    def _week_cells(self) -> list[date]:
        start = self._week_start_of(self._week_anchor)
        return [start + timedelta(days=i) for i in range(7)]

    def _week_title(self) -> str:
        days = self._week_cells()
        start, end = days[0], days[-1]
        if start.year == end.year and start.month == end.month:
            return f"{start.strftime('%B')} {start.day}–{end.day}, {start.year}"
        if start.year == end.year:
            return f"{start.strftime('%b')} {start.day} – {end.strftime('%b')} {end.day}, {start.year}"
        return (
            f"{start.strftime('%b')} {start.day}, {start.year} – "
            f"{end.strftime('%b')} {end.day}, {end.year}"
        )

    def _visible_cells(self) -> list[date]:
        if self.store.view_mode == "week":
            return self._week_cells()
        return self._month_cells()

    def _month_cells(self) -> list[date]:
        cal = pycal.Calendar(firstweekday=self._first_weekday())
        weeks = cal.monthdatescalendar(self.view_year, self.view_month)
        cells: list[date] = []
        for week in weeks:
            cells.extend(week)
        while len(cells) < 42:
            cells.append(cells[-1] + timedelta(days=1))
        return cells[:42]

    def _rebuild_month(self) -> None:
        self._ensure_day_buttons()
        if self.store.view_mode == "week":
            self._title.set_label(self._week_title())
            self.add_css_class("cal-week-view")
            self.remove_css_class("cal-month-view")
        else:
            self._title.set_label(date(self.view_year, self.view_month, 1).strftime("%B %Y"))
            self.add_css_class("cal-month-view")
            self.remove_css_class("cal-week-view")
        self._rebuild_weekdays()
        today = date.today()
        cells = self._visible_cells()
        for week_i in range(0, len(cells), 7):
            week = cells[week_i : week_i + 7]
            span_map, singles = self._allday_span_layout(week)
            for col, day in enumerate(week):
                btn = self._day_buttons[week_i + col]
                btn._cal_date = day  # type: ignore[attr-defined]
                btn._cal_num.set_label(str(day.day))  # type: ignore[attr-defined]
                btn.remove_css_class("cal-today")
                btn.remove_css_class("cal-picked")
                btn.remove_css_class("today")
                btn.remove_css_class("clicked")
                btn.remove_css_class("selected")
                btn.remove_css_class("outside")
                if self.store.view_mode != "week" and day.month != self.view_month:
                    btn.add_css_class("outside")
                if day == today:
                    btn.add_css_class("cal-today")
                if self._clicked_day is not None and day == self._clicked_day:
                    btn.add_css_class("cal-picked")
                events_box = getattr(btn, "_cal_events", None)
                allday_box = getattr(btn, "_cal_allday", None)
                if events_box is None:
                    continue
                if allday_box is not None:
                    self._clear_box(allday_box)
                self._clear_box(events_box)
                target = allday_box if allday_box is not None else events_box
                for ev, role in span_map[day]:
                    if ev is None:
                        target.append(self._span_gap())
                    else:
                        target.append(self._day_event_line(ev, span_role=role))
                for ev in singles[day]:
                    target.append(self._day_event_line(ev))
                timed = [ev for ev in self._day_events(day) if not ev.all_day]
                self._append_timed_with_now(
                    events_box, day, timed, self._day_event_line
                )
        self._rebuild_agenda()

    def _event_color(self, ev: CalendarEvent) -> str:
        color = (ev.color or "").strip()
        if color.startswith("#") and len(color) in (4, 7):
            return color
        if color and all(ch in "0123456789abcdefABCDEF" for ch in color):
            return f"#{color}"
        return "#458588"

    def _colorize(self, widget: Gtk.Widget, color: str) -> None:
        css = Gtk.CssProvider()
        css.load_from_data(
            f"label, * {{ color: {color}; }}\n".encode()
        )
        widget.get_style_context().add_provider(
            css, Gtk.STYLE_PROVIDER_PRIORITY_USER
        )

    def _day_event_line(self, ev: CalendarEvent, span_role: str = "single") -> Gtk.Widget:
        color = "#ffffff" if ev.all_day else self._event_color(ev)
        title = html.escape(ev.title)
        if ev.all_day:
            markup = f'<span foreground="{color}" font_weight="700">{title}</span>'
        else:
            clock = html.escape(ev.start_clock())
            markup = (
                f'<span foreground="{color}">'
                f'<span font_weight="700">{clock}</span>  {title}'
                f"</span>"
            )
        line = Gtk.Label()
        line.set_use_markup(True)
        line.set_markup(markup)
        line.set_halign(Gtk.Align.START)
        line.set_hexpand(True)
        line.set_xalign(0)
        line.set_ellipsize(Pango.EllipsizeMode.END)
        line.set_wrap(False)
        line.add_css_class("cal-event-snip")
        if not ev.all_day:
            line.add_css_class("cal-event-time")
        self._colorize(line, color)
        target: Gtk.Widget = line
        if ev.all_day and span_role in ("start", "mid", "end"):
            wrap = Gtk.Box()
            wrap.set_hexpand(True)
            wrap.add_css_class("cal-span-bar")
            wrap.add_css_class(f"cal-span-{span_role}")
            wrap.append(line)
            self._style_span(wrap, span_role)
            target = wrap
        self._bind_event_click(target, ev)
        return target

    def _bind_event_click(self, widget: Gtk.Widget, ev: CalendarEvent) -> None:
        gesture = Gtk.GestureClick()
        gesture.set_button(1)
        gesture.set_propagation_phase(Gtk.PropagationPhase.CAPTURE)

        def pressed(gest: Gtk.GestureClick, *_args) -> None:
            gest.set_state(Gtk.EventSequenceState.CLAIMED)
            self._show_event(ev)

        gesture.connect("pressed", pressed)
        widget.add_controller(gesture)
        try:
            widget.set_cursor_from_name("pointer")
        except Exception:
            pass

    def _style_span(self, widget: Gtk.Widget, role: str) -> None:
        if role == "start":
            shadow = (
                "inset 2px 0 0 #ffffff, inset 0 2px 0 #ffffff, inset 0 -2px 0 #ffffff"
            )
            radius = "6px 0 0 6px"
        elif role == "end":
            shadow = (
                "inset -2px 0 0 #ffffff, inset 0 2px 0 #ffffff, inset 0 -2px 0 #ffffff"
            )
            radius = "0 6px 6px 0"
        else:
            shadow = "inset 0 2px 0 #ffffff, inset 0 -2px 0 #ffffff"
            radius = "0"
        css = Gtk.CssProvider()
        css.load_from_data(
            (
                "box {"
                f" box-shadow: {shadow};"
                f" border-radius: {radius};"
                " background: none;"
                " min-height: 1.4em;"
                " padding: 1px 6px;"
                " }"
            ).encode()
        )
        widget.get_style_context().add_provider(css, Gtk.STYLE_PROVIDER_PRIORITY_USER)

    def _span_gap(self) -> Gtk.Widget:
        gap = Gtk.Box()
        gap.add_css_class("cal-span-gap")
        gap.set_hexpand(True)
        return gap

    def _span_key(self, ev: CalendarEvent) -> tuple[str, str]:
        return (ev.source_id, ev.title.strip().casefold())

    def _allday_for_day(self, day: date) -> list[CalendarEvent]:
        return [ev for ev in self._day_events(day) if ev.all_day]

    def _allday_span_layout(
        self, week: list[date]
    ) -> tuple[
        dict[date, list[tuple[CalendarEvent | None, str]]],
        dict[date, list[CalendarEvent]],
    ]:
        per_day = {day: self._allday_for_day(day) for day in week}

        def has_key(day: date, key: tuple[str, str]) -> bool:
            return any(self._span_key(ev) == key for ev in self._allday_for_day(day))

        keys_order: list[tuple[str, str]] = []
        for day in week:
            for ev in per_day[day]:
                key = self._span_key(ev)
                if key not in keys_order:
                    keys_order.append(key)

        runs: list[tuple[tuple[str, str], int, int]] = []
        n = len(week)
        for key in keys_order:
            i = 0
            while i < n:
                if not any(self._span_key(ev) == key for ev in per_day[week[i]]):
                    i += 1
                    continue
                j = i
                while j + 1 < n and any(
                    self._span_key(ev) == key for ev in per_day[week[j + 1]]
                ):
                    j += 1
                continues_left = has_key(week[i] - timedelta(days=1), key)
                continues_right = has_key(week[j] + timedelta(days=1), key)
                if j > i or continues_left or continues_right:
                    runs.append((key, i, j))
                i = j + 1

        lane_ends: list[int] = []
        run_lanes: list[int] = []
        for _key, i0, i1 in runs:
            lane = next((idx for idx, end in enumerate(lane_ends) if end < i0), None)
            if lane is None:
                lane = len(lane_ends)
                lane_ends.append(i1)
            else:
                lane_ends[lane] = i1
            run_lanes.append(lane)

        nlanes = len(lane_ends)
        span_map: dict[date, list[tuple[CalendarEvent | None, str]]] = {
            day: [(None, "gap")] * nlanes for day in week
        }
        spanning_keys: dict[date, set[tuple[str, str]]] = {day: set() for day in week}
        for (key, i0, i1), lane in zip(runs, run_lanes):
            continues_left = has_key(week[i0] - timedelta(days=1), key)
            continues_right = has_key(week[i1] + timedelta(days=1), key)
            for i in range(i0, i1 + 1):
                inst = next(ev for ev in per_day[week[i]] if self._span_key(ev) == key)
                if i == i0 and not continues_left:
                    role = "start"
                elif i == i1 and not continues_right:
                    role = "end"
                else:
                    role = "mid"
                span_map[week[i]][lane] = (inst, role)
                spanning_keys[week[i]].add(key)

        for day in week:
            slots = span_map[day]
            while slots and slots[-1][0] is None:
                slots.pop()

        singles = {
            day: [ev for ev in per_day[day] if self._span_key(ev) not in spanning_keys[day]]
            for day in week
        }
        return span_map, singles

    def _now(self) -> datetime:
        return datetime.now(tz=LOCAL_TZ)

    def _now_marker(self) -> Gtk.Widget:
        now = self._now()
        wrap = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        wrap.add_css_class("cal-now")
        wrap.set_hexpand(True)
        label = Gtk.Label(label=_clock(now))
        label.add_css_class("cal-now-label")
        label.set_valign(Gtk.Align.CENTER)
        accent = getattr(self, "_accent", "#fe8019")
        self._colorize(label, accent)
        line = Gtk.Box()
        line.add_css_class("cal-now-line")
        line.set_hexpand(True)
        line.set_valign(Gtk.Align.CENTER)
        wrap.append(label)
        wrap.append(line)
        return wrap

    def _append_timed_with_now(self, box: Gtk.Box, day: date, timed: list[CalendarEvent], factory) -> None:
        show_now = day == date.today()
        now = self._now() if show_now else None
        inserted = False
        if show_now and now is not None and not any(as_local(ev.start) < now for ev in timed):
            box.append(self._now_marker())
            inserted = True
        for ev in timed:
            if show_now and now is not None and not inserted and as_local(ev.start) >= now:
                box.append(self._now_marker())
                inserted = True
            box.append(factory(ev))
        if show_now and not inserted:
            box.append(self._now_marker())

    def _tick_now(self) -> bool:
        try:
            if not self.get_mapped():
                return True
            today = date.today()
            showing_today = any(
                getattr(btn, "_cal_date", None) == today for btn in self._day_buttons
            )
            if showing_today or self._agenda_day() == today:
                self._rebuild_month()
        except Exception:
            return True
        return True

    def _day_events(self, day: date) -> list[CalendarEvent]:
        return sorted(
            self._events_by_day.get(day, []),
            key=lambda ev: (not ev.all_day, as_local(ev.start), ev.title.lower()),
        )

    def _agenda_day(self) -> date:
        return self._clicked_day or date.today()

    def _on_day_clicked(self, button: Gtk.Button) -> None:
        day = getattr(button, "_cal_date", None)
        if not isinstance(day, date):
            return
        self._clicked_day = day
        self.selected = day
        self._week_anchor = day
        if self.store.view_mode == "month" and day.month != self.view_month:
            self.view_year, self.view_month = day.year, day.month
        self._rebuild_month()

    def _sync_nav_tooltips(self) -> None:
        if self.store.view_mode == "week":
            self._prev_btn.set_tooltip_text("Previous week")
            self._next_btn.set_tooltip_text("Next week")
        else:
            self._prev_btn.set_tooltip_text("Previous month")
            self._next_btn.set_tooltip_text("Next month")

    def _on_view_toggled(self, button: Gtk.ToggleButton) -> None:
        if self._view_guard or not button.get_active():
            return
        self._set_view_mode("week" if button is self._week_btn else "month")

    def _set_view_mode(self, mode: str) -> None:
        mode = "week" if mode == "week" else "month"
        prev = self.store.view_mode
        if mode == "week":
            today = date.today()
            if self._clicked_day is not None:
                self._week_anchor = self._clicked_day
            elif today.year == self.view_year and today.month == self.view_month:
                self._week_anchor = today
            else:
                self._week_anchor = date(self.view_year, self.view_month, 1)
        elif prev == "week":
            focus = self._clicked_day or self._week_anchor
            self.view_year, self.view_month = focus.year, focus.month
        self.store.view_mode = mode
        self.store.save()
        self._view_guard = True
        try:
            self._month_btn.set_active(mode == "month")
            self._week_btn.set_active(mode == "week")
        finally:
            self._view_guard = False
        self._sync_nav_tooltips()
        self._rebuild_month()

    def _shift_view(self, delta: int) -> None:
        if self.store.view_mode == "week":
            start = self._week_start_of(self._week_anchor)
            self._week_anchor = start + timedelta(days=7 * delta)
            self.view_year, self.view_month = self._week_anchor.year, self._week_anchor.month
        else:
            month = self.view_month + delta
            year = self.view_year
            while month < 1:
                month += 12
                year -= 1
            while month > 12:
                month -= 12
                year += 1
            self.view_year, self.view_month = year, month
        self._rebuild_month()
        self.refresh_async()

    def _shift_month(self, delta: int) -> None:
        self._shift_view(delta)

    def go_today(self) -> None:
        today = date.today()
        self.view_year, self.view_month = today.year, today.month
        self._week_anchor = today
        self._clicked_day = None
        self.selected = today
        self._rebuild_month()
        self.refresh_async()

    def _window_range(self) -> tuple[datetime, datetime]:
        def add_months(year: int, month: int, delta: int) -> date:
            total = year * 12 + (month - 1) + delta
            return date(total // 12, total % 12 + 1, 1)

        prev = add_months(self.view_year, self.view_month, -1)
        nxt = add_months(self.view_year, self.view_month, 2)
        start = datetime(prev.year, prev.month, 1, tzinfo=LOCAL_TZ)
        end = datetime(nxt.year, nxt.month, 1, tzinfo=LOCAL_TZ)
        return start, end

    def refresh_async(self) -> None:
        start, end = self._window_range()
        gen = getattr(self, "_refresh_gen", 0) + 1
        self._refresh_gen = gen
        self._busy = True
        self._status.set_label("Loading calendars…")
        store_snapshot = self.store

        def worker():
            events: list[CalendarEvent] = []
            errors: list[str] = []
            try:
                for src in list(store_snapshot.sources):
                    if not src.enabled:
                        continue
                    try:
                        if src.kind == "ics":
                            loaded, hint = load_ics_source(src, start, end, network=True)
                            placeholders = {"calendar", "ics", "ics calendar", ""}
                            if (
                                hint
                                and src.name.strip().lower() in placeholders
                                and hint.strip().lower() not in placeholders
                            ):
                                src.name = hint
                                store_snapshot.save()
                            events.extend(loaded)
                        elif src.kind == "google":
                            events.extend(
                                google_api.fetch_google_events(store_snapshot, src, start, end)
                            )
                    except Exception as exc:
                        src.error = str(exc)
                        errors.append(f"{src.name}: {exc}")
                events.sort(key=lambda ev: (ev.start, ev.title.lower()))
            except Exception as exc:
                errors.append(str(exc))
            GLib.idle_add(self._apply_events, gen, events, errors)
            return False

        threading.Thread(target=worker, daemon=True).start()

    def _load_cached_sync(self) -> None:
        start, end = self._window_range()
        events: list[CalendarEvent] = []
        errors: list[str] = []
        for src in list(self.store.sources):
            if not src.enabled or src.kind != "ics":
                continue
            try:
                loaded, _hint = load_ics_source(src, start, end, network=False)
                events.extend(loaded)
            except Exception as exc:
                errors.append(str(exc))
        events.sort(key=lambda ev: (ev.start, ev.title.lower()))
        if events or errors:
            self._apply_events(0, events, errors)

    def _note(self, text: str) -> None:
        try:
            (cache_dir() / "last-refresh.txt").write_text(text, encoding="utf-8")
        except OSError:
            pass

    def _apply_events(self, gen: int, events: list[CalendarEvent], errors: list[str]) -> bool:
        if gen and gen < getattr(self, "_refresh_gen", 0):
            return False
        self._busy = False
        try:
            self.events = events
            self._index_events()
            hidden_n = len(self.store.hidden_events)
            if errors and not events:
                self._status.set_label(" · ".join(errors[:2]))
            elif not self.store.sources:
                self._status.set_label("Add a Google or .ics calendar to see events.")
            else:
                n = len([ev for ev in events if not self._event_hidden(ev)])
                extra = f" ({errors[0]})" if errors else ""
                hidden_txt = f"  ·  {hidden_n} hidden" if hidden_n else ""
                self._status.set_label(
                    f"{n} event{'s' if n != 1 else ''} loaded.{hidden_txt}{extra}"
                )
            self._note(f"events={len(events)} days={len(self._events_by_day)} errors={errors!r}")
            self._rebuild_month()
        except Exception as exc:
            self._status.set_label(f"Could not display events: {exc}")
            self._note(f"display error: {exc!r}")
        return False

    def _clear_box(self, box: Gtk.Box) -> None:
        child = box.get_first_child()
        while child is not None:
            nxt = child.get_next_sibling()
            box.remove(child)
            child = nxt

    def _rebuild_agenda(self) -> None:
        day = self._agenda_day()
        rows = self._day_events(day)
        today = date.today()
        if self._clicked_day is None:
            heading = f"Today  ·  {day.strftime('%A')} {day.strftime('%b')} {day.day}"
        elif day == today:
            heading = f"{day.strftime('%A')} {day.strftime('%b')} {day.day}  ·  today"
        else:
            heading = f"{day.strftime('%A')} {day.strftime('%b')} {day.day}"
        extra = f"  ·  {len(rows)} event{'s' if len(rows) != 1 else ''}"
        self._agenda_heading.set_label(heading + extra)
        self._clear_box(self._agenda_box)
        if not rows:
            if day == today:
                self._agenda_box.append(self._now_marker())
            else:
                empty = Gtk.Label(label="No events")
                empty.set_wrap(True)
                empty.set_xalign(0)
                empty.add_css_class("dim-label")
                self._agenda_box.append(empty)
            return
        for ev in rows:
            if ev.all_day:
                self._agenda_box.append(self._event_card(ev))
        timed = [ev for ev in rows if not ev.all_day]
        self._append_timed_with_now(self._agenda_box, day, timed, self._event_card)

    def _event_card(self, ev: CalendarEvent) -> Gtk.Widget:
        btn = Gtk.Button()
        btn.add_css_class("event-card")
        btn.set_has_frame(False)
        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        swatch = Gtk.Box()
        swatch.add_css_class("event-swatch")
        css = Gtk.CssProvider()
        color = self._event_color(ev)
        css.load_from_data(f".event-swatch {{ background: {color}; }}".encode())
        swatch.get_style_context().add_provider(css, Gtk.STYLE_PROVIDER_PRIORITY_USER)
        box.append(swatch)
        texts = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        texts.set_hexpand(True)
        title = Gtk.Label(label=ev.title)
        title.set_halign(Gtk.Align.START)
        title.set_xalign(0)
        title.set_wrap(True)
        title.set_ellipsize(Pango.EllipsizeMode.END)
        title.set_max_width_chars(36)
        self._colorize(title, "#ffffff" if ev.all_day else color)
        meta_text = ev.source_name if ev.all_day else f"{ev.time_label()}  ·  {ev.source_name}"
        meta = Gtk.Label(label=meta_text)
        meta.set_halign(Gtk.Align.START)
        meta.set_xalign(0)
        meta.add_css_class("dim-label")
        meta.set_ellipsize(Pango.EllipsizeMode.END)
        texts.append(title)
        texts.append(meta)
        box.append(texts)
        btn.set_child(box)
        btn.connect("clicked", lambda *_a, event=ev: self._show_event(event))
        return btn

    def _show_event(self, ev: CalendarEvent) -> None:
        win = Gtk.Window(title=ev.title or "Event")
        win.add_css_class("gtk-calendar")
        win.set_transient_for(self)
        win.set_modal(True)
        win.set_default_size(440, 320)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        box.set_margin_start(16)
        box.set_margin_end(16)
        box.set_margin_top(16)
        box.set_margin_bottom(16)
        win.set_child(box)

        heading = Gtk.Label(label=ev.title)
        heading.set_wrap(True)
        heading.set_xalign(0)
        heading.add_css_class("title-4")
        box.append(heading)

        meta_bits = [ev.time_label(), ev.source_name]
        if ev.location:
            meta_bits.append(ev.location)
        meta = Gtk.Label(label="\n".join(meta_bits))
        meta.set_wrap(True)
        meta.set_xalign(0)
        meta.add_css_class("dim-label")
        box.append(meta)

        if ev.description:
            scrolled = Gtk.ScrolledWindow()
            scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
            scrolled.set_vexpand(True)
            desc = Gtk.Label(label=ev.description)
            desc.set_wrap(True)
            desc.set_xalign(0)
            desc.set_yalign(0)
            desc.set_selectable(True)
            scrolled.set_child(desc)
            box.append(scrolled)

        actions = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        actions.set_halign(Gtk.Align.END)
        hide_btn = Gtk.Button(label="Hide")
        hide_btn.add_css_class("destructive-action")
        hide_btn.set_tooltip_text("Hide this event from the calendar")
        hide_btn.connect("clicked", lambda *_a, event=ev, dialog=win: self._hide_event(event, dialog))
        close_btn = Gtk.Button(label="Close")
        close_btn.connect("clicked", lambda *_: win.destroy())
        actions.append(hide_btn)
        actions.append(close_btn)
        box.append(actions)
        win.present()

    def _event_hidden(self, ev: CalendarEvent) -> bool:
        return self.store.is_hidden(
            ev.hide_key(),
            ev.uid,
            ev.source_id,
            ev.title,
            as_local(ev.start).isoformat(),
        )

    def _refresh_status_count(self) -> None:
        hidden_n = len(self.store.hidden_events)
        n = len([item for item in self.events if not self._event_hidden(item)])
        hidden_txt = f"  ·  {hidden_n} hidden" if hidden_n else ""
        self._status.set_label(f"{n} event{'s' if n != 1 else ''} loaded.{hidden_txt}")

    def _hide_event(self, ev: CalendarEvent, dialog: Gtk.Window | None = None) -> None:
        self.store.hide_event(
            ev.hide_key(),
            ev.uid,
            ev.title,
            ev.source_id,
            ev.time_label(),
            as_local(ev.start).isoformat(),
        )
        if dialog is not None:
            dialog.destroy()
        self._index_events()
        self._rebuild_month()
        self._refresh_status_count()

    def _index_events(self) -> None:
        by_day: dict[date, list[CalendarEvent]] = {}
        for ev in self.events:
            if self._event_hidden(ev):
                continue
            for day in ev.iter_days():
                by_day.setdefault(day, []).append(ev)
        self._events_by_day = by_day

    def _unhide_event(self, key: str) -> None:
        self.store.unhide_event(key)
        self._index_events()
        self._rebuild_month()
        self._refresh_status_count()

    def _on_event_activated(self, _list, row) -> None:
        ev = getattr(row, "_event", None)
        if isinstance(ev, CalendarEvent):
            self._show_event(ev)

    def add_ics_path(self, path: str, url: str = "") -> None:
        ident = self.store.new_ics_id()
        src_path = Path(path).expanduser()
        stored = self.store.copy_ics_file(src_path, ident) if src_path.is_file() else src_path
        source = CalendarSource(
            id=ident,
            kind="ics",
            name=src_path.stem.replace("_", " ").strip() or "ICS calendar",
            color=self.store.next_color(),
            enabled=True,
            url=normalize_ics_url(url),
            path=str(stored),
        )
        self.store.upsert_source(source)
        self.refresh_async()

    def add_ics_url(self, url: str) -> None:
        target = normalize_ics_url(url)
        if not target:
            return
        source = CalendarSource(
            id=self.store.new_ics_id(),
            kind="ics",
            name="ICS calendar",
            color=self.store.next_color(),
            enabled=True,
            url=target,
        )
        self.store.upsert_source(source)
        self.refresh_async()

    def open_sources(self) -> None:
        SourcesWindow(self).present()


class SourcesWindow(Gtk.Window):
    def __init__(self, parent: CalendarWindow):
        super().__init__(title="Calendars")
        self.add_css_class("gtk-calendar")
        self.set_transient_for(parent)
        self.set_modal(True)
        self.set_default_size(560, 640)
        self.parent_win = parent
        self.store = parent.store

        header = Gtk.HeaderBar()
        header.set_show_title_buttons(True)
        self.set_titlebar(header)

        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self.set_child(scrolled)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=14)
        box.set_margin_start(16)
        box.set_margin_end(16)
        box.set_margin_top(16)
        box.set_margin_bottom(16)
        scrolled.set_child(box)

        week_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        week_row.append(Gtk.Label(label="Week starts"))
        self.week_drop = Gtk.DropDown.new_from_strings(["Sunday", "Monday"])
        self.week_drop.set_selected(0 if self.store.week_start == "sunday" else 1)
        self.week_drop.connect("notify::selected", self._on_week_changed)
        week_row.append(self.week_drop)
        box.append(week_row)

        size_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        size_row.append(Gtk.Label(label="Event text"))
        self.size_label = Gtk.Label(label=f"{self.store.event_font_px} px")
        self.size_label.set_width_chars(5)
        adj = Gtk.Adjustment(
            value=self.store.event_font_px,
            lower=10,
            upper=36,
            step_increment=1,
            page_increment=2,
        )
        self.size_scale = Gtk.Scale(orientation=Gtk.Orientation.HORIZONTAL, adjustment=adj)
        self.size_scale.set_digits(0)
        self.size_scale.set_draw_value(False)
        self.size_scale.set_hexpand(True)
        self.size_scale.connect("value-changed", self._on_font_changed)
        size_row.append(self.size_scale)
        size_row.append(self.size_label)
        box.append(size_row)

        google = Gtk.Frame(label="Google Calendar")
        box.append(google)
        gbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        gbox.set_margin_start(12)
        gbox.set_margin_end(12)
        gbox.set_margin_top(12)
        gbox.set_margin_bottom(12)
        google.set_child(gbox)

        help_lbl = Gtk.Label(
            label=(
                "Create a Google Cloud Desktop OAuth client, enable the Calendar API, "
                "then paste the client ID and secret. Sign-in opens the browser."
            )
        )
        help_lbl.set_wrap(True)
        help_lbl.set_xalign(0)
        help_lbl.add_css_class("dim-label")
        gbox.append(help_lbl)

        self.client_id = Gtk.Entry()
        self.client_id.set_placeholder_text("OAuth client ID")
        self.client_id.set_text(self.store.google_client_id)
        self.client_id.connect("changed", self._on_client_changed)
        gbox.append(self.client_id)
        self.client_secret = Gtk.Entry()
        self.client_secret.set_placeholder_text("OAuth client secret")
        self.client_secret.set_visibility(False)
        self.client_secret.set_text(self.store.google_client_secret)
        self.client_secret.connect("changed", self._on_client_changed)
        gbox.append(self.client_secret)

        brow = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        json_btn = Gtk.Button(label="Load client JSON…")
        json_btn.connect("clicked", self._pick_client_json)
        brow.append(json_btn)
        self.google_btn = Gtk.Button(label="Sign in with Google")
        self.google_btn.add_css_class("suggested-action")
        self.google_btn.connect("clicked", self._google_clicked)
        brow.append(self.google_btn)
        gbox.append(brow)
        self.google_status = Gtk.Label()
        self.google_status.set_xalign(0)
        self.google_status.add_css_class("dim-label")
        gbox.append(self.google_status)
        self._sync_google_status()

        ics = Gtk.Frame(label=".ics calendars")
        box.append(ics)
        ibox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        ibox.set_margin_start(12)
        ibox.set_margin_end(12)
        ibox.set_margin_top(12)
        ibox.set_margin_bottom(12)
        ics.set_child(ibox)

        add_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        file_btn = Gtk.Button(label="Add .ics file…")
        file_btn.connect("clicked", self._pick_ics_file)
        add_row.append(file_btn)
        url_btn = Gtk.Button(label="Add URL…")
        url_btn.connect("clicked", self._prompt_ics_url)
        add_row.append(url_btn)
        ibox.append(add_row)

        self.source_list = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        ibox.append(self.source_list)
        self._rebuild_source_rows()

        hidden = Gtk.Frame(label="Hidden events")
        box.append(hidden)
        hbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        hbox.set_margin_start(12)
        hbox.set_margin_end(12)
        hbox.set_margin_top(12)
        hbox.set_margin_bottom(12)
        hidden.set_child(hbox)
        hint = Gtk.Label(label="Click an event on the calendar, then Hide. Unhide restores it.")
        hint.set_wrap(True)
        hint.set_xalign(0)
        hint.add_css_class("dim-label")
        hbox.append(hint)
        self.hidden_list = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        hbox.append(self.hidden_list)
        self._rebuild_hidden_rows()

    def _sync_google_status(self) -> None:
        if not google_api.google_libs_available():
            self.google_status.set_label("Install python3-googleapi and python3-google-auth-oauthlib to enable Google sign-in.")
            self.google_btn.set_sensitive(False)
            return
        if google_api.signed_in():
            account = self.store.google_account or "Google account"
            self.google_status.set_label(f"Signed in as {account}.")
            self.google_btn.set_label("Sign out")
            self.google_btn.remove_css_class("suggested-action")
        else:
            self.google_status.set_label("Not signed in.")
            self.google_btn.set_label("Sign in with Google")
            self.google_btn.add_css_class("suggested-action")

    def _on_week_changed(self, *_args) -> None:
        self.store.week_start = "sunday" if self.week_drop.get_selected() == 0 else "monday"
        self.store.save()
        self.parent_win._rebuild_month()

    def _on_font_changed(self, scale: Gtk.Scale) -> None:
        size = max(10, min(36, int(round(scale.get_value()))))
        if size == self.store.event_font_px:
            return
        self.store.event_font_px = size
        self.size_label.set_label(f"{size} px")
        self.store.save()
        self.parent_win._apply_app_css(gtk_theme.load_profile())

    def _on_client_changed(self, *_args) -> None:
        self.store.google_client_id = self.client_id.get_text().strip()
        self.store.google_client_secret = self.client_secret.get_text().strip()
        self.store.save()

    def _pick_client_json(self, *_args) -> None:
        dialog = Gtk.FileDialog()
        dialog.set_title("Google OAuth client JSON")
        filt = Gtk.FileFilter()
        filt.set_name("JSON")
        filt.add_mime_type("application/json")
        filt.add_pattern("*.json")
        filters = Gio.ListStore.new(Gtk.FileFilter)
        filters.append(filt)
        dialog.set_filters(filters)

        def done(dlg, result):
            try:
                gfile = dlg.open_finish(result)
            except GLib.Error:
                return
            if gfile is None:
                return
            path = gfile.get_path()
            if not path:
                return
            try:
                google_api.load_client_secrets_file(Path(path), self.store)
            except Exception as exc:
                _alert(self, "Could not load client JSON", str(exc))
                return
            self.client_id.set_text(self.store.google_client_id)
            self.client_secret.set_text(self.store.google_client_secret)

        dialog.open(self, None, done)

    def _google_clicked(self, *_args) -> None:
        if google_api.signed_in():
            google_api.clear_credentials()
            self.store.google_account = ""
            self.store.sources = [src for src in self.store.sources if src.kind != "google"]
            self.store.save()
            self._sync_google_status()
            self._rebuild_source_rows()
            self.parent_win.refresh_async()
            return
        self.google_btn.set_sensitive(False)
        self.google_status.set_label("Waiting for Google sign-in in the browser…")

        def worker():
            try:
                google_api.ensure_credentials(self.store)
                calendars = google_api.list_google_calendars(self.store)
                email = ""
                try:
                    email = google_api.account_email(self.store)
                except Exception:
                    email = ""
                GLib.idle_add(self._google_signed_in, calendars, email, "")
            except Exception as exc:
                GLib.idle_add(self._google_signed_in, [], "", str(exc))

        threading.Thread(target=worker, daemon=True).start()

    def _google_signed_in(self, calendars: list[CalendarSource], email: str, error: str) -> bool:
        self.google_btn.set_sensitive(True)
        if error:
            _alert(self, "Google sign-in failed", error)
            self._sync_google_status()
            return False
        self.store.google_account = email or "Google"
        self.store.replace_google_calendars(calendars)
        self._sync_google_status()
        self._rebuild_source_rows()
        self.parent_win.refresh_async()
        return False

    def _pick_ics_file(self, *_args) -> None:
        dialog = Gtk.FileDialog()
        dialog.set_title("Add .ics calendar")
        filt = Gtk.FileFilter()
        filt.set_name("iCalendar")
        filt.add_mime_type("text/calendar")
        filt.add_pattern("*.ics")
        filters = Gio.ListStore.new(Gtk.FileFilter)
        filters.append(filt)
        dialog.set_filters(filters)

        def done(dlg, result):
            try:
                gfile = dlg.open_finish(result)
            except GLib.Error:
                return
            if gfile is None:
                return
            path = gfile.get_path()
            if path:
                self.parent_win.add_ics_path(path)
                self._rebuild_source_rows()

        dialog.open(self, None, done)

    def _prompt_ics_url(self, *_args) -> None:
        win = Gtk.Window(title="Add calendar URL")
        win.set_transient_for(self)
        win.set_modal(True)
        win.set_default_size(460, 140)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.set_margin_start(16)
        box.set_margin_end(16)
        box.set_margin_top(16)
        box.set_margin_bottom(16)
        win.set_child(box)
        box.append(Gtk.Label(label="https:// or webcal:// address of an .ics feed", xalign=0))
        entry = Gtk.Entry()
        entry.set_placeholder_text("https://example.com/calendar.ics")
        box.append(entry)
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        row.set_halign(Gtk.Align.END)
        cancel = Gtk.Button(label="Cancel")
        cancel.connect("clicked", lambda *_: win.destroy())
        add = Gtk.Button(label="Add")
        add.add_css_class("suggested-action")

        def commit(*_a):
            url = entry.get_text().strip()
            win.destroy()
            if url:
                self.parent_win.add_ics_url(url)
                self._rebuild_source_rows()

        add.connect("clicked", commit)
        entry.connect("activate", commit)
        row.append(cancel)
        row.append(add)
        box.append(row)
        win.present()
        entry.grab_focus()

    def _rebuild_source_rows(self) -> None:
        child = self.source_list.get_first_child()
        while child is not None:
            nxt = child.get_next_sibling()
            self.source_list.remove(child)
            child = nxt
        if not self.store.sources:
            empty = Gtk.Label(label="No calendars yet.")
            empty.add_css_class("dim-label")
            empty.set_xalign(0)
            self.source_list.append(empty)
            return
        for src in self.store.sources:
            self.source_list.append(self._source_row(src))

    def _rebuild_hidden_rows(self) -> None:
        self.parent_win._clear_box(self.hidden_list)
        hidden = list(self.store.hidden_events)
        if not hidden:
            empty = Gtk.Label(label="No hidden events")
            empty.add_css_class("dim-label")
            empty.set_xalign(0)
            self.hidden_list.append(empty)
            return
        for item in hidden:
            row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            label = item.get("title") or "Event"
            when = item.get("when") or ""
            if when:
                label = f"{label}  ·  {when}"
            name = Gtk.Label(label=label)
            name.set_hexpand(True)
            name.set_xalign(0)
            name.set_ellipsize(Pango.EllipsizeMode.END)
            row.append(name)
            show = Gtk.Button(label="Unhide")
            ident = item.get("key") or item.get("uid") or ""
            show.connect(
                "clicked",
                lambda *_a, hid=ident: self._unhide(hid),
            )
            row.append(show)
            self.hidden_list.append(row)

    def _unhide(self, uid: str) -> None:
        self.parent_win._unhide_event(uid)
        self._rebuild_hidden_rows()

    def _source_row(self, src: CalendarSource) -> Gtk.Widget:
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        enabled = Gtk.CheckButton()
        enabled.set_active(src.enabled)
        enabled.set_tooltip_text("Show this calendar")
        enabled.connect("toggled", lambda btn, ident=src.id: self._toggle(ident, btn.get_active()))
        row.append(enabled)
        rgba = Gdk.RGBA()
        if not rgba.parse(src.color or "#458588"):
            rgba.parse("#458588")
        color_btn = Gtk.ColorDialogButton()
        dialog = Gtk.ColorDialog()
        dialog.set_with_alpha(False)
        color_btn.set_dialog(dialog)
        color_btn.set_rgba(rgba)
        color_btn.set_tooltip_text("Calendar color")
        color_btn.set_valign(Gtk.Align.CENTER)
        color_btn.connect(
            "notify::rgba",
            lambda button, _p, ident=src.id: self._set_source_color(ident, button.get_rgba()),
        )
        row.append(color_btn)
        name = Gtk.Label(label=src.name)
        name.set_hexpand(True)
        name.set_xalign(0)
        name.set_ellipsize(Pango.EllipsizeMode.END)
        row.append(name)
        kind = Gtk.Label(label="Google" if src.kind == "google" else "ICS")
        kind.add_css_class("dim-label")
        row.append(kind)
        if src.kind == "ics":
            remove = Gtk.Button.new_from_icon_name("user-trash-symbolic")
            remove.set_tooltip_text("Remove")
            remove.connect("clicked", lambda *_a, ident=src.id: self._remove(ident))
            row.append(remove)
        return row

    def _rgba_hex(self, rgba: Gdk.RGBA) -> str:
        r = max(0, min(255, int(round(rgba.red * 255))))
        g = max(0, min(255, int(round(rgba.green * 255))))
        b = max(0, min(255, int(round(rgba.blue * 255))))
        return f"#{r:02x}{g:02x}{b:02x}"

    def _set_source_color(self, ident: str, rgba: Gdk.RGBA) -> None:
        src = self.store.source_by_id(ident)
        if src is None:
            return
        color = self._rgba_hex(rgba)
        if src.color.lower() == color.lower():
            return
        src.color = color
        self.store.save()
        for ev in self.parent_win.events:
            if ev.source_id == ident:
                ev.color = color
        self.parent_win._rebuild_month()

    def _toggle(self, ident: str, enabled: bool) -> None:
        src = self.store.source_by_id(ident)
        if src is None:
            return
        src.enabled = enabled
        self.store.save()
        self.parent_win.refresh_async()

    def _remove(self, ident: str) -> None:
        self.store.remove_source(ident)
        self._rebuild_source_rows()
        self.parent_win.refresh_async()
