//! Find in Files — search file contents under the current tab’s folder.

use std::cell::RefCell;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use regex::{Regex, RegexBuilder};

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

/// Dockable Find in Files panel for the bottom tools notebook.
pub struct FindInFilesPanel {
    pub root: gtk::Box,
    query: gtk::Entry,
    dir_entry: gtk::Entry,
}

impl FindInFilesPanel {
    pub fn new(
        parent: &gtk::ApplicationWindow,
        on_reveal: Rc<dyn Fn(PathBuf)>,
        on_open_folder: Rc<dyn Fn(PathBuf)>,
    ) -> Rc<Self> {
        let query = gtk::Entry::new();
        query.set_placeholder_text(Some("Search text or regular expression"));
        query.set_hexpand(true);

        let dir_entry = gtk::Entry::new();
        dir_entry.set_hexpand(true);
        let browse =
            gtk_theme::labeled_button(gtk_theme::icon_for_label("Browse"), "Browse…");

        let types = gtk::Entry::new();
        types.set_text("*");
        types.set_placeholder_text(Some("File globs, e.g. *.rs *.toml"));

        let case_sensitive = gtk::CheckButton::with_label("Case sensitive");
        let whole_word = gtk::CheckButton::with_label("Whole word");
        let is_regex = gtk::CheckButton::with_label("Regular expression");
        let include_sub = gtk::CheckButton::with_label("Include subfolders");
        include_sub.set_active(true);

        let form = gtk::Box::new(gtk::Orientation::Vertical, 6);
        form.set_margin_top(8);
        form.set_margin_bottom(4);
        form.set_margin_start(10);
        form.set_margin_end(10);

        form.append(&labeled("Search for:", &query));
        let dir_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        dir_row.append(&dir_entry);
        dir_row.append(&browse);
        form.append(&labeled("Directory:", &dir_row));
        form.append(&labeled("File types:", &types));

        let opts_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        opts_row.append(&case_sensitive);
        opts_row.append(&whole_word);
        opts_row.append(&is_regex);
        opts_row.append(&include_sub);
        form.append(&opts_row);

        let search_btn =
            gtk_theme::labeled_button(gtk_theme::icon_for_label("Search"), "Search");
        search_btn.add_css_class("suggested-action");
        let open_btn =
            gtk_theme::labeled_button(gtk_theme::icon_for_label("Open"), "Open File");
        open_btn.set_sensitive(false);
        open_btn.set_tooltip_text(Some("Open the selected match file with its default app"));
        let reveal_btn = gtk_theme::labeled_button(
            gtk_theme::icon_for_label("Show in Folder"),
            "Show in Folder",
        );
        reveal_btn.set_sensitive(false);
        reveal_btn.set_tooltip_text(Some("Select the match file in the main file view"));
        let open_folder_btn = gtk_theme::labeled_button(
            gtk_theme::icon_for_label("Open Folder"),
            "Open Folder",
        );
        open_folder_btn.set_sensitive(false);
        open_folder_btn
            .set_tooltip_text(Some("Open the folder that contains the selected match"));
        let btn_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        btn_row.set_halign(gtk::Align::End);
        btn_row.append(&open_btn);
        btn_row.append(&reveal_btn);
        btn_row.append(&open_folder_btn);
        btn_row.append(&search_btn);
        form.append(&btn_row);

        let status = gtk::Label::new(Some("Ready — searches the current tab folder by default."));
        status.set_xalign(0.0);
        status.add_css_class("dim-label");
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        form.append(&status);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("gtk-content");
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hexpand(true)
            .min_content_height(40)
            .propagate_natural_height(false)
            .build();
        scroll.add_css_class("gtk-content");

        // Keep the search form from forcing a huge paned minimum height.
        let form_scroll = gtk::ScrolledWindow::builder()
            .child(&form)
            .hexpand(true)
            .vexpand(false)
            .propagate_natural_height(true)
            .max_content_height(160)
            .build();
        form_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.set_size_request(-1, 80);
        root.append(&form_scroll);
        root.append(&scroll);

        {
            let parent = parent.clone();
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

        let selected_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));

        let set_selection_sensitive: Rc<dyn Fn(bool)> = {
            let open_btn = open_btn.clone();
            let reveal_btn = reveal_btn.clone();
            let open_folder_btn = open_folder_btn.clone();
            Rc::new(move |has: bool| {
                open_btn.set_sensitive(has);
                reveal_btn.set_sensitive(has);
                open_folder_btn.set_sensitive(has);
            })
        };

        {
            let selected_path = Rc::clone(&selected_path);
            let set_selection_sensitive = Rc::clone(&set_selection_sensitive);
            list.connect_row_selected(move |_, row| {
                let path = row.and_then(|r| unsafe {
                    r.data::<PathBuf>("path").map(|p| p.as_ref().clone())
                });
                set_selection_sensitive(path.is_some());
                *selected_path.borrow_mut() = path;
            });
        }

        {
            let on_reveal = Rc::clone(&on_reveal);
            let selected_path = Rc::clone(&selected_path);
            let set_selection_sensitive = Rc::clone(&set_selection_sensitive);
            list.connect_row_activated(move |_, row| {
                let path = unsafe { row.data::<PathBuf>("path").map(|p| p.as_ref().clone()) };
                if let Some(path) = path {
                    *selected_path.borrow_mut() = Some(path.clone());
                    set_selection_sensitive(true);
                    on_reveal(path);
                }
            });
        }

        {
            let on_reveal = Rc::clone(&on_reveal);
            let selected_path = Rc::clone(&selected_path);
            reveal_btn.connect_clicked(move |_| {
                if let Some(path) = selected_path.borrow().clone() {
                    on_reveal(path);
                }
            });
        }
        {
            let parent = parent.clone();
            let selected_path = Rc::clone(&selected_path);
            open_btn.connect_clicked(move |_| {
                if let Some(path) = selected_path.borrow().clone() {
                    crate::util::open_file_default(Some(&parent), &gio::File::for_path(&path));
                }
            });
        }
        {
            let on_open_folder = Rc::clone(&on_open_folder);
            let selected_path = Rc::clone(&selected_path);
            open_folder_btn.connect_clicked(move |_| {
                if let Some(path) = selected_path.borrow().clone() {
                    on_open_folder(containing_folder(&path));
                }
            });
        }

        install_result_context_menu(
            &list,
            parent,
            Rc::clone(&selected_path),
            Rc::clone(&on_reveal),
            Rc::clone(&on_open_folder),
            Rc::clone(&set_selection_sensitive),
        );

        {
            let list = list.clone();
            let status = status.clone();
            let query_for_btn = query.clone();
            let dir_entry = dir_entry.clone();
            let types = types.clone();
            let case_sensitive = case_sensitive.clone();
            let whole_word = whole_word.clone();
            let is_regex = is_regex.clone();
            let include_sub = include_sub.clone();
            let run = Rc::new(move || {
                let q = query_for_btn.text().to_string();
                if q.trim().is_empty() {
                    status.set_text("Enter a search term.");
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
                start_search(&list, &status, opts);
            });

            let run_click = Rc::clone(&run);
            search_btn.connect_clicked(move |_| run_click());
            query.connect_activate(move |_| run());
        }

        Rc::new(Self {
            root,
            query,
            dir_entry,
        })
    }

    pub fn set_directory(&self, directory: &Path) {
        self.dir_entry
            .set_text(&directory.display().to_string());
    }

    pub fn focus_search(&self) {
        self.query.grab_focus();
    }
}

fn containing_folder(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| path.to_path_buf())
    }
}

fn install_result_context_menu(
    list: &gtk::ListBox,
    parent: &impl IsA<gtk::Window>,
    selected_path: Rc<RefCell<Option<PathBuf>>>,
    on_reveal: Rc<dyn Fn(PathBuf)>,
    on_open_folder: Rc<dyn Fn(PathBuf)>,
    set_selection_sensitive: Rc<dyn Fn(bool)>,
) {
    let parent = parent.clone().upcast::<gtk::Window>();
    let group = gio::SimpleActionGroup::new();
    {
        let parent = parent.clone();
        let selected_path = Rc::clone(&selected_path);
        let act = gio::SimpleAction::new("open-file", None);
        act.connect_activate(move |_, _| {
            if let Some(path) = selected_path.borrow().clone() {
                crate::util::open_file_default(Some(&parent), &gio::File::for_path(&path));
            }
        });
        group.add_action(&act);
    }
    {
        let on_reveal = Rc::clone(&on_reveal);
        let selected_path = Rc::clone(&selected_path);
        let act = gio::SimpleAction::new("show-in-folder", None);
        act.connect_activate(move |_, _| {
            if let Some(path) = selected_path.borrow().clone() {
                on_reveal(path);
            }
        });
        group.add_action(&act);
    }
    {
        let on_open_folder = Rc::clone(&on_open_folder);
        let selected_path = Rc::clone(&selected_path);
        let act = gio::SimpleAction::new("open-folder", None);
        act.connect_activate(move |_, _| {
            if let Some(path) = selected_path.borrow().clone() {
                on_open_folder(containing_folder(&path));
            }
        });
        group.add_action(&act);
    }
    list.insert_action_group("find", Some(&group));

    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();
    icons.append(&menu, "Open File", "find.open-file", "document-open-symbolic");
    icons.append(
        &menu,
        "Show in Folder",
        "find.show-in-folder",
        "folder-symbolic",
    );
    icons.append(
        &menu,
        "Open Folder",
        "find.open-folder",
        "folder-open-symbolic",
    );

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    icons.bind_popover(&popover);
    popover.set_parent(list);
    popover.set_has_arrow(false);
    {
        let popover_weak = popover.downgrade();
        list.connect_destroy(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.unparent();
            }
        });
    }

    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    {
        let list = list.clone();
        let selected_path = Rc::clone(&selected_path);
        let set_selection_sensitive = Rc::clone(&set_selection_sensitive);
        let popover = popover.clone();
        gesture.connect_pressed(move |g, _, x, y| {
            // Prefer the row under the pointer so actions target that match.
            if let Some(row) = list.row_at_y(y as i32) {
                list.select_row(Some(&row));
                let path = unsafe { row.data::<PathBuf>("path").map(|p| p.as_ref().clone()) };
                set_selection_sensitive(path.is_some());
                *selected_path.borrow_mut() = path;
            }
            if selected_path.borrow().is_none() {
                return;
            }
            g.set_state(gtk::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gdk::Rectangle::new(
                x.round() as i32,
                y.round() as i32,
                1,
                1,
            )));
            popover.popup();
        });
    }
    list.add_controller(gesture);
}

fn labeled(title: &str, child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.add_css_class("dim-label");
    box_.append(&label);
    box_.append(child);
    box_
}

fn start_search(results: &gtk::ListBox, status: &gtk::Label, opts: SearchOpts) {
    while let Some(child) = results.first_child() {
        results.remove(&child);
    }
    status.set_text("Searching…");

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

fn run_search(opts: &SearchOpts) -> Result<Vec<MatchHit>, String> {
    if !opts.directory.is_dir() {
        return Err(format!("Not a directory: {}", opts.directory.display()));
    }
    if which_rg() {
        if let Ok(hits) = run_search_rg(opts) {
            return Ok(hits);
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
        if hits.len() >= MAX_HITS {
            break;
        }
    }
    Ok(hits)
}

fn split_rg_line(line: &str) -> Option<(&str, &str)> {
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
    !buf[..n].contains(&0)
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
