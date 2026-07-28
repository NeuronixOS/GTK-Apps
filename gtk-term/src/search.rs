//! Find/search bar widget for searching terminal scrollback.
//!
//! Mirrors gnome-terminal's TerminalFindBar: entry + up/down + options
//! (match case, whole words, regex) with Escape to dismiss.

use gtk4 as gtk;
use gtk::prelude::*;
use vte4::prelude::*;

/// Build the search bar UI and wire it to the given terminal getter.
/// Returns `(revealer, set_terminal_fn)` where the revealer wraps the
/// entire find bar and `set_terminal_fn` updates which terminal is being
/// searched (call it on tab switch).
pub fn build_find_bar() -> (gtk::Revealer, gtk::Box) {
    let entry = gtk::Entry::builder()
        .placeholder_text("Search…")
        .hexpand(true)
        .primary_icon_name("edit-find-symbolic")
        .build();

    let match_case = gtk::CheckButton::builder()
        .label("Match Case")
        .build();
    let whole_words = gtk::CheckButton::builder()
        .label("Whole Words")
        .build();
    let use_regex = gtk::CheckButton::builder()
        .label("Regular Expression")
        .build();

    let opts_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    opts_box.append(&match_case);
    opts_box.append(&whole_words);
    opts_box.append(&use_regex);

    let opts_popover = gtk::Popover::builder()
        .child(&opts_box)
        .build();

    let opts_btn = gtk::MenuButton::builder()
        .icon_name("emblem-system-symbolic")
        .tooltip_text("Search Options")
        .popover(&opts_popover)
        .build();

    let prev_btn = gtk::Button::from_icon_name("go-up-symbolic");
    prev_btn.set_tooltip_text(Some("Previous match"));
    let next_btn = gtk::Button::from_icon_name("go-down-symbolic");
    next_btn.set_tooltip_text(Some("Next match"));

    let nav_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    nav_box.add_css_class("linked");
    nav_box.append(&prev_btn);
    nav_box.append(&next_btn);

    let close_btn = gtk::Button::from_icon_name("window-close-symbolic");
    close_btn.add_css_class("flat");
    close_btn.set_tooltip_text(Some("Close search"));

    let bar_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    bar_box.set_margin_top(6);
    bar_box.set_margin_bottom(6);
    bar_box.set_margin_start(6);
    bar_box.set_margin_end(9);
    bar_box.append(&entry);
    bar_box.append(&nav_box);
    bar_box.append(&opts_btn);
    bar_box.append(&close_btn);

    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .reveal_child(false)
        .child(&bar_box)
        .build();

    // Close button hides the revealer and refocuses the terminal.
    {
        let revealer_weak = revealer.downgrade();
        close_btn.connect_clicked(move |_| {
            if let Some(rev) = revealer_weak.upgrade() {
                rev.set_reveal_child(false);
            }
        });
    }

    // Escape key in entry closes the search bar.
    {
        let revealer_weak = revealer.downgrade();
        let key_controller = gtk::EventControllerKey::new();
        key_controller.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(rev) = revealer_weak.upgrade() {
                    rev.set_reveal_child(false);
                }
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        entry.add_controller(key_controller);
    }

    // Store the option widgets in the bar_box via widget names so we can
    // retrieve them later when updating the regex.
    entry.set_widget_name("find-entry");
    match_case.set_widget_name("find-match-case");
    whole_words.set_widget_name("find-whole-words");
    use_regex.set_widget_name("find-use-regex");

    (revealer, bar_box)
}

/// Wire the find bar's entry/buttons to a specific VTE terminal.
/// Call this once when constructing the UI and again whenever the active
/// tab changes.
pub fn connect_find_bar(
    bar_box: &gtk::Box,
    revealer: &gtk::Revealer,
    get_terminal: impl Fn() -> Option<vte4::Terminal> + Clone + 'static,
) {
    let entry = find_child_by_name::<gtk::Entry>(bar_box, "find-entry");
    let match_case = find_child_by_name::<gtk::CheckButton>(bar_box, "find-match-case");
    let whole_words = find_child_by_name::<gtk::CheckButton>(bar_box, "find-whole-words");
    let use_regex_btn = find_child_by_name::<gtk::CheckButton>(bar_box, "find-use-regex");

    let (Some(entry), Some(match_case), Some(whole_words), Some(use_regex_btn)) =
        (entry, match_case, whole_words, use_regex_btn)
    else {
        return;
    };

    let update_regex = {
        let entry = entry.clone();
        let match_case = match_case.clone();
        let whole_words = whole_words.clone();
        let use_regex_btn = use_regex_btn.clone();
        let get_terminal = get_terminal.clone();
        move || {
            let Some(term) = get_terminal() else { return };
            let text = entry.text();
            if text.is_empty() {
                term.search_set_regex(None::<&vte4::Regex>, 0);
                return;
            }

            let pattern = if use_regex_btn.is_active() {
                text.to_string()
            } else {
                regex_escape(&text)
            };

            let mut flags = "(?su)".to_string();
            if !match_case.is_active() {
                flags.push_str("(?i)");
            }

            let final_pattern = if whole_words.is_active() {
                format!("{flags}\\b{pattern}\\b")
            } else {
                format!("{flags}{pattern}")
            };

            match vte4::Regex::for_search(&final_pattern, 0) {
                Ok(re) => {
                    term.search_set_regex(Some(&re), 0);
                    term.search_set_wrap_around(true);
                    entry.remove_css_class("error");
                }
                Err(_) => {
                    entry.add_css_class("error");
                }
            }
        }
    };

    // Update regex on every keystroke.
    {
        let update = update_regex.clone();
        entry.connect_changed(move |_| update());
    }

    // Also update when toggling options.
    {
        let update = update_regex.clone();
        match_case.connect_toggled(move |_| update());
    }
    {
        let update = update_regex.clone();
        whole_words.connect_toggled(move |_| update());
    }
    {
        let update = update_regex.clone();
        use_regex_btn.connect_toggled(move |_| update());
    }

    // Prev / Next buttons.
    let prev_btn = find_descendant_button(bar_box, "go-up-symbolic");
    let next_btn = find_descendant_button(bar_box, "go-down-symbolic");

    if let Some(btn) = prev_btn {
        let gt = get_terminal.clone();
        btn.connect_clicked(move |_| {
            if let Some(t) = gt() {
                t.search_find_previous();
            }
        });
    }
    if let Some(btn) = next_btn {
        let gt = get_terminal.clone();
        btn.connect_clicked(move |_| {
            if let Some(t) = gt() {
                t.search_find_next();
            }
        });
    }

    // Enter = find previous (upwards), Shift+Enter = find next (downwards),
    // matching gnome-terminal behaviour.
    {
        let gt = get_terminal.clone();
        let revealer_weak = revealer.downgrade();
        let key_controller = gtk::EventControllerKey::new();
        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            if key == gtk::gdk::Key::Return || key == gtk::gdk::Key::KP_Enter {
                if let Some(t) = gt() {
                    if modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
                        t.search_find_next();
                    } else {
                        t.search_find_previous();
                    }
                }
                return gtk::glib::Propagation::Stop;
            }
            if key == gtk::gdk::Key::Escape {
                if let Some(rev) = revealer_weak.upgrade() {
                    rev.set_reveal_child(false);
                }
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        entry.add_controller(key_controller);
    }
}

fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if "\\^$.|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn find_child_by_name<T: IsA<gtk::Widget>>(container: &gtk::Box, name: &str) -> Option<T> {
    let mut child = container.first_child();
    while let Some(w) = child {
        if w.widget_name() == name {
            return w.downcast::<T>().ok();
        }
        if let Ok(inner_box) = w.clone().downcast::<gtk::Box>() {
            if let Some(found) = find_child_by_name::<T>(&inner_box, name) {
                return Some(found);
            }
        }
        if let Ok(popover) = w.clone().downcast::<gtk::Popover>() {
            if let Some(pop_child) = popover.child() {
                if pop_child.widget_name() == name {
                    return pop_child.downcast::<T>().ok();
                }
                if let Ok(inner_box) = pop_child.downcast::<gtk::Box>() {
                    if let Some(found) = find_child_by_name::<T>(&inner_box, name) {
                        return Some(found);
                    }
                }
            }
        }
        if let Ok(menu_btn) = w.clone().downcast::<gtk::MenuButton>() {
            if let Some(popover) = menu_btn.popover() {
                if let Ok(pop) = popover.downcast::<gtk::Popover>() {
                    if let Some(pop_child) = pop.child() {
                        if pop_child.widget_name() == name {
                            return pop_child.downcast::<T>().ok();
                        }
                        if let Ok(inner_box) = pop_child.downcast::<gtk::Box>() {
                            if let Some(found) = find_child_by_name::<T>(&inner_box, name) {
                                return Some(found);
                            }
                        }
                    }
                }
            }
        }
        child = w.next_sibling();
    }
    None
}

fn find_descendant_button(container: &gtk::Box, icon_name: &str) -> Option<gtk::Button> {
    let mut child = container.first_child();
    while let Some(w) = child {
        if let Ok(btn) = w.clone().downcast::<gtk::Button>() {
            if btn.icon_name().as_deref() == Some(icon_name) {
                return Some(btn);
            }
        }
        if let Ok(inner_box) = w.clone().downcast::<gtk::Box>() {
            if let Some(found) = find_descendant_button(&inner_box, icon_name) {
                return Some(found);
            }
        }
        child = w.next_sibling();
    }
    None
}
