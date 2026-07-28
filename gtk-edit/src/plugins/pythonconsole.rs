use std::cell::RefCell;

use gtk4 as gtk;
use gtk::prelude::*;
use vte4::prelude::*;

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

struct PythonConsolePlugin {
    info: PluginInfo,
    term: RefCell<Option<vte4::Terminal>>,
}

impl Plugin for PythonConsolePlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for PythonConsolePlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        let term = vte4::Terminal::new();
        term.set_size(80, 12);
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&term)
            .vexpand(true)
            .hexpand(true)
            .build();

        // Spawn python3 -i
        let argv = ["python3", "-i"];
        let _ = term.spawn_async(
            vte4::PtyFlags::DEFAULT,
            None,
            &argv,
            &[],
            gtk::glib::SpawnFlags::DEFAULT,
            || {},
            -1,
            gtk::gio::Cancellable::NONE,
            |_| {},
        );

        let notebook = find_bottom_notebook(&ctx.bottom_panel);
        if let Some(nb) = notebook {
            nb.append_page(&scrolled, Some(&gtk::Label::new(Some("Python Console"))));
        } else {
            ctx.bottom_panel.append(&scrolled);
        }
        ctx.bottom_panel.set_visible(true);
        *self.term.borrow_mut() = Some(term);
    }

    fn deactivate(&mut self) {
        if let Some(term) = self.term.borrow_mut().take() {
            if let Some(parent) = term.parent() {
                if let Some(scroll) = parent.downcast_ref::<gtk::ScrolledWindow>() {
                    if let Some(nb_parent) = scroll.parent() {
                        if let Ok(nb) = nb_parent.downcast::<gtk::Notebook>() {
                            nb.detach_tab(scroll);
                        }
                    }
                }
            }
        }
    }
}

fn find_bottom_notebook(bottom: &gtk::Box) -> Option<gtk::Notebook> {
    let mut child = bottom.first_child();
    while let Some(c) = child {
        let next = c.next_sibling();
        if let Ok(nb) = c.clone().downcast::<gtk::Notebook>() {
            return Some(nb);
        }
        if let Some(box_) = c.downcast_ref::<gtk::Box>() {
            if let Some(nb) = find_bottom_notebook(box_) {
                return Some(nb);
            }
        }
        child = next;
    }
    None
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info(
        "pythonconsole",
        "Python Console",
        "Interactive Python console in the bottom panel (python3 -i via VTE).",
    );
    make_factory(i.clone(), move || {
        Box::new(PythonConsolePlugin {
            info: i.clone(),
            term: RefCell::new(None),
        })
    })
}
