//! Bottom-panel VTE terminal that tracks the focused folder.

use std::cell::{Cell, RefCell};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

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
    child_pid: Cell<Option<i32>>,
    /// After gtk-files feeds `cd`, ignore /proc cwd until the shell catches up.
    ignore_shell_until: Cell<Option<Instant>>,
    /// True while applying a shell-driven cwd so `on_location` must not feed `cd`.
    from_shell_nav: Cell<bool>,
    on_cwd_from_shell: RefCell<Option<Rc<dyn Fn(PathBuf)>>>,
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
        // Git uses bold green/red; without this, bold is only a heavier glyph.
        terminal.set_bold_is_bright(true);

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
            child_pid: Cell::new(None),
            ignore_shell_until: Cell::new(None),
            from_shell_nav: Cell::new(false),
            on_cwd_from_shell: RefCell::new(None),
        });

        {
            let weak = Rc::downgrade(&panel);
            panel.terminal.connect_child_exited(move |_term, _status| {
                let Some(panel) = weak.upgrade() else {
                    return;
                };
                panel.alive.set(false);
                panel.child_pid.set(None);
                // Defer respawn so VTE can finish tearing down the old PTY.
                let weak = Rc::downgrade(&panel);
                glib::idle_add_local_once(move || {
                    if let Some(panel) = weak.upgrade() {
                        panel.spawn_shell();
                    }
                });
            });
        }

        {
            let weak = Rc::downgrade(&panel);
            panel
                .terminal
                .connect_current_directory_uri_changed(move |term| {
                    let Some(panel) = weak.upgrade() else {
                        return;
                    };
                    let Some(uri) = term.current_directory_uri() else {
                        return;
                    };
                    if let Some(path) = path_from_cwd_uri(uri.as_str()) {
                        panel.consider_shell_cwd(path);
                    }
                });
        }

        {
            let weak = Rc::downgrade(&panel);
            glib::timeout_add_local(Duration::from_millis(300), move || {
                let Some(panel) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                panel.poll_shell_cwd();
                glib::ControlFlow::Continue
            });
        }

        panel.spawn_shell();
        panel
    }

    /// When the shell `cd`s, gtk-files should follow (without feeding another `cd`).
    pub fn set_on_cwd_from_shell<F: Fn(PathBuf) + 'static>(&self, f: F) {
        *self.on_cwd_from_shell.borrow_mut() = Some(Rc::new(f));
    }

    /// Recolor the VTE to match a suite theme profile.
    pub fn apply_theme_profile(&self, profile: &gtk_theme::Profile) {
        apply_vte_profile(&self.terminal, profile);
    }

    /// Keep the shell in sync with the focused folder.
    pub fn sync_cwd(self: &Rc<Self>, path: &Path) {
        self.sync_cwd_inner(path, false);
    }

    /// Clear the visible terminal screen (same as typing Ctrl+L in the shell).
    pub fn clear_screen(&self) {
        // Home + erase display + erase scrollback. Do not call VTE reset():
        // that drops 256-color / SGR state, so `git status` / `git diff` lose
        // green (added) / red (modified) coloring until a new shell.
        self.terminal.feed(b"\x1b[H\x1b[2J\x1b[3J");
        if self.alive.get() {
            self.terminal.feed_child(b"\x0c");
        }
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
        if self.from_shell_nav.get() {
            *self.cwd.borrow_mut() = path;
            return;
        }
        if !force && *self.cwd.borrow() == path && self.alive.get() {
            return;
        }
        *self.cwd.borrow_mut() = path.clone();
        if self.alive.get() {
            self.ignore_shell_until
                .set(Some(Instant::now() + Duration::from_millis(500)));
            feed_cd(&self.terminal, &path);
            // After the shell consumes `cd`, wipe the panel the same way Ctrl+L
            // does so prior `git status` / command output does not linger.
            let panel = Rc::clone(self);
            glib::timeout_add_local_once(Duration::from_millis(60), move || {
                panel.clear_screen();
            });
        } else {
            self.spawn_shell();
        }
    }

    fn poll_shell_cwd(&self) {
        // Prefer the shell-written cwd file (independent of VTE OSC 7 / PTY pgid).
        if let Some(path) = reported_cwd() {
            self.consider_shell_cwd(path);
        }
        if let Some(path) = self.child_pid.get().and_then(proc_cwd) {
            self.consider_shell_cwd(path);
        }
        if let Some(path) = self.foreground_cwd() {
            self.consider_shell_cwd(path);
        }
    }

    fn foreground_cwd(&self) -> Option<PathBuf> {
        let pty = self.terminal.pty()?;
        let pgid = unsafe { libc::tcgetpgrp(pty.fd().as_raw_fd()) };
        if pgid > 0 {
            proc_cwd(pgid)
        } else {
            None
        }
    }

    fn consider_shell_cwd(&self, path: PathBuf) {
        if self
            .ignore_shell_until
            .get()
            .is_some_and(|until| Instant::now() < until)
        {
            return;
        }
        if !path.is_dir() {
            return;
        }
        let path = path.canonicalize().unwrap_or(path);
        if *self.cwd.borrow() == path {
            return;
        }
        *self.cwd.borrow_mut() = path.clone();
        // Clone the Rc so navigate (which may re-enter the terminal) cannot
        // panic on a live RefCell borrow.
        let cb = self.on_cwd_from_shell.borrow().clone();
        if let Some(cb) = cb {
            self.from_shell_nav.set(true);
            cb(path);
            self.from_shell_nav.set(false);
        }
    }

    fn spawn_shell(self: &Rc<Self>) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let dir = self.cwd.borrow().to_string_lossy().into_owned();
        // zsh: ignore leading-space commands so folder-nav ` cd` stays out of history.
        // bash/others: HISTCONTROL=ignorespace via the inherited+patched env below.
        let is_zsh = shell.rsplit('/').next().is_some_and(|n| n == "zsh");
        let bash_rc = write_bash_hook();
        let zsh_zdot = write_zsh_zdotdir();
        let argv: Vec<String> = if is_zsh {
            vec![
                shell,
                "-o".into(),
                "HIST_IGNORE_SPACE".into(),
                "-i".into(),
            ]
        } else if let Some(ref rc) = bash_rc {
            vec![
                shell,
                "--rcfile".into(),
                rc.to_string_lossy().into_owned(),
                "-i".into(),
            ]
        } else {
            vec![shell, "-i".into()]
        };
        let argv_refs: Vec<&str> = argv.iter().map(String::as_str).collect();

        let mut env_owned: Vec<String> = std::env::vars()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        patch_histcontrol(&mut env_owned);
        // VTE sets TERM to xterm-256color unless envv overrides it. Copying the
        // GUI process environment (Hyprland / .desktop) often passes TERM=linux
        // or TERM=dumb, and git then prints `status`/`diff` with no color.
        ensure_vte_color_env(&mut env_owned);
        if is_zsh {
            if let Some(ref zdot) = zsh_zdot {
                upsert_env(&mut env_owned, "ZDOTDIR", &zdot.to_string_lossy());
            }
        }
        let env_refs: Vec<&str> = env_owned.iter().map(String::as_str).collect();
        let _ = std::fs::write(runtime_dir().join("cwd"), format!("{dir}\n"));

        let weak = Rc::downgrade(self);

        self.terminal.spawn_async(
            vte4::PtyFlags::DEFAULT,
            Some(dir.as_str()),
            &argv_refs,
            &env_refs,
            glib::SpawnFlags::DEFAULT,
            || {},
            -1,
            gio::Cancellable::NONE,
            move |result| {
                let Some(panel) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(pid) => {
                        panel.alive.set(true);
                        panel.child_pid.set(Some(pid.0));
                        panel.terminal.watch_child(pid);
                    }
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

fn patch_histcontrol(env: &mut Vec<String>) {
    const KEY: &str = "HISTCONTROL=";
    if let Some(entry) = env.iter_mut().find(|e| e.starts_with(KEY)) {
        let val = &entry[KEY.len()..];
        if !val.split(':').any(|p| p == "ignorespace" || p == "ignoreboth") {
            *entry = if val.is_empty() {
                format!("{KEY}ignorespace")
            } else {
                format!("{entry}:ignorespace")
            };
        }
    } else {
        env.push(format!("{KEY}ignorespace"));
    }
}

fn proc_cwd(pid: i32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

/// OSC 7 `file://host/path` is not always a GLib "native" URI; strip the host.
fn path_from_cwd_uri(uri: &str) -> Option<PathBuf> {
    let file = gio::File::for_uri(uri);
    if let Some(path) = file.path() {
        return Some(path);
    }
    let rest = uri.strip_prefix("file://")?;
    let path_part = if rest.starts_with('/') {
        rest
    } else {
        rest.find('/').map(|i| &rest[i..])?
    };
    Some(PathBuf::from(path_part))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("gtk-files")
}

fn reported_cwd() -> Option<PathBuf> {
    let text = std::fs::read_to_string(runtime_dir().join("cwd")).ok()?;
    let path = PathBuf::from(text.trim());
    path.is_dir().then_some(path)
}

fn write_bash_hook() -> Option<PathBuf> {
    let dir = runtime_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("gtk-files: hook dir {dir:?}: {err}");
        return None;
    }
    let path = dir.join("bashrc");
    // v4: source bashrc with TERM=linux so it does not wrap PS1 in OSC 0
    // (that sequence runs *after* PROMPT_COMMAND and VTE drops OSC 7 cwd).
    // Restore xterm-256color for git/ls, write PWD to a file gtk-files polls,
    // and emit OSC 7 to /dev/tty.
    let body = r#"# gtk-files cwd-hook v4
export COLORTERM="${COLORTERM:-truecolor}"
_gtk_files_term="${TERM:-xterm-256color}"
case "$_gtk_files_term" in
  dumb|linux|unknown) _gtk_files_term=xterm-256color ;;
esac
# Source user rc without an xterm TERM so Debian bashrc skips OSC 0 titles.
TERM=linux
if [ -r "$HOME/.bashrc" ]; then
  . "$HOME/.bashrc"
elif [ -r /etc/bash.bashrc ]; then
  . /etc/bash.bashrc
fi
export TERM="$_gtk_files_term"
unset _gtk_files_term
if [[ "$PS1" != *$'\033['* && "$PS1" != *'033['* ]]; then
  PS1='\[\033[01;32m\]\u@\h\[\033[00m\]:\[\033[01;34m\]\w\[\033[00m\]\$ '
fi
__gtk_files_osc7() {
  mkdir -p "${XDG_RUNTIME_DIR:-/tmp}/gtk-files"
  printf '%s\n' "${PWD:-/}" > "${XDG_RUNTIME_DIR:-/tmp}/gtk-files/cwd"
  printf '\033]7;file://%s\033\\' "${PWD:-/}" >/dev/tty 2>/dev/null || true
}
if declare -p PROMPT_COMMAND >/dev/null 2>&1 && [[ $(declare -p PROMPT_COMMAND) == "declare -a"* ]]; then
  case " ${PROMPT_COMMAND[*]} " in
    *" __gtk_files_osc7 "*) ;;
    *) PROMPT_COMMAND+=(__gtk_files_osc7) ;;
  esac
else
  case ";${PROMPT_COMMAND:-};" in
    *__gtk_files_osc7*) ;;
    *) PROMPT_COMMAND="${PROMPT_COMMAND:+$PROMPT_COMMAND; }__gtk_files_osc7" ;;
  esac
fi
__gtk_files_osc7
"#;
    if let Err(err) = std::fs::write(&path, body) {
        eprintln!("gtk-files: hook write {path:?}: {err}");
        return None;
    }
    Some(path)
}

fn write_zsh_zdotdir() -> Option<PathBuf> {
    let dir = runtime_dir().join("zsh");
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("gtk-files: zsh hook dir {dir:?}: {err}");
        return None;
    }
    let path = dir.join(".zshrc");
    let body = r#"# gtk-files cwd-hook v4
export COLORTERM="${COLORTERM:-truecolor}"
_gtk_files_term="${TERM:-xterm-256color}"
case "$_gtk_files_term" in
  dumb|linux|unknown) _gtk_files_term=xterm-256color ;;
esac
TERM=linux
[ -r "$HOME/.zshrc" ] && . "$HOME/.zshrc"
export TERM="$_gtk_files_term"
unset _gtk_files_term
__gtk_files_osc7() {
  mkdir -p "${XDG_RUNTIME_DIR:-/tmp}/gtk-files"
  printf '%s\n' "${PWD:-/}" > "${XDG_RUNTIME_DIR:-/tmp}/gtk-files/cwd"
  printf '\033]7;file://%s\033\\' "${PWD:-/}" >/dev/tty 2>/dev/null || true
}
typeset -ga precmd_functions
case " ${precmd_functions[*]} " in
  *" __gtk_files_osc7 "*) ;;
  *) precmd_functions+=(__gtk_files_osc7) ;;
esac
__gtk_files_osc7
"#;
    if let Err(err) = std::fs::write(&path, body) {
        eprintln!("gtk-files: zsh hook write {path:?}: {err}");
        return None;
    }
    Some(dir)
}

fn upsert_env(env: &mut Vec<String>, key: &str, value: &str) {
    let prefix = format!("{key}=");
    if let Some(entry) = env.iter_mut().find(|e| e.starts_with(&prefix)) {
        *entry = format!("{prefix}{value}");
    } else {
        env.push(format!("{prefix}{value}"));
    }
}

/// Make the VTE child look like a real color terminal to git/ls/grep.
fn ensure_vte_color_env(env: &mut Vec<String>) {
    upsert_env(env, "TERM", "xterm-256color");
    upsert_env(env, "COLORTERM", "truecolor");
    // Launchers sometimes set this; git 2.41+ then disables color entirely.
    env.retain(|e| !e.eq_ignore_ascii_case("NO_COLOR") && !e.starts_with("NO_COLOR="));
    // `git diff` / `git log` page through less; without -R, ANSI is stripped.
    const LESS_KEY: &str = "LESS=";
    if let Some(entry) = env.iter_mut().find(|e| e.starts_with(LESS_KEY)) {
        let val = &entry[LESS_KEY.len()..];
        if !val.split(|c: char| c == ' ' || c == '-').any(|p| p.contains('R')) {
            *entry = if val.is_empty() {
                format!("{LESS_KEY}-FRX")
            } else {
                format!("{entry}R")
            };
        }
    }
}

fn feed_cd(terminal: &vte4::Terminal, path: &Path) {
    terminal.feed_child(history_safe_cd(path).as_bytes());
}

/// Ctrl-U, then a cd that does not stay in shell history.
fn history_safe_cd(path: &Path) -> String {
    let escaped = shell_single_quote(&path.to_string_lossy());
    let shell = std::env::var("SHELL").unwrap_or_default();
    let name = Path::new(&shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("bash");
    // Ctrl-U clears typed input. Screen wipe happens in `clear_screen` after cd.
    match name {
        "zsh" => format!("\u{15} cd -- {escaped}\n"),
        "fish" => format!("\u{15} cd {escaped}\n"),
        // bash: record-then-delete so it works even if .bashrc reset HISTCONTROL.
        _ => format!(
            "\u{15} builtin cd -- {escaped} && {{ history -d $(history 1) 2>/dev/null || true; }}\n"
        ),
    }
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
