//! Find & Replace dialog.

use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use sourceview5::prelude::*;

use crate::config::Config;
use crate::search::search_iter;

pub struct ReplaceDialog {
    pub dialog: gtk::Window,
    pub search_entry: gtk::Entry,
    pub replace_entry: gtk::Entry,
    pub match_case: gtk::CheckButton,
    pub whole_word: gtk::CheckButton,
    pub regex: gtk::CheckButton,
    pub wrap_around: gtk::CheckButton,
    pub find_btn: gtk::Button,
    pub replace_btn: gtk::Button,
    pub replace_all_btn: gtk::Button,
}

impl ReplaceDialog {
    pub fn new(parent: &impl IsA<gtk::Window>) -> Rc<Self> {
        let dialog = gtk::Window::builder()
            .title("Replace")
            .transient_for(parent)
            .modal(true)
            .default_width(420)
            .build();
        gtk_theme::style_dialog(&dialog);

        let search_entry = gtk::Entry::builder()
            .placeholder_text("Search for")
            .hexpand(true)
            .build();
        let replace_entry = gtk::Entry::builder()
            .placeholder_text("Replace with")
            .hexpand(true)
            .build();

        let match_case = gtk::CheckButton::with_label("Match case");
        let whole_word = gtk::CheckButton::with_label("Match entire word only");
        let regex = gtk::CheckButton::with_label("Regular expression");
        let wrap_around = gtk::CheckButton::with_label("Wrap around");
        wrap_around.set_active(true);

        let grid = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(8)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        grid.attach(&gtk::Label::new(Some("Search for:")), 0, 0, 1, 1);
        grid.attach(&search_entry, 1, 0, 1, 1);
        grid.attach(&gtk::Label::new(Some("Replace with:")), 0, 1, 1, 1);
        grid.attach(&replace_entry, 1, 1, 1, 1);
        grid.attach(&match_case, 1, 2, 1, 1);
        grid.attach(&whole_word, 1, 3, 1, 1);
        grid.attach(&regex, 1, 4, 1, 1);
        grid.attach(&wrap_around, 1, 5, 1, 1);

        let find_btn = gtk_theme::labeled_button("edit-find-symbolic", "Find");
        let replace_btn =
            gtk_theme::labeled_button(gtk_theme::icon_for_label("Replace"), "Replace");
        let replace_all_btn =
            gtk_theme::labeled_button(gtk_theme::icon_for_label("Replace All"), "Replace All");
        let close_btn =
            gtk_theme::labeled_button(gtk_theme::icon_for_label("Close"), "Close");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        buttons.set_halign(gtk::Align::End);
        buttons.append(&find_btn);
        buttons.append(&replace_btn);
        buttons.append(&replace_all_btn);
        buttons.append(&close_btn);
        grid.attach(&buttons, 0, 6, 2, 1);

        dialog.set_child(Some(&grid));

        {
            let d = dialog.clone();
            close_btn.connect_clicked(move |_| d.close());
        }

        Rc::new(Self {
            dialog,
            search_entry,
            replace_entry,
            match_case,
            whole_word,
            regex,
            wrap_around,
            find_btn,
            replace_btn,
            replace_all_btn,
        })
    }

    /// Show the dialog. `initial_search` (e.g. current selection) wins over history.
    pub fn present_with(&self, config: &Config, initial_search: Option<&str>) {
        if let Some(text) = initial_search.filter(|t| !t.is_empty()) {
            self.search_entry.set_text(text);
        } else if let Some(last) = config.state.search_history.first() {
            self.search_entry.set_text(last);
        }
        if let Some(last) = config.state.replace_history.first() {
            self.replace_entry.set_text(last);
        }
        self.dialog.present();
        self.search_entry.grab_focus();
        self.search_entry.select_region(0, -1);
    }

    fn settings(&self) -> sourceview5::SearchSettings {
        let settings = sourceview5::SearchSettings::new();
        settings.set_search_text(Some(self.search_entry.text().as_str()));
        settings.set_case_sensitive(self.match_case.is_active());
        settings.set_at_word_boundaries(self.whole_word.is_active());
        settings.set_regex_enabled(self.regex.is_active());
        settings.set_wrap_around(self.wrap_around.is_active());
        settings
    }

    pub fn find_next(&self, buffer: &sourceview5::Buffer, view: &sourceview5::View) -> bool {
        let settings = self.settings();
        // Wrap is on by default; keep pressing Find cycles through all hits.
        let context = sourceview5::SearchContext::new(buffer, Some(&settings));
        context.set_highlight(true);
        let iter = search_iter(buffer, true);
        if let Some((start, end, _)) = context.forward(&iter) {
            buffer.select_range(&start, &end);
            view.scroll_to_iter(&mut start.clone(), 0.2, false, 0.0, 0.0);
            true
        } else {
            false
        }
    }

    pub fn replace_one(&self, buffer: &sourceview5::Buffer, view: &sourceview5::View) -> bool {
        let replacement = self.replace_entry.text().to_string();
        if let Some((mut start, mut end)) = buffer.selection_bounds() {
            buffer.begin_user_action();
            buffer.delete(&mut start, &mut end);
            buffer.insert(&mut start, &replacement);
            buffer.end_user_action();
        }
        self.find_next(buffer, view)
    }

    pub fn replace_all(&self, buffer: &sourceview5::Buffer) -> u32 {
        let settings = self.settings();
        let context = sourceview5::SearchContext::new(buffer, Some(&settings));
        let replacement = self.replace_entry.text();
        // sourceview5 replace_all returns Result<(), Error> in some versions;
        // count occurrences via a simple loop fallback.
        let mut count = 0u32;
        let mut iter = buffer.start_iter();
        while let Some((start, end, _)) = context.forward(&iter) {
            let mut s = start;
            let mut e = end;
            buffer.begin_user_action();
            buffer.delete(&mut s, &mut e);
            buffer.insert(&mut s, replacement.as_str());
            buffer.end_user_action();
            count += 1;
            iter = s;
            if count > 100_000 {
                break;
            }
        }
        count
    }
}
