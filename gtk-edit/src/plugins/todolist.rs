//! Todo List — clean status markers and divider lines from the active document
//! (ported from the gedit todo-list plugin).

use gtk4 as gtk;
use gtk::gio;
use gtk::prelude::*;

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

fn long_dash_line() -> String {
    "➖️".repeat(58)
}

fn long_block_line() -> String {
    "🬂".repeat(72)
}

struct TodoListPlugin {
    info: PluginInfo,
}

impl Plugin for TodoListPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for TodoListPlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        let win = ctx.window.clone();
        let action = gio::SimpleAction::new("todo-list", None);
        action.connect_activate(move |_, _| show_todo_dialog(&win));
        ctx.window.add_action(&action);
        ctx.menu_icons.borrow_mut().append(
            &ctx.tools_menu,
            "Todo List…",
            "win.todo-list",
            "view-list-symbolic",
        );

        if let Some(app) = ctx.window.application() {
            app.set_accels_for_action("win.todo-list", &["<Primary>r"]);
        }
    }

    fn deactivate(&mut self) {}
}

fn show_todo_dialog(win: &gtk::ApplicationWindow) {
    let dialog = gtk::Window::builder()
        .title("TODO List")
        .transient_for(win)
        .modal(true)
        .default_width(320)
        .default_height(120)
        .build();
    gtk_theme::style_dialog(&dialog);

    let hint = gtk::Label::new(Some(
        "Start cleans status markers (💤 ✓ …) and normalizes divider lines in the active document.",
    ));
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    hint.add_css_class("dim-label");

    let start = gtk_theme::labeled_button("media-playback-start-symbolic", "Start");
    start.add_css_class("suggested-action");
    let close = gtk_theme::labeled_button(gtk_theme::icon_for_label("Close"), "Close");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&close);
    buttons.append(&start);

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);
    outer.set_margin_start(16);
    outer.set_margin_end(16);
    outer.append(&hint);
    outer.append(&buttons);
    dialog.set_child(Some(&outer));

    {
        let d = dialog.clone();
        close.connect_clicked(move |_| d.close());
    }
    {
        let d = dialog.clone();
        let win = win.clone();
        start.connect_clicked(move |_| {
            run_todo_cleanup(&win);
            d.close();
        });
    }
    dialog.present();
}

fn run_todo_cleanup(win: &gtk::ApplicationWindow) {
    let Some(tab) = crate::window::current_tab_from_window(win) else {
        return;
    };
    let buffer = &tab.document.buffer;
    let start = buffer.start_iter();
    let end = buffer.end_iter();
    let mut text = buffer.text(&start, &end, true).to_string();

    for marker in ["💤 ", "✓ ", "..."] {
        text = text.replace(marker, "");
    }

    let dash = long_dash_line();
    let block = long_block_line();
    text = text.replace(&dash, "");
    text = text.replace(&block, &format!("{block}\n{dash}"));

    let cleaned: String = text
        .split_inclusive('\n')
        .map(|line| {
            if line.trim().is_empty() {
                "\n".to_string()
            } else {
                line.to_string()
            }
        })
        .collect();
    let cleaned = cleaned.replace("\n\n\n", "\n");

    buffer.begin_user_action();
    let mut s = buffer.start_iter();
    let mut e = buffer.end_iter();
    buffer.delete(&mut s, &mut e);
    let mut at = buffer.start_iter();
    buffer.insert(&mut at, &cleaned);
    buffer.end_user_action();
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info(
        "todolist",
        "Todo List",
        "Simple TODO helper that cleans status markers and divider lines (Ctrl+R).",
    );
    make_factory(i.clone(), move || Box::new(TodoListPlugin { info: i.clone() }))
}
