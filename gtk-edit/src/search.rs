//! Incremental search overlay and go-to-line bar.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use sourceview5::prelude::*;

pub struct SearchBar {
    pub revealer: gtk::Revealer,
    pub entry: gtk::Entry,
    pub match_case: gtk::CheckButton,
    pub whole_word: gtk::CheckButton,
    pub regex: gtk::CheckButton,
    pub wrap_around: gtk::CheckButton,
    pub prev_btn: gtk::Button,
    pub next_btn: gtk::Button,
    pub opts_btn: gtk::MenuButton,
    pub nav_box: gtk::Box,
    mode: RefCell<SearchMode>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    Find,
    GoToLine,
}

impl SearchBar {
    pub fn new() -> Rc<Self> {
        let entry = gtk::Entry::builder()
            .placeholder_text("Search…")
            .hexpand(true)
            .primary_icon_name("edit-find-symbolic")
            .build();

        let match_case = gtk::CheckButton::with_label("Match case");
        let whole_word = gtk::CheckButton::with_label("Whole word");
        let regex = gtk::CheckButton::with_label("Regular expression");
        let wrap_around = gtk::CheckButton::with_label("Wrap around");
        wrap_around.set_active(true);

        let opts = gtk::Box::new(gtk::Orientation::Vertical, 4);
        opts.append(&match_case);
        opts.append(&whole_word);
        opts.append(&regex);
        opts.append(&wrap_around);
        let popover = gtk::Popover::builder().child(&opts).build();
        let opts_btn = gtk::MenuButton::builder()
            .icon_name("emblem-system-symbolic")
            .tooltip_text("Search options")
            .popover(&popover)
            .build();

        let prev = gtk::Button::from_icon_name("go-up-symbolic");
        prev.set_tooltip_text(Some("Find previous"));
        let next = gtk::Button::from_icon_name("go-down-symbolic");
        next.set_tooltip_text(Some("Find next"));
        let nav = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        nav.add_css_class("linked");
        nav.append(&prev);
        nav.append(&next);

        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");

        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_top(4);
        bar.set_margin_bottom(4);
        bar.set_margin_start(6);
        bar.set_margin_end(6);
        bar.append(&entry);
        bar.append(&nav);
        bar.append(&opts_btn);
        bar.append(&close);

        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .reveal_child(false)
            .child(&bar)
            .build();

        let this = Rc::new(Self {
            revealer: revealer.clone(),
            entry: entry.clone(),
            match_case,
            whole_word,
            regex,
            wrap_around,
            prev_btn: prev.clone(),
            next_btn: next.clone(),
            opts_btn,
            nav_box: nav,
            mode: RefCell::new(SearchMode::Find),
        });

        {
            let this2 = Rc::clone(&this);
            close.connect_clicked(move |_| {
                this2.hide();
            });
        }
        this
    }

    /// Reveal the find bar. When `initial` is set (e.g. current selection),
    /// that text is placed in the search entry.
    pub fn show_find_with(&self, initial: Option<&str>) {
        *self.mode.borrow_mut() = SearchMode::Find;
        self.entry.set_placeholder_text(Some("Search…"));
        self.entry.set_primary_icon_name(Some("edit-find-symbolic"));
        self.nav_box.set_visible(true);
        self.opts_btn.set_visible(true);
        if let Some(text) = initial.filter(|t| !t.is_empty()) {
            self.entry.set_text(text);
        }
        self.revealer.set_reveal_child(true);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    pub fn show_goto_line(&self) {
        *self.mode.borrow_mut() = SearchMode::GoToLine;
        self.entry.set_placeholder_text(Some("Go to line (or line:column)…"));
        self.entry.set_primary_icon_name(Some("go-jump-symbolic"));
        // Clear leftover search text so Enter isn't a silent no-op.
        self.entry.set_text("");
        self.nav_box.set_visible(false);
        self.opts_btn.set_visible(false);
        self.revealer.set_reveal_child(true);
        self.entry.grab_focus();
    }

    pub fn hide(&self) {
        self.revealer.set_reveal_child(false);
        self.nav_box.set_visible(true);
        self.opts_btn.set_visible(true);
    }

    pub fn is_goto_mode(&self) -> bool {
        *self.mode.borrow() == SearchMode::GoToLine
    }

    pub fn find(
        &self,
        buffer: &sourceview5::Buffer,
        view: &sourceview5::View,
        forward: bool,
        highlight: bool,
    ) -> bool {
        if self.is_goto_mode() {
            return self.goto_line(buffer, view);
        }
        let needle = self.entry.text().to_string();
        if needle.is_empty() {
            clear_search_highlights(buffer);
            return false;
        }

        let settings = sourceview5::SearchSettings::new();
        settings.set_search_text(Some(&needle));
        settings.set_case_sensitive(self.match_case.is_active());
        settings.set_at_word_boundaries(self.whole_word.is_active());
        settings.set_regex_enabled(self.regex.is_active());
        settings.set_wrap_around(self.wrap_around.is_active());

        let context = sourceview5::SearchContext::new(buffer, Some(&settings));
        context.set_highlight(highlight);

        // Start after (forward) / before (backward) the current selection so
        // repeated Find advances instead of re-hitting the same match when the
        // insert mark sits at the selection start (right-to-left select).
        let iter = search_iter(buffer, forward);
        let found = if forward {
            context.forward(&iter)
        } else {
            context.backward(&iter)
        };

        if let Some((start, end, _wrapped)) = found {
            buffer.select_range(&start, &end);
            view.scroll_to_iter(&mut start.clone(), 0.2, false, 0.0, 0.0);
            true
        } else {
            false
        }
    }

    /// Jump to line (1-based), optionally `line:column`. Supports `+N` / `-N`
    /// relative offsets like classic gedit.
    pub fn goto_line(&self, buffer: &sourceview5::Buffer, view: &sourceview5::View) -> bool {
        let text = self.entry.text().to_string();
        let text = text.trim();
        if text.is_empty() {
            return false;
        }

        let (line_part, col_part) = match text.split_once(':') {
            Some((l, c)) => (l.trim(), Some(c.trim())),
            None => (text, None),
        };

        let line_count = buffer.line_count();
        let insert = buffer.get_insert();
        let cur_line = buffer.iter_at_mark(&insert).line();

        let target_line = if let Some(rest) = line_part.strip_prefix('+') {
            let off = rest.parse::<i32>().unwrap_or(0).max(0);
            cur_line + off
        } else if let Some(rest) = line_part.strip_prefix('-') {
            let off = rest.parse::<i32>().unwrap_or(0).max(0);
            (cur_line - off).max(0)
        } else {
            match line_part.parse::<i32>() {
                Ok(n) => (n - 1).max(0),
                Err(_) => return false,
            }
        };

        let mut iter = if target_line >= line_count {
            buffer.end_iter()
        } else {
            buffer
                .iter_at_line(target_line)
                .unwrap_or_else(|| buffer.end_iter())
        };

        if let Some(col_str) = col_part {
            if let Ok(col) = col_str.parse::<i32>() {
                let col = col.max(0);
                let chars_in_line = iter.chars_in_line();
                // chars_in_line includes the newline when present; clamp to line end.
                let max_off = if iter.ends_line() {
                    chars_in_line
                } else {
                    chars_in_line.saturating_sub(1)
                };
                iter.set_line_offset(col.min(max_off).max(0));
            }
        }

        buffer.place_cursor(&iter);
        let mark = buffer.get_insert();
        view.scroll_to_mark(&mark, 0.25, true, 0.0, 0.5);
        view.grab_focus();
        self.hide();
        true
    }
}

pub fn clear_search_highlights(buffer: &sourceview5::Buffer) {
    let settings = sourceview5::SearchSettings::new();
    let context = sourceview5::SearchContext::new(buffer, Some(&settings));
    context.set_highlight(false);
}

/// Iter to begin a search from so Find Next/Prev skips the current hit.
pub fn search_iter(buffer: &sourceview5::Buffer, forward: bool) -> gtk::TextIter {
    if let Some((start, end)) = buffer.selection_bounds() {
        if forward {
            end
        } else {
            start
        }
    } else {
        buffer.iter_at_mark(&buffer.get_insert())
    }
}
