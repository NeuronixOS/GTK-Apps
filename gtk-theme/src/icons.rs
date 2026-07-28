//! Adwaita / Freedesktop icon helpers for buttons and menus.
//!
//! Icon artwork comes from the system `adwaita-icon-theme`. GTK4
//! `PopoverMenu` does not show `Gio.MenuItem` icons on ordinary text rows,
//! so [`IconMenu`] marks rows with `custom` ids and binds `Image` + `Label`
//! children after the popover is built.
//!
//! Call [`ensure_adwaita_icons`] (via [`crate::apply_chrome`]) so Adwaita
//! symbolics resolve even when the desktop icon theme is something else
//! (e.g. Faenza).

use std::sync::Once;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib::prelude::*;
use gtk::prelude::*;

/// Default pixel size for symbolic chrome icons beside labels.
pub const SYMBOLIC_PIXEL_SIZE: i32 = 16;

/// Runtime package that supplies the symbolic icons these helpers request.
pub const RUNTIME_ICON_THEME_PACKAGE: &str = "adwaita-icon-theme";

/// Freedesktop app icon names used with [`gtk::Window::set_default_icon_name`].
pub mod app_icons {
    pub const CALC: &str = "accessories-calculator";
    pub const EDIT: &str = "accessories-text-editor";
    pub const FILES: &str = "system-file-manager";
    /// Eye of GNOME / image viewer; widely shipped under this name.
    pub const IMAGE: &str = "org.gnome.eog";
    pub const TERM: &str = "utilities-terminal";
}

/// Ensure Adwaita icon directories are searched so suite chrome icons resolve
/// even when the session theme (e.g. Faenza) does not inherit Adwaita.
///
/// Adds Adwaita’s symbolic category dirs to the display [`gtk::IconTheme`]
/// search path (basename lookup). Does not change `gtk-icon-theme-name`, so
/// full-color mime/app icons can still come from the session theme.
///
/// Safe to call repeatedly; only the first call mutates the display theme.
pub fn ensure_adwaita_icons() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let theme = gtk::IconTheme::for_display(&display);
        const ROOTS: &[&str] = &["/usr/share/icons/Adwaita", "/usr/local/share/icons/Adwaita"];
        // GTK search paths look for ICON.svg directly in the directory; Adwaita
        // stores symbolics under these category folders (not only theme lookup).
        const SYMBOLIC_SUBDIRS: &[&str] = &[
            "symbolic/actions",
            "symbolic/apps",
            "symbolic/categories",
            "symbolic/devices",
            "symbolic/emblems",
            "symbolic/emotes",
            "symbolic/mimetypes",
            "symbolic/places",
            "symbolic/status",
            "symbolic/ui",
            "symbolic/legacy",
            "symbolic",
        ];
        for root in ROOTS {
            let root_path = std::path::Path::new(root);
            if !root_path.is_dir() {
                continue;
            }
            theme.add_search_path(root);
            for sub in SYMBOLIC_SUBDIRS {
                let p = root_path.join(sub);
                if p.is_dir() {
                    theme.add_search_path(p);
                }
            }
        }
    });
}

/// A single custom menu row: icon beside label.
#[derive(Debug, Clone)]
pub struct MenuIconEntry {
    pub id: String,
    pub icon: String,
    pub label: String,
    /// Detailed action name (`win.save`, `app.about`, …) activated on click.
    pub action: String,
}

/// Collects custom menu-row bindings while building a `Gio.Menu` tree.
///
/// Call [`IconMenu::bind_popover`] or [`IconMenu::bind_menubar`] once on the
/// top-level widget created from that model.
///
/// Custom `PopoverMenu` children replace the default model button, so rows are
/// bound as actionable [`gtk::Button`]s (plain `Image`+`Label` boxes are not
/// clickable).
#[derive(Debug, Default, Clone)]
pub struct IconMenu {
    next_id: u32,
    entries: Vec<MenuIconEntry>,
}

impl IconMenu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[MenuIconEntry] {
        &self.entries
    }

    /// Merge entries from another builder (same popover/model tree).
    pub fn extend(&mut self, other: IconMenu) {
        self.next_id = self.next_id.max(other.next_id);
        self.entries.extend(other.entries);
    }

    fn alloc_id(&mut self) -> String {
        let id = format!("gtk-theme-icon-{}", self.next_id);
        self.next_id += 1;
        id
    }

    /// Append a normal actionable item; icon is shown beside the label.
    pub fn append(&mut self, menu: &gio::Menu, label: &str, action: &str, icon: &str) {
        let id = self.alloc_id();
        // Action stays on the model for accel display / exporters; the custom
        // child button re-binds the same action for clicks.
        let item = gio::MenuItem::new(Some(label), Some(action));
        item.set_attribute_value("custom", Some(&id.to_variant()));
        let gicon = gio::ThemedIcon::new(icon);
        item.set_icon(&gicon);
        menu.append_item(&item);
        self.entries.push(MenuIconEntry {
            id,
            icon: icon.to_string(),
            label: label.to_string(),
            action: action.to_string(),
        });
    }

    /// Like [`IconMenu::append`], looking up the icon via [`icon_for_action`].
    pub fn append_action(&mut self, menu: &gio::Menu, label: &str, action: &str) {
        let icon = icon_for_action(action);
        self.append(menu, label, action, icon);
    }

    /// Append a submenu whose parent row uses a themed icon (when GTK shows it).
    pub fn append_submenu(
        &mut self,
        parent: &gio::Menu,
        label: &str,
        submenu: &gio::Menu,
        icon: &str,
    ) {
        let item = gio::MenuItem::new(Some(label), None);
        item.set_submenu(Some(submenu));
        let gicon = gio::ThemedIcon::new(icon);
        item.set_icon(&gicon);
        parent.append_item(&item);
    }

    /// Bind icon+label custom children into a [`gtk::PopoverMenu`].
    pub fn bind_popover(&self, popover: &gtk::PopoverMenu) {
        for entry in &self.entries {
            let child = menu_row_button(&entry.icon, &entry.label, &entry.action);
            popover.add_child(&child, &entry.id);
        }
    }

    /// Bind icon+label custom children into a [`gtk::PopoverMenuBar`].
    pub fn bind_menubar(&self, bar: &gtk::PopoverMenuBar) {
        for entry in &self.entries {
            let child = menu_row_button(&entry.icon, &entry.label, &entry.action);
            bar.add_child(&child, &entry.id);
        }
    }

    /// Bind into the popover owned by a [`gtk::MenuButton`], if present.
    pub fn bind_menu_button(&self, button: &gtk::MenuButton) {
        if let Some(popover) = button.popover().and_downcast::<gtk::PopoverMenu>() {
            self.bind_popover(&popover);
        }
    }
}

/// Symbolic [`gtk::Image`] at the suite’s standard chrome size.
pub fn symbolic_image(icon_name: &str) -> gtk::Image {
    let image = gtk::Image::from_icon_name(icon_name);
    image.set_pixel_size(SYMBOLIC_PIXEL_SIZE);
    image.set_icon_size(gtk::IconSize::Normal);
    image
}

/// Toolbar-style icon-only button (existing suite pattern).
pub fn icon_button(icon_name: &str) -> gtk::Button {
    gtk::Button::from_icon_name(icon_name)
}

/// Button with a symbolic icon beside the label text.
pub fn labeled_button(icon_name: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_child(Some(&icon_label_box(icon_name, label)));
    button
}

/// Horizontal box: symbolic image + label (left-aligned).
pub fn icon_label_box(icon_name: &str, label: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.set_halign(gtk::Align::Fill);
    row.set_hexpand(true);
    row.set_valign(gtk::Align::Center);
    let image = symbolic_image(icon_name);
    image.set_halign(gtk::Align::Start);
    image.set_valign(gtk::Align::Center);
    // Let clicks reach the parent actionable button.
    image.set_can_target(false);
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_halign(gtk::Align::Start);
    text.set_hexpand(true);
    text.set_can_target(false);
    row.append(&image);
    row.append(&text);
    row
}

/// Clickable menu row used as a `PopoverMenu` / `PopoverMenuBar` custom child.
///
/// GTK replaces the default model button when `custom` is set, so the child
/// itself must be actionable via [`gtk::Actionable::set_action_name`].
///
/// For manually parented context `PopoverMenu`s, the host must also
/// `insert_action_group("win", …)` (see gtk-files) so `win.*` resolves.
/// Do not replace this with a bare `activate_action` click handler — that
/// breaks `MenuButton` hamburger menus (Preferences, etc.).
pub fn menu_row_button(icon_name: &str, label: &str, action: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("gtk-theme-menu-row");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button.set_halign(gtk::Align::Fill);
    // Use detailed names so parameterized actions like
    // `win.create-from-template('Text Document')` keep their target.
    button.set_detailed_action_name(action);
    // Fill the row so GtkButton cannot center a shrink-wrapped child.
    let row = icon_label_box(icon_name, label);
    row.set_halign(gtk::Align::Fill);
    row.set_hexpand(true);
    button.set_child(Some(&row));
    button.connect_clicked(|btn| {
        // Custom children do not auto-dismiss the menu like model buttons.
        if let Some(ancestor) = btn.ancestor(gtk::Popover::static_type()) {
            if let Ok(popover) = ancestor.downcast::<gtk::Popover>() {
                popover.popdown();
            }
        }
    });
    button
}

/// Map a detailed action name (`win.save`, `app.about`, …) to an Adwaita symbolic icon.
pub fn icon_for_action(action: &str) -> &'static str {
    // `win.create-from-template('…')` → `create-from-template`
    let name = action
        .rsplit('.')
        .next()
        .unwrap_or(action)
        .split('(')
        .next()
        .unwrap_or(action)
        .split("::")
        .next()
        .unwrap_or(action);
    match name {
        // File / documents
        "new" | "new-file" | "new-document" | "create-from-template" => "document-new-symbolic",
        "new-window" => "window-new-symbolic",
        "new-tab" | "tab-new" => "tab-new-symbolic",
        "new-folder" => "folder-new-symbolic",
        "open" | "open-folder" | "open-with" | "open-in-tab" | "open-tab" | "open-window" => {
            "document-open-symbolic"
        }
        "open-recent" | "history" => "document-open-recent-symbolic",
        "save" => "document-save-symbolic",
        "save-as" => "document-save-as-symbolic",
        "save-all" => "document-save-symbolic",
        "revert" => "document-revert-symbolic",
        "print" | "print-preview" => "document-print-symbolic",
        "page-setup" => "document-page-setup-symbolic",
        "properties" => "document-properties-symbolic",
        "close" | "close-tab" | "close-window" => "window-close-symbolic",
        "quit" | "exit" => "application-exit-symbolic",

        // Edit
        "cut" => "edit-cut-symbolic",
        "copy" | "copy-name" | "copy-path" | "copy-name-path" => "edit-copy-symbolic",
        "paste" | "paste-into" => "edit-paste-symbolic",
        "undo" => "edit-undo-symbolic",
        "redo" => "edit-redo-symbolic",
        "select-all" => "edit-select-all-symbolic",
        "delete" | "trash" | "empty-trash" => "user-trash-symbolic",
        "find" | "search" | "find-in-files" => "edit-find-symbolic",
        "replace" | "find-replace" => "edit-find-replace-symbolic",
        "rename" => "document-edit-symbolic",
        "duplicate" => "edit-copy-symbolic",
        "create-link" => "insert-link-symbolic",
        "clear" | "clear-history" => "edit-clear-symbolic",

        // View / navigation
        "reload" | "refresh" => "view-refresh-symbolic",
        "toggle-view" | "view-grid" | "view-list" => "view-grid-symbolic",
        "show-hidden" => "view-reveal-symbolic",
        "go-back" | "back" | "previous" | "prev" => "go-previous-symbolic",
        "go-forward" | "forward" | "next" => "go-next-symbolic",
        "go-up" | "up" | "parent" => "go-up-symbolic",
        "go-home" | "home" => "go-home-symbolic",
        "edit-location" | "enter-location" => "go-jump-symbolic",
        "zoom-in" => "zoom-in-symbolic",
        "zoom-out" => "zoom-out-symbolic",
        "zoom-fit" | "fit" | "zoom-original" | "zoom-reset" => "zoom-fit-best-symbolic",
        "fullscreen" | "view-fullscreen" => "view-fullscreen-symbolic",
        "sidebar" | "toggle-sidebar" => "view-sidebar-start-symbolic",

        // Sort / thumbs
        "sort-name" | "sort-size" | "sort-type" | "sort-modified" => "view-sort-ascending-symbolic",
        "thumb-small" | "thumb-medium" | "thumb-large" | "thumb-larger" | "thumb-largest" => {
            "image-x-generic-symbolic"
        }

        // Bookmarks / favorites
        "add-favorite" | "add-bookmark" | "bookmark" => "bookmark-new-symbolic",

        // Image / media
        "rotate-left" | "rotate-right" => "object-rotate-left-symbolic",
        "flip-horizontal" | "flip-vertical" => "object-flip-horizontal-symbolic",
        "slideshow" => "media-playback-start-symbolic",
        "convert-to-jpeg" | "convert-to-png" | "convert-to-pdf" | "convert-to-webp" => {
            "image-x-generic-symbolic"
        }

        // Terminal
        "reset" | "reset-terminal" => "edit-clear-symbolic",
        "read-only" => "emblem-readonly-symbolic",
        "copy-input" => "edit-copy-symbolic",

        // App / help
        "preferences" | "prefs" | "settings" => "preferences-system-symbolic",
        "shortcuts" | "keyboard-shortcuts" => "preferences-desktop-keyboard-symbolic",
        "about" | "help" => "help-about-symbolic",
        "theme" | "profile" => "color-select-symbolic",

        // Fallbacks for common dialog verbs (used as fake actions or direct icons)
        "ok" | "apply" | "create" => "object-select-symbolic",
        "cancel" => "window-close-symbolic",
        "browse" => "folder-symbolic",

        _ => "emblem-system-symbolic",
    }
}

/// Convenience icons for common dialog button labels.
pub fn icon_for_label(label: &str) -> &'static str {
    let key = label
        .trim_end_matches('…')
        .trim_end_matches("...")
        .trim()
        .to_ascii_lowercase();
    match key.as_str() {
        "ok" | "okay" | "apply" | "create" | "rename" | "save" | "open" | "search" => {
            icon_for_action(key.as_str())
        }
        "cancel" | "close" => "window-close-symbolic",
        "browse" => "folder-symbolic",
        "show in folder" | "reveal" => "folder-symbolic",
        "find" | "replace" | "replace all" => "edit-find-replace-symbolic",
        _ => "emblem-system-symbolic",
    }
}
