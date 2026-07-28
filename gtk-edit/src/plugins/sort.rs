use gtk4 as gtk;
use gtk::gio;
use gtk::prelude::*;

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

struct SortPlugin {
    info: PluginInfo,
    action: Option<gio::SimpleAction>,
}

impl Plugin for SortPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for SortPlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        let win = ctx.window.clone();
        let action = gio::SimpleAction::new("sort", None);
        action.connect_activate(move |_, _| show_sort(&win));
        ctx.window.add_action(&action);
        ctx.menu_icons
            .borrow_mut()
            .append_action(&ctx.tools_menu, "Sort…", "win.sort");
        self.action = Some(action);
    }

    fn deactivate(&mut self) {
        self.action = None;
    }
}

fn show_sort(win: &gtk::ApplicationWindow) {
    let Some(tab) = crate::window::current_tab_from_window(win) else {
        return;
    };
    let dialog = gtk::Window::builder()
        .title("Sort")
        .transient_for(win)
        .modal(true)
        .default_width(320)
        .build();
    gtk_theme::style_dialog(&dialog);
    let reverse = gtk::CheckButton::with_label("Reverse order");
    let ignore_case = gtk::CheckButton::with_label("Ignore case");
    ignore_case.set_active(true);
    let remove_dups = gtk::CheckButton::with_label("Remove duplicates");
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 8);
    box_.set_margin_top(12);
    box_.set_margin_bottom(12);
    box_.set_margin_start(12);
    box_.set_margin_end(12);
    box_.append(&reverse);
    box_.append(&ignore_case);
    box_.append(&remove_dups);
    let ok = gtk_theme::labeled_button("view-sort-ascending-symbolic", "Sort");
    let cancel = gtk_theme::labeled_button(gtk_theme::icon_for_label("Cancel"), "Cancel");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&cancel);
    buttons.append(&ok);
    box_.append(&buttons);
    dialog.set_child(Some(&box_));

    {
        let d = dialog.clone();
        cancel.connect_clicked(move |_| d.close());
    }
    {
        let d = dialog.clone();
        let buffer = tab.document.buffer.clone();
        ok.connect_clicked(move |_| {
            let (mut start, mut end) = match buffer.selection_bounds() {
                Some(bounds) => bounds,
                None => (buffer.start_iter(), buffer.end_iter()),
            };
            // Expand to whole lines
            start.set_line_offset(0);
            if !end.starts_line() {
                end.forward_to_line_end();
                end.forward_char();
            }
            let text = buffer.text(&start, &end, false).to_string();
            let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
            if ignore_case.is_active() {
                lines.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
            } else {
                lines.sort();
            }
            if reverse.is_active() {
                lines.reverse();
            }
            if remove_dups.is_active() {
                lines.dedup();
            }
            let mut out = lines.join("\n");
            if text.ends_with('\n') {
                out.push('\n');
            }
            buffer.begin_user_action();
            buffer.delete(&mut start, &mut end);
            buffer.insert(&mut start, &out);
            buffer.end_user_action();
            d.close();
        });
    }
    dialog.present();
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info("sort", "Sort", "Sorts lines in the document or selection.");
    make_factory(i.clone(), move || {
        Box::new(SortPlugin {
            info: i.clone(),
            action: None,
        })
    })
}
