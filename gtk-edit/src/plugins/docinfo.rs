use gtk4 as gtk;
use gtk::gio;
use gtk::prelude::*;

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

struct DocInfoPlugin {
    info: PluginInfo,
    action: Option<gio::SimpleAction>,
}

impl Plugin for DocInfoPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for DocInfoPlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        let win = ctx.window.clone();
        let action = gio::SimpleAction::new("docinfo", None);
        action.connect_activate(move |_, _| show_docinfo(&win));
        ctx.window.add_action(&action);
        ctx.menu_icons.borrow_mut().append(
            &ctx.tools_menu,
            "Document Statistics…",
            "win.docinfo",
            "dialog-information-symbolic",
        );
        self.action = Some(action);
    }

    fn deactivate(&mut self) {
        self.action = None;
    }
}

fn show_docinfo(win: &gtk::ApplicationWindow) {
    let Some(tab) = crate::window::current_tab_from_window(win) else {
        return;
    };
    let text = tab.document.text();
    let lines = text.lines().count();
    let words = text.split_whitespace().count();
    let chars = text.chars().count();
    let bytes = text.len();
    let detail = format!(
        "Lines: {lines}\nWords: {words}\nCharacters: {chars}\nBytes: {bytes}"
    );
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message("Document Statistics")
        .detail(&detail)
        .buttons(["Close"])
        .build();
    dialog.show(Some(win));
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info(
        "docinfo",
        "Document Statistics",
        "Shows word, line, and character counts for the document.",
    );
    make_factory(i.clone(), move || {
        Box::new(DocInfoPlugin {
            info: i.clone(),
            action: None,
        })
    })
}
