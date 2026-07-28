use chrono::Local;
use gtk4 as gtk;
use gtk::gio;
use gtk::prelude::*;

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

struct TimePlugin {
    info: PluginInfo,
    action: Option<gio::SimpleAction>,
}

impl Plugin for TimePlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for TimePlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        let win = ctx.window.clone();
        let action = gio::SimpleAction::new("insert-date", None);
        action.connect_activate(move |_, _| insert_date(&win));
        ctx.window.add_action(&action);
        ctx.menu_icons.borrow_mut().append(
            &ctx.tools_menu,
            "Insert Date and Time…",
            "win.insert-date",
            "x-office-calendar-symbolic",
        );
        self.action = Some(action);
    }

    fn deactivate(&mut self) {
        self.action = None;
    }
}

fn insert_date(win: &gtk::ApplicationWindow) {
    let Some(tab) = crate::window::current_tab_from_window(win) else {
        return;
    };
    let formats = [
        "%c",
        "%Y-%m-%d",
        "%Y-%m-%d %H:%M:%S",
        "%d/%m/%Y",
        "%H:%M:%S",
    ];
    let dialog = gtk::Window::builder()
        .title("Insert Date and Time")
        .transient_for(win)
        .modal(true)
        .default_width(360)
        .build();
    gtk_theme::style_dialog(&dialog);
    let list = gtk::ListBox::new();
    for fmt in formats {
        let sample = Local::now().format(fmt).to_string();
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::new(Some(&format!("{sample}  ({fmt})")));
        label.set_halign(gtk::Align::Start);
        label.set_margin_start(8);
        label.set_margin_top(4);
        label.set_margin_bottom(4);
        row.set_child(Some(&label));
        unsafe {
            row.set_data("fmt", fmt.to_string());
        }
        list.append(&row);
    }
    let insert = gtk_theme::labeled_button("list-add-symbolic", "Insert");
    let cancel = gtk_theme::labeled_button(gtk_theme::icon_for_label("Cancel"), "Cancel");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&cancel);
    buttons.append(&insert);
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 8);
    outer.set_margin_top(12);
    outer.set_margin_bottom(12);
    outer.set_margin_start(12);
    outer.set_margin_end(12);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .min_content_height(180)
        .build();
    outer.append(&scroll);
    outer.append(&buttons);
    dialog.set_child(Some(&outer));

    {
        let d = dialog.clone();
        cancel.connect_clicked(move |_| d.close());
    }
    {
        let d = dialog.clone();
        let view = tab.view.clone();
        let buffer = tab.document.buffer.clone();
        insert.connect_clicked(move |_| {
            if let Some(row) = list.selected_row() {
                let fmt = unsafe {
                    row.data::<String>("fmt")
                        .map(|p| p.as_ref().clone())
                        .unwrap_or_else(|| "%c".into())
                };
                let s = Local::now().format(&fmt).to_string();
                let mut iter = buffer.iter_at_mark(&buffer.get_insert());
                buffer.insert(&mut iter, &s);
                view.grab_focus();
            }
            d.close();
        });
    }
    dialog.present();
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info(
        "time",
        "Insert Date/Time",
        "Inserts the current date and time at the cursor position.",
    );
    make_factory(i.clone(), move || {
        Box::new(TimePlugin {
            info: i.clone(),
            action: None,
        })
    })
}
