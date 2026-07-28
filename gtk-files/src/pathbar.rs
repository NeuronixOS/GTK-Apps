//! Breadcrumb path bar with editable location entry.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::util::home_dir;

pub struct PathBar {
    pub root: gtk::Box,
    buttons: gtk::Box,
    entry: gtk::Entry,
    stack: gtk::Stack,
    current: RefCell<PathBuf>,
    on_navigate: RefCell<Option<Rc<dyn Fn(PathBuf)>>>,
    on_open_tab: RefCell<Option<Rc<dyn Fn(PathBuf)>>>,
    on_open_window: RefCell<Option<Rc<dyn Fn(PathBuf)>>>,
}

impl PathBar {
    pub fn new() -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("pathbar");
        root.set_hexpand(true);

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);

        let scroll = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        buttons.add_css_class("linked");
        scroll.set_child(Some(&buttons));

        let entry = gtk::Entry::builder()
            .hexpand(true)
            .placeholder_text("Enter location…")
            .build();

        let entry_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        entry_box.add_css_class("linked");
        entry_box.set_hexpand(true);
        entry_box.append(&entry);
        let close_btn = gtk::Button::from_icon_name("window-close-symbolic");
        close_btn.set_tooltip_text(Some("Cancel"));
        entry_box.append(&close_btn);

        stack.add_named(&scroll, Some("path"));
        stack.add_named(&entry_box, Some("entry"));
        stack.set_visible_child_name("path");

        root.append(&stack);

        let bar = Rc::new(Self {
            root,
            buttons,
            entry: entry.clone(),
            stack: stack.clone(),
            current: RefCell::new(home_dir()),
            on_navigate: RefCell::new(None),
            on_open_tab: RefCell::new(None),
            on_open_window: RefCell::new(None),
        });

        {
            let bar2 = Rc::clone(&bar);
            let click = gtk::GestureClick::new();
            click.set_button(1);
            // Double-click empty path area → edit location
            click.connect_pressed(move |_, n_press, _, _| {
                if n_press == 2 {
                    bar2.show_entry();
                }
            });
            bar.buttons.add_controller(click);
        }

        {
            let bar2 = Rc::clone(&bar);
            entry.connect_activate(move |e| {
                let text = e.text().to_string();
                bar2.go_to_text(&text);
            });
        }

        {
            let bar2 = Rc::clone(&bar);
            close_btn.connect_clicked(move |_| {
                bar2.hide_entry();
            });
        }

        // Ctrl+L handled by window action; also support Escape in entry
        {
            let bar2 = Rc::clone(&bar);
            let key = gtk::EventControllerKey::new();
            key.connect_key_pressed(move |_, key, _, _| {
                if key == gdk::Key::Escape {
                    bar2.hide_entry();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
            entry.add_controller(key);
        }

        bar
    }

    pub fn set_on_navigate<F: Fn(PathBuf) + 'static>(&self, f: F) {
        *self.on_navigate.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_on_open_tab<F: Fn(PathBuf) + 'static>(&self, f: F) {
        *self.on_open_tab.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_on_open_window<F: Fn(PathBuf) + 'static>(&self, f: F) {
        *self.on_open_window.borrow_mut() = Some(Rc::new(f));
    }

    pub fn show_entry(&self) {
        let path = self.current.borrow().clone();
        self.entry.set_text(&path.to_string_lossy());
        self.stack.set_visible_child_name("entry");
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    pub fn hide_entry(&self) {
        self.stack.set_visible_child_name("path");
    }

    pub fn set_location(&self, path: &Path) {
        *self.current.borrow_mut() = path.to_path_buf();
        self.rebuild_buttons(path);
        self.hide_entry();
    }

    fn navigate(&self, path: PathBuf) {
        if let Some(cb) = self.on_navigate.borrow().as_ref() {
            cb(path);
        }
    }

    fn go_to_text(&self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let path = if text.starts_with('~') {
            let home = home_dir();
            if text == "~" {
                home
            } else if let Some(rest) = text.strip_prefix("~/") {
                home.join(rest)
            } else {
                PathBuf::from(text)
            }
        } else if text.starts_with("trash:") {
            // Handled as URI by caller via navigate callback — pass special marker
            if let Some(cb) = self.on_navigate.borrow().as_ref() {
                // Use a sentinel empty path with trash — window handles trash URI separately
                // Store trash by navigating via file URI path representation
                cb(PathBuf::from("trash:///"));
            }
            self.hide_entry();
            return;
        } else {
            PathBuf::from(text)
        };

        if path.exists() {
            self.navigate(path);
            self.hide_entry();
        } else {
            // Try as URI
            let file = gio::File::for_commandline_arg(text);
            if let Some(p) = file.path() {
                if p.exists() || file.uri().starts_with("trash:") {
                    self.navigate(p);
                    self.hide_entry();
                    return;
                }
            }
            if file.uri().starts_with("trash:") {
                self.navigate(PathBuf::from("trash:///"));
                self.hide_entry();
            }
        }
    }

    fn rebuild_buttons(&self, path: &Path) {
        while let Some(child) = self.buttons.first_child() {
            self.buttons.remove(&child);
        }

        if path.to_string_lossy() == "trash:///" || path.starts_with("trash:") {
            let btn = gtk::Button::with_label("Trash");
            btn.add_css_class("flat");
            btn.set_sensitive(false);
            self.buttons.append(&btn);
            return;
        }

        let home = home_dir();
        let components: Vec<PathBuf> = if path.starts_with(&home) {
            let mut v = vec![home.clone()];
            if let Ok(rel) = path.strip_prefix(&home) {
                let mut acc = home.clone();
                for c in rel.components() {
                    acc.push(c);
                    v.push(acc.clone());
                }
            }
            v
        } else {
            let mut v = Vec::new();
            let mut acc = PathBuf::new();
            for c in path.components() {
                acc.push(c);
                v.push(acc.clone());
            }
            if v.is_empty() {
                v.push(PathBuf::from("/"));
            }
            v
        };

        let on_tab = self.on_open_tab.borrow().clone();
        let on_window = self.on_open_window.borrow().clone();

        for (i, comp) in components.iter().enumerate() {
            let label = if i == 0 && *comp == home {
                "Home".to_string()
            } else if comp == Path::new("/") {
                "Computer".to_string()
            } else {
                comp.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| comp.display().to_string())
            };
            let btn = gtk::Button::with_label(&label);
            btn.add_css_class("flat");
            btn.set_tooltip_text(Some("Open in New Tab (middle-click too)"));
            let path = comp.clone();

            // Primary click → new tab
            {
                let on_tab = on_tab.clone();
                let path = path.clone();
                btn.connect_clicked(move |_| {
                    if let Some(cb) = &on_tab {
                        cb(path.clone());
                    }
                });
            }

            // Middle click → new tab
            {
                let on_tab = on_tab.clone();
                let path = path.clone();
                let mid = gtk::GestureClick::new();
                mid.set_button(2);
                mid.connect_pressed(move |g, _, _, _| {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    if let Some(cb) = &on_tab {
                        cb(path.clone());
                    }
                });
                btn.add_controller(mid);
            }

            // Right click → Open in New Tab / Open in New Window
            install_crumb_menu(&btn, path, on_tab.clone(), on_window.clone());

            self.buttons.append(&btn);
        }
    }
}

fn install_crumb_menu(
    btn: &gtk::Button,
    path: PathBuf,
    on_tab: Option<Rc<dyn Fn(PathBuf)>>,
    on_window: Option<Rc<dyn Fn(PathBuf)>>,
) {
    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();
    icons.append_action(&menu, "Open in New Tab", "crumb.open-tab");
    icons.append_action(&menu, "Open in New Window", "crumb.open-window");

    let group = gio::SimpleActionGroup::new();
    {
        let on_tab = on_tab.clone();
        let path = path.clone();
        let act = gio::SimpleAction::new("open-tab", None);
        act.connect_activate(move |_, _| {
            if let Some(cb) = &on_tab {
                cb(path.clone());
            }
        });
        group.add_action(&act);
    }
    {
        let on_window = on_window.clone();
        let path = path.clone();
        let act = gio::SimpleAction::new("open-window", None);
        act.connect_activate(move |_, _| {
            if let Some(cb) = &on_window {
                cb(path.clone());
            }
        });
        group.add_action(&act);
    }
    btn.insert_action_group("crumb", Some(&group));

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    icons.bind_popover(&popover);
    popover.set_has_arrow(false);
    popover.set_parent(btn);
    {
        let popover_weak = popover.downgrade();
        btn.connect_destroy(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.unparent();
            }
        });
    }

    let right = gtk::GestureClick::new();
    right.set_button(3);
    {
        let popover = popover.clone();
        right.connect_pressed(move |g, _, x, y| {
            g.set_state(gtk::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
    }
    btn.add_controller(right);
}
