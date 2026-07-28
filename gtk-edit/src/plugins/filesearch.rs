//! File Search — search for text across files in a directory
//! (simplified Rust port of the gedit file-search plugin).

use std::cell::RefCell;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use gtk4 as gtk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use regex::{Regex, RegexBuilder};

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

struct FileSearchPlugin {
    info: PluginInfo,
    root: RefCell<Option<gtk::Box>>,
    results: RefCell<Option<gtk::ListBox>>,
    status: RefCell<Option<gtk::Label>>,
    window: RefCell<Option<gtk::ApplicationWindow>>,
}

impl Plugin for FileSearchPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for FileSearchPlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        *self.window.borrow_mut() = Some(ctx.window.clone());

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.set_margin_start(8);
        header.set_margin_end(8);
        header.set_margin_top(4);
        header.set_margin_bottom(2);
        let title = gtk::Label::new(Some("File Search"));
        title.add_css_class("heading");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        let status = gtk::Label::new(Some("Idle"));
        status.add_css_class("dim-label");
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        header.append(&title);
        header.append(&status);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("gtk-content");
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hexpand(true)
            .min_content_height(120)
            .build();
        scroll.add_css_class("gtk-content");

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outer.set_vexpand(true);
        outer.append(&header);
        outer.append(&scroll);

        if let Some(nb) = find_bottom_notebook(&ctx.bottom_panel) {
            nb.append_page(&outer, Some(&gtk::Label::new(Some("File Search"))));
        } else {
            ctx.bottom_panel.append(&outer);
        }
        ctx.bottom_panel.set_visible(true);

        *self.root.borrow_mut() = Some(outer);
        *self.results.borrow_mut() = Some(list.clone());
        *self.status.borrow_mut() = Some(status);

        {
            let win = ctx.window.clone();
            list.connect_row_activated(move |_, row| {
                let path = unsafe {
                    row.data::<PathBuf>("path")
                        .map(|p| p.as_ref().clone())
                };
                let line = unsafe {
                    row.data::<u32>("line")
                        .map(|p| *p.as_ref())
                        .unwrap_or(1)
                };
                if let Some(path) = path {
                    crate::window::open_path_in_window(&win, &path);
                    if let Some(tab) = crate::window::current_tab_from_window(&win) {
                        jump_to_line(&tab.document.buffer, &tab.view, line);
                    }
                }
            });
        }

        let results = self.results.clone();
        let status_lbl = self.status.clone();
        let win = ctx.window.clone();
        let action = gio::SimpleAction::new("file-search", None);
        action.connect_activate(move |_, _| {
            show_search_dialog(
                &win,
                results.borrow().clone(),
                status_lbl.borrow().clone(),
            );
        });
        ctx.window.add_action(&action);
        ctx.menu_icons.borrow_mut().append(
            &ctx.search_menu,
            "Search Files…",
            "win.file-search",
            "edit-find-symbolic",
        );

        if let Some(app) = ctx.window.application() {
            app.set_accels_for_action("win.file-search", &["<Primary><Shift>f"]);
        }
    }

    fn deactivate(&mut self) {
        if let Some(root) = self.root.borrow_mut().take() {
            if let Some(parent) = root.parent() {
                if let Some(nb) = parent.downcast_ref::<gtk::Notebook>() {
                    nb.detach_tab(&root);
                }
            }
        }
        *self.results.borrow_mut() = None;
        *self.status.borrow_mut() = None;
        *self.window.borrow_mut() = None;
    }
}

#[derive(Clone)]
struct SearchOpts {
    query: String,
    directory: PathBuf,
    case_sensitive: bool,
    whole_word: bool,
    is_regex: bool,
    include_subfolders: bool,
    file_globs: Vec<String>,
}

#[derive(Clone)]
struct MatchHit {
    path: PathBuf,
    line: u32,
    text: String,
}

fn show_search_dialog(
    win: &gtk::ApplicationWindow,
    results: Option<gtk::ListBox>,
    status: Option<gtk::Label>,
) {
    let Some(results) = results else { return };
    let Some(status) = status else { return };

    let default_dir = crate::window::current_tab_from_window(win)
        .and_then(|t| t.document.path())
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")));

    let dialog = gtk::Window::builder()
        .title("Search Files")
        .transient_for(win)
        .modal(true)
        .default_width(520)
        .default_height(360)
        .build();
    gtk_theme::style_dialog(&dialog);

    let query = gtk::Entry::new();
    query.set_placeholder_text(Some("Search text"));
    query.set_hexpand(true);

    let dir_entry = gtk::Entry::new();
    dir_entry.set_text(&default_dir.display().to_string());
    dir_entry.set_hexpand(true);
    let browse = gtk_theme::labeled_button(gtk_theme::icon_for_label("Browse…"), "Browse…");

    let types = gtk::Entry::new();
    types.set_text("*");
    types.set_placeholder_text(Some("File globs, e.g. *.rs *.toml"));

    let case_sensitive = gtk::CheckButton::with_label("Case sensitive");
    let whole_word = gtk::CheckButton::with_label("Whole word");
    let is_regex = gtk::CheckButton::with_label("Regular expression");
    let include_sub = gtk::CheckButton::with_label("Include subfolders");
    include_sub.set_active(true);

    let form = gtk::Box::new(gtk::Orientation::Vertical, 8);
    form.set_margin_top(12);
    form.set_margin_bottom(12);
    form.set_margin_start(12);
    form.set_margin_end(12);

    form.append(&gtk::Label::new(Some("Search for:")));
    form.append(&query);

    let dir_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    dir_row.append(&dir_entry);
    dir_row.append(&browse);
    form.append(&gtk::Label::new(Some("Directory:")));
    form.append(&dir_row);

    form.append(&gtk::Label::new(Some("File types:")));
    form.append(&types);
    form.append(&case_sensitive);
    form.append(&whole_word);
    form.append(&is_regex);
    form.append(&include_sub);

    let search_btn =
        gtk_theme::labeled_button(gtk_theme::icon_for_label("Search"), "Search");
    search_btn.add_css_class("suggested-action");
    let cancel = gtk_theme::labeled_button(gtk_theme::icon_for_label("Cancel"), "Cancel");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&cancel);
    buttons.append(&search_btn);
    form.append(&buttons);
    dialog.set_child(Some(&form));

    {
        let d = dialog.clone();
        cancel.connect_clicked(move |_| d.close());
    }
    {
        let parent = dialog.clone();
        let dir_entry = dir_entry.clone();
        browse.connect_clicked(move |_| {
            let dir_entry = dir_entry.clone();
            let start = gio::File::for_path(dir_entry.text().as_str());
            gtk_theme::present_file_chooser_at(
                Some(&parent),
                "Search Directory",
                gtk::FileChooserAction::SelectFolder,
                "Select",
                None,
                None,
                Some(&start),
                move |file| {
                    if let Some(path) = file.and_then(|f| f.path()) {
                        dir_entry.set_text(&path.display().to_string());
                    }
                },
            );
        });
    }
    {
        let d = dialog.clone();
        let win = win.clone();
        let results = results.clone();
        let status = status.clone();
        let query = query.clone();
        search_btn.connect_clicked(move |_| {
            let q = query.text().to_string();
            if q.is_empty() {
                return;
            }
            let opts = SearchOpts {
                query: q,
                directory: PathBuf::from(dir_entry.text().as_str()),
                case_sensitive: case_sensitive.is_active(),
                whole_word: whole_word.is_active(),
                is_regex: is_regex.is_active(),
                include_subfolders: include_sub.is_active(),
                file_globs: types
                    .text()
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
            };
            d.close();
            start_search(&win, &results, &status, opts);
        });
    }

    dialog.present();
    query.grab_focus();
}

fn start_search(
    win: &gtk::ApplicationWindow,
    results: &gtk::ListBox,
    status: &gtk::Label,
    opts: SearchOpts,
) {
    while let Some(child) = results.first_child() {
        results.remove(&child);
    }
    status.set_text("Searching…");
    reveal_file_search_panel(win);

    let root = opts.directory.clone();
    let (tx, rx) = mpsc::channel::<Result<Vec<MatchHit>, String>>();
    let opts_bg = opts.clone();
    thread::spawn(move || {
        let _ = tx.send(run_search(&opts_bg));
    });

    let results = results.clone();
    let status = status.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
        match rx.try_recv() {
            Ok(Ok(hits)) => {
                status.set_text(&format!("{} matches", hits.len()));
                for hit in hits {
                    let display = hit
                        .path
                        .strip_prefix(&root)
                        .unwrap_or(&hit.path)
                        .display()
                        .to_string();
                    let row = gtk::ListBoxRow::new();
                    let label = gtk::Label::new(Some(&format!(
                        "{}:{}  {}",
                        display,
                        hit.line,
                        hit.text.trim()
                    )));
                    label.set_xalign(0.0);
                    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    label.set_margin_start(8);
                    label.set_margin_end(8);
                    label.set_margin_top(3);
                    label.set_margin_bottom(3);
                    row.set_child(Some(&label));
                    unsafe {
                        row.set_data("path", hit.path);
                        row.set_data("line", hit.line);
                    }
                    results.append(&row);
                }
                glib::ControlFlow::Break
            }
            Ok(Err(e)) => {
                status.set_text(&format!("Error: {e}"));
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                status.set_text("Search finished with no response");
                glib::ControlFlow::Break
            }
        }
    });
}

fn reveal_file_search_panel(win: &gtk::ApplicationWindow) {
    let Some(ew) = crate::window::current_from_window(win) else {
        return;
    };
    ew.bottom_panel.set_visible_panel(true);
    ew.config.borrow_mut().ui.bottom_panel_visible = true;
    ew.ensure_bottom_panel_height();
    ew.bottom_panel.restore_page_id("filesearch");
}

fn run_search(opts: &SearchOpts) -> Result<Vec<MatchHit>, String> {
    if !opts.directory.is_dir() {
        return Err(format!("Not a directory: {}", opts.directory.display()));
    }

    // Prefer ripgrep when available; fall back to a Rust walker.
    if which_rg() {
        match run_search_rg(opts) {
            Ok(hits) => return Ok(hits),
            Err(_) => { /* fall through */ }
        }
    }
    run_search_walk(opts)
}

fn which_rg() -> bool {
    Command::new("rg")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_search_rg(opts: &SearchOpts) -> Result<Vec<MatchHit>, String> {
    let mut cmd = Command::new("rg");
    cmd.arg("--line-number")
        .arg("--no-heading")
        .arg("--color=never")
        .arg("--max-count")
        .arg("2000");
    if !opts.case_sensitive {
        cmd.arg("-i");
    }
    if opts.whole_word {
        cmd.arg("-w");
    }
    if !opts.is_regex {
        cmd.arg("-F");
    }
    if !opts.include_subfolders {
        cmd.arg("--max-depth").arg("1");
    }
    for g in &opts.file_globs {
        if g != "*" {
            cmd.arg("-g").arg(g);
        }
    }
    cmd.arg("--").arg(&opts.query).arg(&opts.directory);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());

    let output = cmd.output().map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        // path:line:text  (path may contain :)
        let Some((path_line, text)) = split_rg_line(line) else {
            continue;
        };
        let Some((path, line_no)) = path_line.rsplit_once(':') else {
            continue;
        };
        let Ok(line) = line_no.parse::<u32>() else {
            continue;
        };
        hits.push(MatchHit {
            path: PathBuf::from(path),
            line,
            text: text.to_string(),
        });
        if hits.len() >= 2000 {
            break;
        }
    }
    Ok(hits)
}

fn split_rg_line(line: &str) -> Option<(&str, &str)> {
    // Find the second colon from a pattern path:lineno:text
    let mut idx = None;
    let mut colons = 0u8;
    for (i, ch) in line.char_indices() {
        if ch == ':' {
            colons += 1;
            if colons == 2 {
                idx = Some(i);
                break;
            }
        }
    }
    let i = idx?;
    Some((&line[..i], &line[i + 1..]))
}

fn run_search_walk(opts: &SearchOpts) -> Result<Vec<MatchHit>, String> {
    let pattern = build_regex(opts)?;
    let mut hits = Vec::new();
    let mut files_seen = 0usize;
    walk_dir(
        &opts.directory,
        opts,
        &pattern,
        opts.include_subfolders,
        0,
        &mut hits,
        &mut files_seen,
    )?;
    if hits.is_empty() && files_seen == 0 {
        return Err(format!(
            "No readable files under {}",
            opts.directory.display()
        ));
    }
    Ok(hits)
}

fn build_regex(opts: &SearchOpts) -> Result<Regex, String> {
    let mut pat = if opts.is_regex {
        opts.query.clone()
    } else {
        regex::escape(&opts.query)
    };
    if opts.whole_word {
        pat = format!(r"\b{pat}\b");
    }
    RegexBuilder::new(&pat)
        .case_insensitive(!opts.case_sensitive)
        .build()
        .map_err(|e| e.to_string())
}

const MAX_HITS: usize = 2000;
const MAX_FILES: usize = 50_000;
const MAX_DEPTH: usize = 32;

fn walk_dir(
    dir: &Path,
    opts: &SearchOpts,
    pattern: &Regex,
    recurse: bool,
    depth: usize,
    hits: &mut Vec<MatchHit>,
    files_seen: &mut usize,
) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        if hits.len() >= MAX_HITS || *files_seen >= MAX_FILES {
            break;
        }
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.starts_with('.') || is_vcs_dir(name) {
            continue;
        }
        if path.is_dir() {
            if recurse && depth < MAX_DEPTH {
                walk_dir(&path, opts, pattern, true, depth + 1, hits, files_seen)?;
            }
            continue;
        }
        if !matches_globs(name, &opts.file_globs) {
            continue;
        }
        if !looks_like_text_file(&path) {
            continue;
        }
        *files_seen += 1;
        let Ok(file) = fs::File::open(&path) else {
            continue;
        };
        let reader = BufReader::new(file);
        for (i, line) in reader.lines().enumerate() {
            let Ok(line) = line else {
                continue;
            };
            if pattern.is_match(&line) {
                hits.push(MatchHit {
                    path: path.clone(),
                    line: (i + 1) as u32,
                    text: truncate_line(&line),
                });
                if hits.len() >= MAX_HITS {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn looks_like_text_file(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    let mut buf = [0u8; 8192];
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    if n == 0 {
        return true;
    }
    // Skip obvious binaries.
    if buf[..n].contains(&0) {
        return false;
    }
    true
}

fn truncate_line(line: &str) -> String {
    const MAX: usize = 240;
    if line.chars().count() <= MAX {
        line.to_string()
    } else {
        let s: String = line.chars().take(MAX).collect();
        format!("{s}…")
    }
}

fn is_vcs_dir(name: &str) -> bool {
    matches!(name, ".git" | ".hg" | ".svn" | ".bzr" | "CVS")
}

fn matches_globs(name: &str, globs: &[String]) -> bool {
    if globs.is_empty() || globs.iter().any(|g| g == "*") {
        return true;
    }
    globs.iter().any(|g| glob_match(g, name))
}

fn glob_match(pattern: &str, name: &str) -> bool {
    // Minimal * glob matcher.
    let pat = pattern.as_bytes();
    let text = name.as_bytes();
    let mut pi = 0usize;
    let mut ti = 0usize;
    let mut star = None::<(usize, usize)>;
    while ti < text.len() {
        if pi < pat.len() && (pat[pi] == text[ti] || pat[pi] == b'?') {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == b'*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, ti));
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }
    pi == pat.len()
}

fn jump_to_line(buffer: &sourceview5::Buffer, view: &sourceview5::View, line: u32) {
    let line = line.saturating_sub(1) as i32;
    if let Some(mut iter) = buffer.iter_at_line(line) {
        buffer.place_cursor(&iter);
        view.scroll_to_iter(&mut iter, 0.25, true, 0.0, 0.3);
        view.grab_focus();
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
        "filesearch",
        "File Search",
        "Search for text across files in a directory (Ctrl+Shift+F).",
    );
    make_factory(i.clone(), move || {
        Box::new(FileSearchPlugin {
            info: i.clone(),
            root: RefCell::new(None),
            results: RefCell::new(None),
            status: RefCell::new(None),
            window: RefCell::new(None),
        })
    })
}
