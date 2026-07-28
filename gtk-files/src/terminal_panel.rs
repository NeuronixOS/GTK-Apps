//! Bottom-panel VTE terminal that tracks the focused folder.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use vte4::prelude::*;

pub struct TerminalPanel {
    pub root: gtk::Box,
    terminal: vte4::Terminal,
    cwd: RefCell<PathBuf>,
    alive: Cell<bool>,
}

impl TerminalPanel {
    pub fn new(start_dir: &Path) -> Rc<Self> {
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
        install_terminal_clipboard(&terminal);

        let scrolled = gtk::ScrolledWindow::builder()
            .child(&terminal)
            .hexpand(true)
            .vexpand(true)
            .propagate_natural_height(false)
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

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("terminal-panel");
        root.set_hexpand(true);
        root.set_vexpand(true);
        // Keep VTE's large natural height from locking the bottom paned handle.
        root.set_size_request(-1, 80);
        root.append(&header);
        root.append(&scrolled);

        let panel = Rc::new(Self {
            root,
            terminal,
            cwd: RefCell::new(start_dir.to_path_buf()),
            alive: Cell::new(false),
        });

        {
            let weak = Rc::downgrade(&panel);
            panel.terminal.connect_child_exited(move |_term, _status| {
                let Some(panel) = weak.upgrade() else {
                    return;
                };
                panel.alive.set(false);
                // Defer respawn so VTE can finish tearing down the old PTY.
                let weak = Rc::downgrade(&panel);
                glib::idle_add_local_once(move || {
                    if let Some(panel) = weak.upgrade() {
                        panel.spawn_shell();
                    }
                });
            });
        }

        panel.spawn_shell();
        panel
    }

    /// Recolor the VTE to match a suite theme profile.
    pub fn apply_theme_profile(&self, profile: &gtk_theme::Profile) {
        apply_vte_profile(&self.terminal, profile);
    }

    /// Keep the shell in sync with the focused folder.
    pub fn sync_cwd(self: &Rc<Self>, path: &Path) {
        self.sync_cwd_inner(path, false);
    }

    /// Like [`sync_cwd`], but always feeds `cd` (e.g. when switching tabs).
    pub fn sync_cwd_force(self: &Rc<Self>, path: &Path) {
        self.sync_cwd_inner(path, true);
    }

    fn sync_cwd_inner(self: &Rc<Self>, path: &Path, force: bool) {
        if !path.is_dir() {
            return;
        }
        let path = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => path.to_path_buf(),
        };
        if !force && *self.cwd.borrow() == path && self.alive.get() {
            return;
        }
        *self.cwd.borrow_mut() = path.clone();
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
                        eprintln!("gtk-files: failed to spawn terminal shell: {err}");
                    }
                }
            },
        );
    }
}

fn apply_vte_profile(terminal: &vte4::Terminal, profile: &gtk_theme::Profile) {
    let palette = profile.palette_rgba();
    let palette_refs: Vec<&gtk::gdk::RGBA> = palette.iter().collect();
    terminal.set_colors(
        Some(&profile.foreground_rgba()),
        Some(&profile.background_rgba()),
        &palette_refs,
    );
}

/// Copy / paste / select-all for the embedded VTE (gnome-terminal style).
/// Uses a local `term` action group so file-manager Ctrl+C/V stay on the file list.
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
    // Ctrl-U clears the current input line, cd into the folder, then Ctrl-L
    // clears the visible screen (same as pressing Ctrl+L). Scrollback and
    // shell history are preserved — unlike CSI 3J / `reset(…, clear_history)`.
    let cmd = format!("\u{15}cd {escaped}\n\u{0c}");
    terminal.feed_child(cmd.as_bytes());
}

fn shell_single_quote(s: &str) -> String {
    // Safe for POSIX sh: 'foo'\''bar'
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
