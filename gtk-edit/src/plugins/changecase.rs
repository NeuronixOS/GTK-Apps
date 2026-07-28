use gtk4 as gtk;
use gtk::gio;
use gtk::prelude::*;

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

struct ChangeCasePlugin {
    info: PluginInfo,
    actions: Vec<gio::SimpleAction>,
}

impl Plugin for ChangeCasePlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for ChangeCasePlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        let win = ctx.window.clone();
        for (name, label, transform) in [
            ("changecase-upper", "All Upper Case", Transform::Upper),
            ("changecase-lower", "All Lower Case", Transform::Lower),
            ("changecase-title", "Invert Case", Transform::Invert),
            ("changecase-titlecase", "Title Case", Transform::Title),
        ] {
            let action = gio::SimpleAction::new(name, None);
            let win2 = win.clone();
            action.connect_activate(move |_, _| {
                apply_transform(&win2, transform);
            });
            win.add_action(&action);
            let icon = match name {
                "changecase-upper" => "format-text-uppercase-symbolic",
                "changecase-lower" => "format-text-lowercase-symbolic",
                "changecase-title" => "format-text-rich-symbolic",
                "changecase-titlecase" => "format-text-rich-symbolic",
                _ => "emblem-system-symbolic",
            };
            ctx.menu_icons.borrow_mut().append(
                &ctx.tools_menu,
                label,
                &format!("win.{name}"),
                icon,
            );
            self.actions.push(action);
        }
    }

    fn deactivate(&mut self) {
        self.actions.clear();
    }
}

#[derive(Clone, Copy)]
enum Transform {
    Upper,
    Lower,
    Invert,
    Title,
}

fn apply_transform(win: &gtk::ApplicationWindow, t: Transform) {
    let Some(tab) = crate::window::current_tab_from_window(win) else {
        return;
    };
    let buf = &tab.document.buffer;
    let Some((mut start, mut end)) = buf.selection_bounds() else {
        return;
    };
    let text = buf.text(&start, &end, false).to_string();
    let new = match t {
        Transform::Upper => text.to_uppercase(),
        Transform::Lower => text.to_lowercase(),
        Transform::Invert => text
            .chars()
            .map(|c| {
                if c.is_uppercase() {
                    c.to_lowercase().collect::<String>()
                } else {
                    c.to_uppercase().collect::<String>()
                }
            })
            .collect(),
        Transform::Title => {
            let mut out = String::new();
            let mut cap = true;
            for c in text.chars() {
                if c.is_whitespace() {
                    out.push(c);
                    cap = true;
                } else if cap {
                    out.extend(c.to_uppercase());
                    cap = false;
                } else {
                    out.extend(c.to_lowercase());
                }
            }
            out
        }
    };
    buf.begin_user_action();
    buf.delete(&mut start, &mut end);
    buf.insert(&mut start, &new);
    buf.end_user_action();
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info(
        "changecase",
        "Change Case",
        "Changes the case of selected text.",
    );
    make_factory(i.clone(), move || {
        Box::new(ChangeCasePlugin {
            info: i.clone(),
            actions: Vec::new(),
        })
    })
}
