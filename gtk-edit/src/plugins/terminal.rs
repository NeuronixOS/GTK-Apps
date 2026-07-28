//! Bottom-panel VTE terminal that tracks the directory of the active document
//! (same idea as gtk-files' embedded terminal).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use vte4::prelude::*;

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

struct TerminalState {
    root: gtk::Box,
    terminal: vte4::Terminal,
    cwd: RefCell<PathBuf>,
    alive: Cell<bool>,
    /// When false, the terminal lives in its own window and no longer follows
    /// the editor's active document directory.
    follow_editor: Cell<bool>,
    /// Editor window (used for cwd sync and hiding the bottom panel).
    window: gtk::ApplicationWindow,
    /// Slot holding this state so Close can drop the plugin's reference.
    slot: Rc<RefCell<Option<Rc<TerminalState>>>>,
}

struct TerminalPlugin {
    info: PluginInfo,
    state: Rc<RefCell<Option<Rc<TerminalState>>>>,
}

impl Plugin for TerminalPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for TerminalPlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        let start = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

        let terminal = vte4::Terminal::new();
        let font = gtk::pango::FontDescription::from_string("Monospace 10");
        terminal.set_font(Some(&font));
        terminal.set_scrollback_lines(5000);
        terminal.set_mouse_autohide(true);
        terminal.set_scroll_on_output(false);
        terminal.set_scroll_on_keystroke(true);
        terminal.set_cursor_blink_mode(vte4::CursorBlinkMode::On);
        terminal.set_can_focus(true);
        terminal.set_focusable(true);
        terminal.set_input_enabled(true);
        apply_vte_profile(&terminal, gtk_theme::load_profile());
        {
            let term = terminal.clone();
            gtk_theme::watch_theme(move |profile| {
                apply_vte_profile(&term, profile);
            });
        }
        install_terminal_clipboard(&terminal);

        let scrolled = gtk::ScrolledWindow::builder()
            .child(&terminal)
            .hexpand(true)
            .vexpand(true)
            .build();

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.add_css_class("terminal-panel-header");
        header.set_margin_start(8);
        header.set_margin_end(8);
        header.set_margin_top(4);
        header.set_margin_bottom(2);
        let title = gtk::Label::new(Some("Terminal"));
        title.add_css_class("heading");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        header.append(&title);

        let path_label = gtk::Label::new(Some(&start.display().to_string()));
        path_label.add_css_class("dim-label");
        path_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        path_label.set_xalign(1.0);
        header.append(&path_label);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("terminal-panel");
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.append(&header);
        root.append(&scrolled);

        let state = Rc::new(TerminalState {
            root: root.clone(),
            terminal,
            cwd: RefCell::new(start),
            alive: Cell::new(false),
            follow_editor: Cell::new(true),
            window: ctx.window.clone(),
            slot: Rc::clone(&self.state),
        });

        // Keep path label in sync via data on root for update_state refreshes.
        unsafe {
            root.set_data("path-label", path_label);
        }

        {
            let weak = Rc::downgrade(&state);
            state.terminal.connect_child_exited(move |_term, _status| {
                let Some(panel) = weak.upgrade() else {
                    return;
                };
                // Detached window closed / tab closed — don't respawn.
                if panel.slot.borrow().is_none() {
                    return;
                }
                panel.alive.set(false);
                let weak = Rc::downgrade(&panel);
                glib::idle_add_local_once(move || {
                    if let Some(panel) = weak.upgrade() {
                        if panel.slot.borrow().is_some() {
                            panel.spawn_shell();
                        }
                    }
                });
            });
        }

        let tab_label = gtk::Label::new(Some("Terminal"));
        tab_label.set_margin_start(4);
        tab_label.set_margin_end(4);
        // Box host so right-clicks on the notebook tab reliably hit a widget.
        let tab_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tab_box.append(&tab_label);
        install_terminal_tab_menu(&tab_box, &state);

        if let Some(nb) = find_bottom_notebook(&ctx.bottom_panel) {
            nb.append_page(&root, Some(&tab_box));
            nb.set_current_page(Some(nb.n_pages().saturating_sub(1)));
        } else {
            ctx.bottom_panel.append(&root);
            // No notebook tab — still allow close/move via header right-click.
            install_terminal_tab_menu(&header, &state);
        }
        ctx.bottom_panel.set_visible(true);
        if let Some(ew) = crate::window::current_from_window(&ctx.window) {
            {
                let mut cfg = ew.config.borrow_mut();
                cfg.ui.bottom_panel_visible = true;
                let _ = cfg.save();
            }
            ew.ensure_bottom_panel_height();
            // Direct hook like gtk-files calling terminal.sync_cwd(&folder).
            let state_for_sync = Rc::clone(&state);
            *ew.terminal_sync.borrow_mut() = Some(Rc::new(move |path: &Path| {
                if state_for_sync.follow_editor.get() {
                    state_for_sync.sync_cwd(path);
                }
            }) as Rc<dyn Fn(&Path)>);
        }

        *self.state.borrow_mut() = Some(Rc::clone(&state));
        // Set cwd from the focused document first, then spawn once in that folder
        // (avoid spawning in $HOME and racing a second spawn on sync).
        if let Some(ew) = crate::window::current_from_window(&ctx.window) {
            ew.sync_terminal_cwd();
        } else {
            state.sync_to_active_document();
        }
    }

    fn deactivate(&mut self) {
        if let Some(state) = self.state.borrow_mut().take() {
            if let Some(ew) = crate::window::current_from_window(&state.window) {
                *ew.terminal_sync.borrow_mut() = None;
            }
            state.teardown_ui();
        }
    }

    fn update_state(&mut self) {
        // Fallback for callers that still go through the plugin bus.
        if let Some(state) = self.state.borrow().as_ref() {
            if state.follow_editor.get() {
                if let Some(ew) = crate::window::current_from_window(&state.window) {
                    ew.sync_terminal_cwd();
                } else {
                    state.sync_to_active_document();
                }
            }
        }
    }
}

impl TerminalState {
    fn sync_to_active_document(self: &Rc<Self>) {
        let dir = crate::window::current_tab_from_window(&self.window)
            .and_then(|tab| tab.document.path())
            .and_then(|p| {
                if p.is_dir() {
                    Some(p)
                } else {
                    p.parent().map(|p| p.to_path_buf())
                }
            })
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")));
        self.sync_cwd(&dir);
    }

    fn sync_cwd(self: &Rc<Self>, path: &Path) {
        if !path.is_dir() {
            return;
        }
        let path = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => path.to_path_buf(),
        };
        if *self.cwd.borrow() == path && self.alive.get() {
            return;
        }
        *self.cwd.borrow_mut() = path.clone();
        unsafe {
            if let Some(label) = self.root.data::<gtk::Label>("path-label") {
                label.as_ref().set_text(&path.display().to_string());
            }
        }
        if self.alive.get() {
            feed_cd(&self.terminal, &path);
        } else {
            self.spawn_shell();
        }
    }

    fn spawn_shell(self: &Rc<Self>) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let dir = self.cwd.borrow().to_string_lossy().into_owned();
        let argv = [shell.as_str()];
        let envv: &[&str] = &[];
        let weak = Rc::downgrade(self);

        self.terminal.spawn_async(
            vte4::PtyFlags::DEFAULT,
            Some(dir.as_str()),
            &argv,
            envv,
            glib::SpawnFlags::DEFAULT,
            || {},
            -1,
            gio::Cancellable::NONE,
            move |result| {
                let Some(panel) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(_) => panel.alive.set(true),
                    Err(err) => {
                        panel.alive.set(false);
                        eprintln!("gtk-edit: failed to spawn terminal shell: {err}");
                    }
                }
            },
        );
    }

    fn teardown_ui(self: &Rc<Self>) {
        if let Some(ew) = crate::window::current_from_window(&self.window) {
            *ew.terminal_sync.borrow_mut() = None;
        }
        let root = self.root.clone();
        if let Some(parent) = root.parent() {
            if let Ok(nb) = parent.clone().downcast::<gtk::Notebook>() {
                nb.detach_tab(&root);
                maybe_hide_empty_bottom_panel(&self.window, &nb);
            } else if let Ok(win) = parent.clone().downcast::<gtk::Window>() {
                // Closing the host window will destroy the child; clear slot first
                // so child-exited does not respawn.
                *self.slot.borrow_mut() = None;
                win.close();
                return;
            } else if let Ok(box_) = parent.downcast::<gtk::Box>() {
                box_.remove(&root);
            }
        }
        *self.slot.borrow_mut() = None;
    }

    fn close_tab(self: &Rc<Self>) {
        self.teardown_ui();
    }

    fn move_to_new_window(self: &Rc<Self>) {
        let root = self.root.clone();
        // Detach from bottom notebook / box without clearing the plugin slot.
        if let Some(parent) = root.parent() {
            if let Ok(nb) = parent.clone().downcast::<gtk::Notebook>() {
                nb.detach_tab(&root);
                maybe_hide_empty_bottom_panel(&self.window, &nb);
            } else if let Ok(box_) = parent.downcast::<gtk::Box>() {
                box_.remove(&root);
            } else {
                // Already in a free window.
                return;
            }
        }

        self.follow_editor.set(false);

        let win = gtk::Window::builder()
            .title("Terminal — GTK Edit")
            .default_width(720)
            .default_height(420)
            .child(&root)
            .build();
        win.add_css_class("gtk-content");

        {
            let state = Rc::clone(self);
            win.connect_close_request(move |_| {
                *state.slot.borrow_mut() = None;
                glib::Propagation::Proceed
            });
        }

        win.present();
    }
}

fn maybe_hide_empty_bottom_panel(editor: &gtk::ApplicationWindow, nb: &gtk::Notebook) {
    if nb.n_pages() > 0 {
        return;
    }
    if let Some(ew) = crate::window::current_from_window(editor) {
        ew.bottom_panel.set_visible_panel(false);
        ew.config.borrow_mut().ui.bottom_panel_visible = false;
        let _ = ew.config.borrow().save();
    }
}

fn install_terminal_tab_menu(tab_widget: &impl IsA<gtk::Widget>, state: &Rc<TerminalState>) {
    let group = gio::SimpleActionGroup::new();
    {
        let state = Rc::clone(state);
        let move_act = gio::SimpleAction::new("move-to-window", None);
        move_act.connect_activate(move |_, _| {
            state.move_to_new_window();
        });
        group.add_action(&move_act);
    }
    {
        let state = Rc::clone(state);
        let close_act = gio::SimpleAction::new("close", None);
        close_act.connect_activate(move |_, _| {
            state.close_tab();
        });
        group.add_action(&close_act);
    }
    tab_widget.insert_action_group("term-tab", Some(&group));

    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();
    icons.append(
        &menu,
        "Move to New Window",
        "term-tab.move-to-window",
        "window-new-symbolic",
    );
    icons.append(
        &menu,
        "Close Tab",
        "term-tab.close",
        "window-close-symbolic",
    );

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    icons.bind_popover(&popover);
    popover.set_parent(tab_widget);
    popover.set_has_arrow(false);
    {
        let popover_weak = popover.downgrade();
        tab_widget.connect_destroy(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.unparent();
            }
        });
    }

    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    {
        let popover = popover.clone();
        gesture.connect_pressed(move |gesture, _n, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
    }
    tab_widget.add_controller(gesture);
}

fn apply_vte_profile(terminal: &vte4::Terminal, profile: &gtk_theme::Profile) {
    let palette = profile.palette_rgba();
    let palette_refs: Vec<&gdk::RGBA> = palette.iter().collect();
    terminal.set_colors(
        Some(&profile.foreground_rgba()),
        Some(&profile.background_rgba()),
        &palette_refs,
    );
}

fn install_terminal_clipboard(terminal: &vte4::Terminal) {
    let group = gio::SimpleActionGroup::new();
    {
        let term = terminal.clone();
        let copy = gio::SimpleAction::new("copy", None);
        copy.connect_activate(move |_, _| {
            term.copy_clipboard_format(vte4::Format::Text);
        });
        group.add_action(&copy);
    }
    {
        let term = terminal.clone();
        let paste = gio::SimpleAction::new("paste", None);
        paste.connect_activate(move |_, _| {
            term.paste_clipboard();
        });
        group.add_action(&paste);
    }
    {
        let term = terminal.clone();
        let select_all = gio::SimpleAction::new("select-all", None);
        select_all.connect_activate(move |_, _| {
            term.select_all();
        });
        group.add_action(&select_all);
    }
    terminal.insert_action_group("term", Some(&group));

    let shortcuts = gtk::ShortcutController::new();
    shortcuts.set_scope(gtk::ShortcutScope::Local);
    for (trigger, action) in [
        ("<Control><Shift>c", "term.copy"),
        ("<Control><Shift>v", "term.paste"),
        ("<Control><Shift>a", "term.select-all"),
    ] {
        let Some(trigger) = gtk::ShortcutTrigger::parse_string(trigger) else {
            continue;
        };
        shortcuts.add_shortcut(gtk::Shortcut::new(
            Some(trigger),
            Some(gtk::NamedAction::new(action)),
        ));
    }
    terminal.add_controller(shortcuts);

    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();
    icons.append_action(&menu, "Copy", "term.copy");
    icons.append_action(&menu, "Paste", "term.paste");
    icons.append_action(&menu, "Select All", "term.select-all");
    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    icons.bind_popover(&popover);
    popover.set_parent(terminal);
    popover.set_has_arrow(false);
    {
        let popover_weak = popover.downgrade();
        terminal.connect_destroy(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.unparent();
            }
        });
    }
    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    {
        let popover = popover.clone();
        gesture.connect_pressed(move |gesture, _n, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
    }
    terminal.add_controller(gesture);
}

fn feed_cd(terminal: &vte4::Terminal, path: &Path) {
    let escaped = shell_single_quote(&path.to_string_lossy());
    // Match gtk-files: Ctrl-U clear line, cd, Ctrl-L clear screen.
    let cmd = format!("\u{15}cd {escaped}\n\u{0c}");
    terminal.feed_child(cmd.as_bytes());
}

fn shell_single_quote(s: &str) -> String {
    let mut out = String::from("'");
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
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
        "terminal",
        "Terminal",
        "Bottom-panel terminal that follows the folder of the active document.",
    );
    make_factory(i.clone(), move || {
        Box::new(TerminalPlugin {
            info: i.clone(),
            state: Rc::new(RefCell::new(None)),
        })
    })
}
