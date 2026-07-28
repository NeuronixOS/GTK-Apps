use std::collections::HashMap;

use gtk4 as gtk;
use gtk::gio;
use gtk::prelude::*;

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

struct SnippetsPlugin {
    info: PluginInfo,
    action: Option<gio::SimpleAction>,
    snippets: HashMap<String, String>,
}

impl Plugin for SnippetsPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for SnippetsPlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        let win = ctx.window.clone();
        let snippets = self.snippets.clone();
        let action = gio::SimpleAction::new("insert-snippet", None);
        action.connect_activate(move |_, _| show_snippets(&win, &snippets));
        ctx.window.add_action(&action);
        ctx.menu_icons.borrow_mut().append(
            &ctx.tools_menu,
            "Insert Snippet…",
            "win.insert-snippet",
            "insert-text-symbolic",
        );

        // Tab-trigger via key controller on activate of manage
        let expand = gio::SimpleAction::new("expand-snippet", None);
        let win2 = ctx.window.clone();
        let snippets2 = self.snippets.clone();
        expand.connect_activate(move |_, _| try_expand(&win2, &snippets2));
        ctx.window.add_action(&expand);
        self.action = Some(action);
    }

    fn deactivate(&mut self) {
        self.action = None;
    }
}

fn default_snippets() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("gpl".into(), "/*\n * Copyright (C) YEAR AUTHOR\n * SPDX-License-Identifier: GPL-2.0-or-later\n */\n".into());
    m.insert(
        "html".into(),
        "<!DOCTYPE html>\n<html>\n<head>\n  <meta charset=\"utf-8\">\n  <title></title>\n</head>\n<body>\n  \n</body>\n</html>\n".into(),
    );
    m.insert("mainc".into(), "#include <stdio.h>\n\nint main(int argc, char **argv)\n{\n    \n    return 0;\n}\n".into());
    m.insert(
        "fn".into(),
        "fn name() {\n    \n}\n".into(),
    );
    m.insert(
        "if".into(),
        "if condition {\n    \n}\n".into(),
    );
    m.insert(
        "for".into(),
        "for item in collection {\n    \n}\n".into(),
    );
    m
}

fn show_snippets(win: &gtk::ApplicationWindow, snippets: &HashMap<String, String>) {
    let dialog = gtk::Window::builder()
        .title("Snippets")
        .transient_for(win)
        .modal(true)
        .default_width(400)
        .default_height(300)
        .build();
    gtk_theme::style_dialog(&dialog);
    let list = gtk::ListBox::new();
    let mut keys: Vec<_> = snippets.keys().cloned().collect();
    keys.sort();
    for key in keys {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::new(Some(&key));
        label.set_halign(gtk::Align::Start);
        label.set_margin_start(8);
        label.set_margin_top(4);
        label.set_margin_bottom(4);
        row.set_child(Some(&label));
        unsafe {
            row.set_data("key", key);
        }
        list.append(&row);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .build();
    dialog.set_child(Some(&scroll));
    let win2 = win.clone();
    let snippets = snippets.clone();
    let d = dialog.clone();
    list.connect_row_activated(move |_, row| {
        let key = unsafe {
            row.data::<String>("key")
                .map(|p| p.as_ref().clone())
                .unwrap_or_default()
        };
        if let Some(body) = snippets.get(&key) {
            insert_text(&win2, body);
        }
        d.close();
    });
    dialog.present();
}

fn try_expand(win: &gtk::ApplicationWindow, snippets: &HashMap<String, String>) {
    let Some(tab) = crate::window::current_tab_from_window(win) else {
        return;
    };
    let buffer = &tab.document.buffer;
    let insert = buffer.get_insert();
    let mut iter = buffer.iter_at_mark(&insert);
    let mut start = iter;
    while start.backward_char() {
        let ch = start.char();
        if !ch.is_alphanumeric() && ch != '_' {
            start.forward_char();
            break;
        }
    }
    let word = buffer.text(&start, &iter, false).to_string();
    if let Some(body) = snippets.get(&word) {
        buffer.begin_user_action();
        buffer.delete(&mut start, &mut iter);
        buffer.insert(&mut start, body);
        buffer.end_user_action();
    }
}

fn insert_text(win: &gtk::ApplicationWindow, text: &str) {
    let Some(tab) = crate::window::current_tab_from_window(win) else {
        return;
    };
    let buffer = &tab.document.buffer;
    let mut iter = buffer.iter_at_mark(&buffer.get_insert());
    buffer.insert(&mut iter, text);
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info(
        "snippets",
        "Snippets",
        "Insert code snippets; use Expand Snippet action after typing a trigger.",
    );
    make_factory(i.clone(), move || {
        Box::new(SnippetsPlugin {
            info: i.clone(),
            action: None,
            snippets: default_snippets(),
        })
    })
}
