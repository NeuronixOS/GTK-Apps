//! Main editor window: menus, tabs, panels, actions.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::glib::prelude::*;
use gtk::prelude::*;
use sourceview5::prelude::*;

use crate::config::Config;
use crate::documents_panel::DocumentsPanel;
use crate::io::{self, externally_modified, load_path, save_path, show_io_error};
use crate::panel::Panel;
use crate::plugin::{PluginEngine, WindowContext};
use crate::prefs;
use crate::print;
use crate::replace::ReplaceDialog;
use crate::search::{clear_search_highlights, SearchBar};
use crate::statusbar::Statusbar;
use crate::tab::{EditorTab, TabNotebook};

const WINDOW_STATE_KEY: &str = "gtk-edit-state";

pub struct EditorWindow {
    pub window: gtk::ApplicationWindow,
    pub config: Rc<RefCell<Config>>,
    pub engine: Rc<PluginEngine>,
    pub groups: RefCell<Vec<Rc<TabNotebook>>>,
    pub groups_box: gtk::Box,
    pub active_group: RefCell<usize>,
    pub search: Rc<SearchBar>,
    pub side_panel: Panel,
    pub bottom_panel: Panel,
    pub docs_panel: Rc<DocumentsPanel>,
    pub statusbar: Statusbar,
    /// Horizontal: left side panel | (editor + bottom tools)
    pub hpaned: gtk::Paned,
    /// Vertical: editor center | bottom panel (terminal / file search / tools)
    pub content_paned: gtk::Paned,
    pub tools_menu: gio::Menu,
    pub edit_menu_extra: gio::Menu,
    pub search_menu_extra: gio::Menu,
    /// Direct terminal cwd hook (same pattern as gtk-files `terminal.sync_cwd`).
    /// Registered by the terminal plugin on activate.
    pub terminal_sync: RefCell<Option<Rc<dyn Fn(&Path)>>>,
    /// Direct file-browser cwd hook: follows the focused document's folder.
    /// Registered by the file browser plugin on activate.
    pub filebrowser_sync: RefCell<Option<Rc<dyn Fn(&Path)>>>,
}

impl EditorWindow {
    pub fn new(
        app: &gtk::Application,
        config: Rc<RefCell<Config>>,
        engine: Rc<PluginEngine>,
    ) -> Rc<Self> {
        // Keep GtkSourceView scheme aligned with the shared suite profile so the
        // editor doesn't stay on light "classic" while chrome uses a dark theme.
        {
            let mut c = config.borrow_mut();
            let profile_id = gtk_theme::load_theme_id();
            let mgr = sourceview5::StyleSchemeManager::default();
            let scheme = gtk_theme::resolve_sourceview_scheme(&profile_id, |id| {
                mgr.scheme(id).is_some()
            });
            c.editor.scheme = scheme.to_string();
        }
        let cfg = config.borrow().clone();
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("GTK Edit")
            .default_width(cfg.state.window_width)
            .default_height(cfg.state.window_height)
            .build();

        let tools_menu = gio::Menu::new();
        let edit_menu_extra = gio::Menu::new();
        let search_menu_extra = gio::Menu::new();

        let (menubar, menu_icons) =
            build_menubar(&tools_menu, &edit_menu_extra, &search_menu_extra);
        let menu_icons = Rc::new(RefCell::new(menu_icons));
        let popover = gtk::PopoverMenu::from_model(Some(&menubar));
        let menu_btn = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .popover(&popover)
            .tooltip_text("Menu")
            .build();

        let header = gtk::HeaderBar::new();
        gtk_theme::prepare_headerbar(&header);
        header.pack_end(&menu_btn);
        header.set_title_widget(Some(&gtk::Label::new(Some("GTK Edit"))));
        window.set_titlebar(Some(&header));

        let search = SearchBar::new();
        let side_panel = Panel::new_untitled();
        let bottom_panel = Panel::new_untitled();
        let docs_panel = DocumentsPanel::new();
        side_panel.add_page("Documents", &docs_panel.root);
        side_panel.set_visible_panel(cfg.ui.side_panel_visible);
        bottom_panel.set_visible_panel(cfg.ui.bottom_panel_visible);

        let statusbar = Statusbar::new();
        statusbar.root.set_visible(cfg.ui.statusbar_visible);

        let groups_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        groups_box.set_homogeneous(true);
        groups_box.set_vexpand(true);
        groups_box.set_hexpand(true);

        let first = TabNotebook::new();
        groups_box.append(&first.notebook);

        let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
        center.set_hexpand(true);
        center.set_vexpand(true);
        center.append(&search.revealer);
        center.append(&groups_box);

        // Bottom panel (terminal / file search / tools) under the editor.
        let content_paned = gtk::Paned::new(gtk::Orientation::Vertical);
        content_paned.set_start_child(Some(&center));
        content_paned.set_end_child(Some(&bottom_panel.root));
        content_paned.set_resize_start_child(true);
        content_paned.set_shrink_start_child(true);
        content_paned.set_resize_end_child(false);
        content_paned.set_shrink_end_child(false);
        content_paned.set_wide_handle(true);

        let hpaned = gtk::Paned::new(gtk::Orientation::Horizontal);
        hpaned.set_start_child(Some(&side_panel.root));
        hpaned.set_end_child(Some(&content_paned));
        hpaned.set_position(cfg.state.side_panel_size);
        hpaned.set_resize_start_child(false);
        hpaned.set_shrink_start_child(false);

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outer.add_css_class("gtk-content");
        outer.append(&hpaned);
        outer.append(&statusbar.root);
        window.set_child(Some(&outer));

        let this = Rc::new(Self {
            window: window.clone(),
            config,
            engine,
            groups: RefCell::new(vec![first]),
            groups_box,
            active_group: RefCell::new(0),
            search,
            side_panel,
            bottom_panel,
            docs_panel,
            statusbar,
            hpaned,
            content_paned,
            tools_menu,
            edit_menu_extra,
            search_menu_extra,
            terminal_sync: RefCell::new(None),
            filebrowser_sync: RefCell::new(None),
        });

        unsafe {
            window.set_data(WINDOW_STATE_KEY, Rc::clone(&this));
        }

        {
            let first_nb = this.groups.borrow()[0].clone();
            this.wire_notebook(&first_nb);
        }

        install_actions(&this);
        install_accels(app);
        wire_search(&this);
        build_language_menu(&this);
        build_tab_width_menu(&this);

        // Activate window plugins
        let ctx = WindowContext {
            window: window.clone(),
            side_panel: this.side_panel.root.clone(),
            bottom_panel: this.bottom_panel.root.clone(),
            tools_menu: this.tools_menu.clone(),
            edit_menu: this.edit_menu_extra.clone(),
            search_menu: this.search_menu_extra.clone(),
            menu_icons: Rc::clone(&menu_icons),
            status_label: this.statusbar.message.clone(),
        };
        this.engine.activate_window_plugins(&ctx);

        // Bind icon rows after plugins have appended their menu items.
        {
            let icons = menu_icons.borrow();
            icons.bind_popover(&popover);
        }

        // Restore last side-panel tab (Documents / File Browser) after plugins
        // have appended their pages.
        this.side_panel
            .restore_page_id(&this.config.borrow().state.side_panel_page);
        {
            let this2 = Rc::clone(&this);
            this.side_panel
                .notebook
                .connect_switch_page(move |_, _, _page| {
                    // Persist immediately so the choice survives even if the
                    // window is killed before a clean close.
                    if let Some(id) = this2.side_panel.current_page_id() {
                        this2.config.borrow_mut().state.side_panel_page = id;
                        let _ = this2.config.borrow().save();
                    }
                });
        }

        // Start with no tabs — File → New / Open adds them as needed.

        // Place the bottom-panel divider once we know the real allocated height.
        {
            let this2 = Rc::clone(&this);
            this.window.connect_map(move |_| {
                this2.ensure_bottom_panel_height();
            });
        }

        // Close request
        {
            let this2 = Rc::clone(&this);
            this.window.connect_close_request(move |_| {
                this2.save_session_state();
                if this2.close_all_tabs() {
                    glib::Propagation::Proceed
                } else {
                    glib::Propagation::Stop
                }
            });
        }

        this
    }

    /// Keep the bottom panel at its configured height when it is visible.
    pub fn ensure_bottom_panel_height(&self) {
        if !self.bottom_panel.root.is_visible() {
            return;
        }
        let paned = self.content_paned.clone();
        let saved = self.config.borrow().state.bottom_panel_size.max(120);
        glib::idle_add_local_once(move || {
            let total = paned.height();
            if total > saved + 160 {
                paned.set_position(total - saved);
            }
        });
    }

    /// Backward-compatible alias used by plugins.
    pub fn ensure_bottom_panel_width(&self) {
        self.ensure_bottom_panel_height();
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn active_notebook(&self) -> Rc<TabNotebook> {
        let idx = *self.active_group.borrow();
        self.groups.borrow()[idx].clone()
    }

    pub fn current_tab(&self) -> Option<Rc<EditorTab>> {
        self.active_notebook().current()
    }

    /// Directory for the focused document (parent of the file), like gtk-files
    /// syncing the terminal to the folder shown in the active tab.
    pub fn focused_document_dir(&self) -> PathBuf {
        self.current_tab()
            .map(|tab| Self::dir_for_tab(&tab))
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")))
    }

    fn dir_for_tab(tab: &EditorTab) -> PathBuf {
        tab.document
            .path()
            .and_then(|p| {
                if p.is_dir() {
                    Some(p)
                } else {
                    p.parent().map(|p| p.to_path_buf())
                }
            })
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")))
    }

    /// Keep the bottom terminal in the focused tab's folder (gtk-files style).
    pub fn sync_terminal_cwd(&self) {
        let dir = self.focused_document_dir();
        self.sync_terminal_to_dir(&dir);
    }

    pub fn sync_terminal_for_tab(&self, tab: &EditorTab) {
        self.sync_terminal_to_dir(&Self::dir_for_tab(tab));
    }

    pub fn sync_terminal_to_dir(&self, dir: &Path) {
        if let Some(sync) = self.terminal_sync.borrow().clone() {
            sync(dir);
        }
        if let Some(sync) = self.filebrowser_sync.borrow().clone() {
            sync(dir);
        }
    }

    pub fn new_tab(&self) -> Rc<EditorTab> {
        let cfg = self.config.borrow().editor.clone();
        let tab = EditorTab::new(&cfg);
        self.wire_tab(&tab);
        self.active_notebook().add_tab(Rc::clone(&tab));
        self.active_notebook()
            .update_tabs_visibility(&self.config.borrow().ui.notebook_show_tabs_mode);
        self.engine.activate_view_plugins(&tab.view);
        self.refresh_documents_panel();
        self.update_statusbar();
        tab
    }

    pub fn open_path(&self, path: &Path) {
        // Reuse existing tab if open
        for group in self.groups.borrow().iter() {
            for tab in group.tabs.borrow().iter() {
                if tab.document.path().as_deref() == Some(path) {
                    // focus
                    if let Some(idx) = group
                        .tabs
                        .borrow()
                        .iter()
                        .position(|t| Rc::ptr_eq(t, tab))
                    {
                        group.notebook.set_current_page(Some(idx as u32));
                    }
                    self.update_statusbar();
                    tab.sync_markdown_preview();
                    // Direct sync like gtk-files — pass the file's folder now.
                    self.sync_terminal_to_dir(
                        path.parent()
                            .filter(|p| p.is_dir())
                            .unwrap_or(path),
                    );
                    return;
                }
            }
        }

        let tab = self.new_tab();
        let cfg = self.config.borrow().clone();
        if let Err(e) = load_path(
            &tab.document,
            path,
            &cfg.editor,
            &cfg.encodings.auto_detected,
        ) {
            // Drop the empty tab created for this failed open.
            let nb = self.active_notebook();
            nb.remove_tab(&tab);
            if nb.len() == 0 {
                self.new_tab();
            }
            let title = if e.contains("does not appear to be a text file") {
                "Cannot open file"
            } else {
                "Error opening file"
            };
            show_io_error(&self.window, title, &e);
            self.refresh_documents_panel();
            self.update_statusbar();
            return;
        }
        if cfg.editor.restore_cursor_position {
            tab.document.restore_cursor();
        }
        tab.refresh_title();
        tab.apply_config(&cfg.editor);
        tab.sync_markdown_preview();
        self.engine.activate_view_plugins(&tab.view);
        self.config.borrow_mut().add_recent(path);
        let _ = self.config.borrow().save();
        self.refresh_documents_panel();
        self.update_statusbar();
        self.window
            .set_title(Some(&format!("{} — GTK Edit", tab.document.title())));
        // Same as gtk-files: sync terminal to this file's folder immediately.
        self.sync_terminal_for_tab(&tab);
    }

    fn wire_tab(&self, tab: &Rc<EditorTab>) {
        let this = current_from_window(&self.window);
        let Some(this) = this else { return };

        {
            let this = Rc::clone(&this);
            let tab_c = Rc::clone(tab);
            tab.close_btn.connect_clicked(move |_| {
                this.close_tab(&tab_c);
            });
        }
        install_editor_tab_menu(&this, tab);
        {
            let this = Rc::clone(&this);
            tab.document.buffer.connect_changed(move |_| {
                this.update_statusbar();
            });
        }
        {
            let this = Rc::clone(&this);
            let buffer = tab.document.buffer.clone();
            buffer.connect_mark_set(move |_, _, _| {
                this.update_statusbar();
            });
        }
        // Clicking into a document view should retarget the terminal cwd.
        {
            let this = Rc::clone(&this);
            let tab_c = Rc::clone(tab);
            let focus = gtk::EventControllerFocus::new();
            focus.connect_enter(move |_| {
                this.sync_terminal_for_tab(&tab_c);
            });
            tab.view.add_controller(focus);
        }
    }

    fn wire_notebook(&self, nb: &Rc<TabNotebook>) {
        let this = current_from_window(&self.window);
        let Some(this) = this else { return };
        let this = Rc::clone(&this);

        // "+" is a Button: clicks call this even when that page is already
        // selected (empty window), which would not emit switch-page.
        {
            let this = Rc::clone(&this);
            let nb_ptr = Rc::clone(nb);
            nb.set_on_plus(move || {
                for (i, g) in this.groups.borrow().iter().enumerate() {
                    if Rc::ptr_eq(g, &nb_ptr) {
                        *this.active_group.borrow_mut() = i;
                        break;
                    }
                }
                this.new_tab();
            });
        }

        let nb_for_switch = Rc::clone(nb);
        nb.notebook.connect_switch_page(move |_, _, page_num| {
            // Capture page_num — during switch-page current_page() can still be old
            // (same idle pattern gtk-files uses for sync_chrome).
            // Never linger on the empty "+" page. Creation itself is handled by
            // the "+" button (avoids double new_tab if both paths fire).
            if nb_for_switch.is_plus_page(page_num) {
                let this = Rc::clone(&this);
                let nb = Rc::clone(&nb_for_switch);
                glib::idle_add_local_once(move || {
                    for (i, g) in this.groups.borrow().iter().enumerate() {
                        if Rc::ptr_eq(g, &nb) {
                            *this.active_group.borrow_mut() = i;
                            break;
                        }
                    }
                    if nb.len() == 0 {
                        this.new_tab();
                    } else if let Some(last) = nb.len().checked_sub(1) {
                        nb.notebook.set_current_page(Some(last as u32));
                    }
                });
                return;
            }
            let this = Rc::clone(&this);
            glib::idle_add_local_once(move || {
                this.update_statusbar();
                this.refresh_documents_panel();
                if let Some(tab) = this.active_notebook().tab_at(page_num) {
                    tab.sync_markdown_preview();
                    this.sync_terminal_for_tab(&tab);
                } else {
                    this.sync_terminal_cwd();
                }
            });
        });
    }

    pub fn close_tab(&self, tab: &Rc<EditorTab>) -> bool {
        if tab.document.is_modified() {
            // Auto-save when a path exists; otherwise prompt Save As.
            if tab.document.path().is_some() {
                if !self.save_tab(tab, false) {
                    return false;
                }
            } else if !self.save_tab(tab, true) {
                return false;
            }
        }

        // Find the notebook that owns this tab (not only the active group).
        let owner = self.groups.borrow().iter().find(|g| {
            g.tabs
                .borrow()
                .iter()
                .any(|t| Rc::ptr_eq(t, tab) || t.page == tab.page)
        }).cloned();

        let Some(nb) = owner else {
            return false;
        };

        nb.remove_tab(tab);
        if nb.len() == 0 {
            // Keep at least one empty tab in the active group.
            if Rc::ptr_eq(&nb, &self.active_notebook()) {
                self.new_tab();
            }
        }
        nb.update_tabs_visibility(&self.config.borrow().ui.notebook_show_tabs_mode);
        self.refresh_documents_panel();
        self.update_statusbar();
        self.engine.update_window_plugins();
        true
    }

    pub fn close_all_tabs(&self) -> bool {
        let tabs: Vec<Rc<EditorTab>> = self
            .groups
            .borrow()
            .iter()
            .flat_map(|g| g.tabs.borrow().clone())
            .collect();
        for tab in tabs {
            if tab.document.is_modified() {
                if tab.document.path().is_some() {
                    if !self.save_tab(&tab, false) {
                        return false;
                    }
                } else if !self.save_tab(&tab, true) {
                    return false;
                }
            }
        }
        true
    }

    pub fn save_tab(&self, tab: &Rc<EditorTab>, save_as: bool) -> bool {
        let cfg = self.config.borrow().editor.clone();
        let path = if save_as || tab.document.path().is_none() {
            None
        } else {
            tab.document.path()
        };

        if let Some(path) = path {
            if let Err(e) = save_path(&tab.document, &path, &cfg) {
                show_io_error(&self.window, "Error saving file", &e);
                return false;
            }
            tab.refresh_title();
            tab.sync_markdown_preview();
            self.config.borrow_mut().add_recent(&path);
            let _ = self.config.borrow().save();
            self.refresh_documents_panel();
            return true;
        }

        // Save As — in-app chooser so it follows the suite theme (portal FileDialog cannot).
        let tab = Rc::clone(tab);
        let this = current_from_window(&self.window);
        let Some(this) = this else {
            return false;
        };
        let parent = this.window.clone();
        let suggested = tab
            .document
            .path()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Untitled.txt".into());
        let folder = tab
            .document
            .path()
            .and_then(|p| p.parent().map(gio::File::for_path));
        gtk_theme::present_file_chooser_at(
            Some(&parent),
            "Save As",
            gtk::FileChooserAction::Save,
            "Save",
            None,
            Some(&suggested),
            folder.as_ref(),
            move |file| {
                let Some(path) = file.and_then(|f| f.path()) else {
                    return;
                };
                let cfg = this.config.borrow().editor.clone();
                if let Err(e) = save_path(&tab.document, &path, &cfg) {
                    show_io_error(&this.window, "Error saving file", &e);
                } else {
                    tab.refresh_title();
                    tab.sync_markdown_preview();
                    this.config.borrow_mut().add_recent(&path);
                    let _ = this.config.borrow().save();
                    this.refresh_documents_panel();
                }
            },
        );
        true
    }

    pub fn refresh_documents_panel(&self) {
        self.docs_panel.clear();
        for group in self.groups.borrow().iter() {
            for tab in group.tabs.borrow().iter() {
                let title = tab.document.title();
                let path = tab
                    .document
                    .path()
                    .map(|p| p.display().to_string());
                let row = self
                    .docs_panel
                    .add_document(&title, path.as_deref());
                let this = current_from_window(&self.window);
                let tab2 = Rc::clone(tab);
                let group2 = Rc::clone(group);
                row.connect_activate(move |_| {
                    if let Some(idx) = group2
                        .tabs
                        .borrow()
                        .iter()
                        .position(|t| Rc::ptr_eq(t, &tab2))
                    {
                        group2.notebook.set_current_page(Some(idx as u32));
                    }
                    if let Some(this) = &this {
                        this.update_statusbar();
                        this.sync_terminal_for_tab(&tab2);
                    }
                });
            }
        }
    }

    pub fn update_statusbar(&self) {
        let Some(tab) = self.current_tab() else {
            return;
        };
        let insert = tab.document.buffer.get_insert();
        let iter = tab.document.buffer.iter_at_mark(&insert);
        self.statusbar
            .set_position(iter.line(), iter.line_offset());
        self.statusbar
            .encoding_label
            .set_text(&tab.document.encoding.borrow());
        let lang_name = tab
            .document
            .buffer
            .language()
            .map(|l| l.name().to_string())
            .unwrap_or_else(|| "Plain Text".into());
        self.statusbar.set_language_label(&lang_name);
        self.statusbar.set_tab_width(tab.view.tab_width());
        self.window
            .set_title(Some(&format!("{} — GTK Edit", tab.document.title())));

        if externally_modified(&tab.document) {
            self.statusbar
                .flash("File changed on disk");
        }
    }

    pub fn apply_config(&self) {
        let cfg = self.config.borrow().clone();
        self.statusbar.root.set_visible(cfg.ui.statusbar_visible);
        self.side_panel
            .set_visible_panel(cfg.ui.side_panel_visible);
        self.bottom_panel
            .set_visible_panel(cfg.ui.bottom_panel_visible);
        if cfg.ui.bottom_panel_visible {
            self.ensure_bottom_panel_width();
        }
        for group in self.groups.borrow().iter() {
            group.update_tabs_visibility(&cfg.ui.notebook_show_tabs_mode);
            for tab in group.tabs.borrow().iter() {
                tab.apply_config(&cfg.editor);
            }
        }
    }

    pub fn new_tab_group(&self) {
        let nb = TabNotebook::new();
        self.wire_notebook(&nb);
        self.groups_box.append(&nb.notebook);
        self.groups.borrow_mut().push(Rc::clone(&nb));
        *self.active_group.borrow_mut() = self.groups.borrow().len() - 1;
        self.new_tab();
    }

    pub fn save_session_state(&self) {
        let mut cfg = self.config.borrow_mut();
        cfg.state.window_width = self.window.default_width();
        cfg.state.window_height = self.window.default_height();
        cfg.state.side_panel_size = self.hpaned.position();
        let total = self.content_paned.height();
        let pos = self.content_paned.position();
        if total > pos + 100 && self.bottom_panel.root.is_visible() {
            cfg.state.bottom_panel_size = total - pos;
        }
        if let Some(id) = self.side_panel.current_page_id() {
            cfg.state.side_panel_page = id;
        }
        let mut files = Vec::new();
        for group in self.groups.borrow().iter() {
            for tab in group.tabs.borrow().iter() {
                if let Some(p) = tab.document.path() {
                    files.push(p.display().to_string());
                }
            }
        }
        cfg.session.open_files = files;
        let _ = cfg.save();
    }
}

pub fn current_from_window(window: &gtk::ApplicationWindow) -> Option<Rc<EditorWindow>> {
    unsafe {
        window
            .data::<Rc<EditorWindow>>(WINDOW_STATE_KEY)
            .map(|p| Rc::clone(p.as_ref()))
    }
}

pub fn current_tab_from_window(window: &gtk::ApplicationWindow) -> Option<Rc<EditorTab>> {
    current_from_window(window)?.current_tab()
}

pub fn open_path_in_window(window: &gtk::ApplicationWindow, path: &Path) {
    if let Some(ew) = current_from_window(window) {
        ew.open_path(path);
    }
}

fn build_menubar(
    tools_menu: &gio::Menu,
    edit_extra: &gio::Menu,
    search_extra: &gio::Menu,
) -> (gio::Menu, gtk_theme::IconMenu) {
    let menubar = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();

    let file = gio::Menu::new();
    icons.append_action(&file, "New", "win.new");
    icons.append_action(&file, "Open…", "win.open");
    icons.append_action(&file, "Save", "win.save");
    icons.append_action(&file, "Save As…", "win.save-as");
    icons.append_action(&file, "Revert", "win.revert");
    icons.append_action(&file, "Print Preview", "win.print-preview");
    icons.append_action(&file, "Print…", "win.print");
    icons.append_action(&file, "Close", "win.close");
    icons.append_action(&file, "Quit", "app.quit");
    icons.append_submenu(
        &menubar,
        "_File",
        &file,
        "text-x-generic-symbolic",
    );

    let edit = gio::Menu::new();
    icons.append_action(&edit, "Undo", "win.undo");
    icons.append_action(&edit, "Redo", "win.redo");
    icons.append_action(&edit, "Cut", "win.cut");
    icons.append_action(&edit, "Copy", "win.copy");
    icons.append_action(&edit, "Paste", "win.paste");
    icons.append_action(&edit, "Delete", "win.delete");
    icons.append_action(&edit, "Select All", "win.select-all");
    edit.append_section(None, edit_extra);
    icons.append_action(&edit, "Preferences…", "win.preferences");
    icons.append_submenu(
        &menubar,
        "_Edit",
        &edit,
        "document-edit-symbolic",
    );

    let view = gio::Menu::new();
    icons.append(
        &view,
        "Statusbar",
        "win.toggle-statusbar",
        "dialog-information-symbolic",
    );
    icons.append(
        &view,
        "Side Panel",
        "win.toggle-side",
        "view-sidebar-start-symbolic",
    );
    icons.append(
        &view,
        "Tools",
        "win.toggle-bottom",
        "view-dual-symbolic",
    );
    icons.append_action(&view, "Fullscreen", "win.fullscreen");
    // Theme profile radios stay plain so stateful indicators remain visible.
    gtk_theme::append_profile_menu(&view, "win.theme");
    icons.append_submenu(
        &menubar,
        "_View",
        &view,
        "view-grid-symbolic",
    );

    let search = gio::Menu::new();
    icons.append_action(&search, "Find…", "win.find");
    icons.append(
        &search,
        "Find Next",
        "win.find-next",
        "go-down-symbolic",
    );
    icons.append(
        &search,
        "Find Previous",
        "win.find-prev",
        "go-up-symbolic",
    );
    icons.append_action(&search, "Replace…", "win.replace");
    icons.append(
        &search,
        "Clear Highlight",
        "win.clear-highlight",
        "edit-clear-symbolic",
    );
    icons.append(
        &search,
        "Go to Line…",
        "win.goto-line",
        "go-jump-symbolic",
    );
    search.append_section(None, search_extra);
    icons.append_submenu(
        &menubar,
        "_Search",
        &search,
        "edit-find-symbolic",
    );

    icons.append_submenu(
        &menubar,
        "_Tools",
        tools_menu,
        "applications-utilities-symbolic",
    );

    let docs = gio::Menu::new();
    icons.append_action(&docs, "Save All", "win.save-all");
    icons.append(
        &docs,
        "Close All",
        "win.close-all",
        "window-close-symbolic",
    );
    icons.append(
        &docs,
        "New Tab Group",
        "win.new-tab-group",
        "tab-new-symbolic",
    );
    icons.append(
        &docs,
        "Previous Document",
        "win.prev-doc",
        "go-previous-symbolic",
    );
    icons.append(
        &docs,
        "Next Document",
        "win.next-doc",
        "go-next-symbolic",
    );
    icons.append(
        &docs,
        "Move to New Window",
        "win.move-to-window",
        "window-new-symbolic",
    );
    icons.append_submenu(
        &menubar,
        "_Documents",
        &docs,
        "x-office-document-symbolic",
    );

    icons.append_action(&menubar, "About", "win.about");

    (menubar, icons)
}

fn install_accels(app: &gtk::Application) {
    let accels = [
        ("win.new", "<Ctrl>n"),
        ("win.open", "<Ctrl>o"),
        ("win.save", "<Ctrl>s"),
        ("win.save-as", "<Ctrl><Shift>s"),
        ("win.close", "<Ctrl>w"),
        ("app.quit", "<Ctrl>q"),
        ("win.undo", "<Ctrl>z"),
        ("win.redo", "<Ctrl><Shift>z"),
        ("win.cut", "<Ctrl>x"),
        ("win.copy", "<Ctrl>c"),
        ("win.paste", "<Ctrl>v"),
        ("win.select-all", "<Ctrl>a"),
        ("win.find", "<Ctrl>f"),
        ("win.find-next", "<Ctrl>g"),
        ("win.find-prev", "<Ctrl><Shift>g"),
        ("win.replace", "<Ctrl>h"),
        ("win.goto-line", "<Ctrl>i"),
        ("win.fullscreen", "F11"),
        ("win.preferences", "<Ctrl>comma"),
        ("win.print", "<Ctrl>p"),
        ("win.next-doc", "<Ctrl>Page_Down"),
        ("win.prev-doc", "<Ctrl>Page_Up"),
        ("win.quick-open", "<Ctrl><Alt>o"),
        ("win.expand-snippet", "<Ctrl>b"),
    ];
    for (action, accel) in accels {
        app.set_accels_for_action(action, &[accel]);
    }
}

fn install_actions(ew: &Rc<EditorWindow>) {
    let win = &ew.window;

    {
        let act = gio::SimpleAction::new_stateful(
            "theme",
            Some(glib::VariantTy::STRING),
            &gtk_theme::load_theme_id().to_variant(),
        );
        act.connect_activate(move |action, param| {
            let Some(id) = param.and_then(|p| p.get::<String>()) else {
                return;
            };
            gtk_theme::select_theme(&id, |_| {});
            action.set_state(&id.to_variant());
        });
        win.add_action(&act);

        // Keep editor scheme in sync when this app — or gtk-term / others — change profile.
        let ew = Rc::clone(ew);
        gtk_theme::watch_theme(move |profile| {
            let mgr = sourceview5::StyleSchemeManager::default();
            let scheme = gtk_theme::resolve_sourceview_scheme(profile.id, |sid| {
                mgr.scheme(sid).is_some()
            });
            // try_borrow: preferences save must not panic if it re-enters via select_theme.
            if let Ok(mut c) = ew.config.try_borrow_mut() {
                c.editor.scheme = scheme.to_string();
                let _ = c.save();
            }
            ew.apply_config();
            if let Some(action) = ew.window.lookup_action("theme") {
                action
                    .downcast_ref::<gio::SimpleAction>()
                    .map(|a| a.set_state(&profile.id.to_variant()));
            }
        });
        gtk_theme::install_open_theme_editor_action(win);
    }

    add_action(win, ew, "new", |ew| {
        ew.new_tab();
    });
    add_action(win, ew, "open", |ew| {
        let ew = Rc::clone(ew);
        let parent = ew.window.clone();
        gtk_theme::present_file_chooser(
            Some(&parent),
            "Open File",
            gtk::FileChooserAction::Open,
            "Open",
            None,
            None,
            move |file| {
                if let Some(path) = file.and_then(|f| f.path()) {
                    ew.open_path(&path);
                }
            },
        );
    });
    add_action(win, ew, "save", |ew| {
        if let Some(tab) = ew.current_tab() {
            ew.save_tab(&tab, false);
        }
    });
    add_action(win, ew, "save-as", |ew| {
        if let Some(tab) = ew.current_tab() {
            ew.save_tab(&tab, true);
        }
    });
    add_action(win, ew, "revert", |ew| {
        if let Some(tab) = ew.current_tab() {
            if let Some(path) = tab.document.path() {
                let cfg = ew.config.borrow().clone();
                let _ = load_path(
                    &tab.document,
                    &path,
                    &cfg.editor,
                    &cfg.encodings.auto_detected,
                );
                tab.refresh_title();
            }
        }
    });
    add_action(win, ew, "close", |ew| {
        if let Some(tab) = ew.current_tab() {
            ew.close_tab(&tab);
        }
    });
    add_action(win, ew, "undo", |ew| {
        if let Some(tab) = ew.current_tab() {
            if tab.document.buffer.can_undo() {
                tab.document.buffer.undo();
            }
        }
    });
    add_action(win, ew, "redo", |ew| {
        if let Some(tab) = ew.current_tab() {
            if tab.document.buffer.can_redo() {
                tab.document.buffer.redo();
            }
        }
    });
    add_action(win, ew, "cut", |ew| {
        if let Some(tab) = ew.current_tab() {
            tab.view.emit_cut_clipboard();
        }
    });
    add_action(win, ew, "copy", |ew| {
        if let Some(tab) = ew.current_tab() {
            tab.view.emit_copy_clipboard();
        }
    });
    add_action(win, ew, "paste", |ew| {
        if let Some(tab) = ew.current_tab() {
            tab.view.emit_paste_clipboard();
        }
    });
    add_action(win, ew, "delete", |ew| {
        if let Some(tab) = ew.current_tab() {
            let buf = &tab.document.buffer;
            if let Some((mut s, mut e)) = buf.selection_bounds() {
                buf.delete(&mut s, &mut e);
            }
        }
    });
    add_action(win, ew, "select-all", |ew| {
        if let Some(tab) = ew.current_tab() {
            let buf = &tab.document.buffer;
            buf.select_range(&buf.start_iter(), &buf.end_iter());
        }
    });
    add_action(win, ew, "find", |ew| {
        let sel = current_selection_text(ew);
        ew.search.show_find_with(sel.as_deref());
    });
    add_action(win, ew, "find-next", |ew| {
        if let Some(tab) = ew.current_tab() {
            let hl = ew.config.borrow().editor.search_highlighting;
            ew.search.find(&tab.document.buffer, &tab.view, true, hl);
        }
    });
    add_action(win, ew, "find-prev", |ew| {
        if let Some(tab) = ew.current_tab() {
            let hl = ew.config.borrow().editor.search_highlighting;
            ew.search.find(&tab.document.buffer, &tab.view, false, hl);
        }
    });
    add_action(win, ew, "replace", |ew| {
        let sel = current_selection_text(ew);
        let dlg = ReplaceDialog::new(&ew.window);
        let cfg = ew.config.borrow().clone();
        dlg.present_with(&cfg, sel.as_deref());
        let ew2 = Rc::clone(ew);
        let dlg2 = Rc::clone(&dlg);
        dlg.find_btn.connect_clicked(move |_| {
            if let Some(tab) = ew2.current_tab() {
                dlg2.find_next(&tab.document.buffer, &tab.view);
                ew2.config
                    .borrow_mut()
                    .push_search_history(&dlg2.search_entry.text());
            }
        });
        let ew2 = Rc::clone(ew);
        let dlg2 = Rc::clone(&dlg);
        dlg.replace_btn.connect_clicked(move |_| {
            if let Some(tab) = ew2.current_tab() {
                dlg2.replace_one(&tab.document.buffer, &tab.view);
                ew2.config
                    .borrow_mut()
                    .push_replace_history(&dlg2.replace_entry.text());
            }
        });
        let ew2 = Rc::clone(ew);
        let dlg2 = Rc::clone(&dlg);
        dlg.replace_all_btn.connect_clicked(move |_| {
            if let Some(tab) = ew2.current_tab() {
                let n = dlg2.replace_all(&tab.document.buffer);
                ew2.statusbar.flash(&format!("Replaced {n} occurrences"));
            }
        });
    });
    add_action(win, ew, "clear-highlight", |ew| {
        if let Some(tab) = ew.current_tab() {
            clear_search_highlights(&tab.document.buffer);
        }
    });
    add_action(win, ew, "goto-line", |ew| {
        ew.search.show_goto_line();
    });
    add_action(win, ew, "toggle-statusbar", |ew| {
        let vis = !ew.statusbar.root.is_visible();
        ew.statusbar.root.set_visible(vis);
        ew.config.borrow_mut().ui.statusbar_visible = vis;
        let _ = ew.config.borrow().save();
    });
    add_action(win, ew, "toggle-side", |ew| {
        let vis = !ew.side_panel.root.is_visible();
        ew.side_panel.set_visible_panel(vis);
        ew.config.borrow_mut().ui.side_panel_visible = vis;
        let _ = ew.config.borrow().save();
    });
    add_action(win, ew, "toggle-bottom", |ew| {
        let vis = !ew.bottom_panel.root.is_visible();
        ew.bottom_panel.set_visible_panel(vis);
        ew.config.borrow_mut().ui.bottom_panel_visible = vis;
        if vis {
            ew.ensure_bottom_panel_height();
        }
        let _ = ew.config.borrow().save();
    });
    add_action(win, ew, "fullscreen", |ew| {
        if ew.window.is_fullscreen() {
            ew.window.unfullscreen();
        } else {
            ew.window.fullscreen();
        }
    });
    add_action(win, ew, "preferences", |ew| {
        let ew2 = Rc::clone(ew);
        prefs::show_preferences(
            &ew.window,
            Rc::clone(&ew.config),
            Rc::clone(&ew.engine),
            Rc::new(move || ew2.apply_config()),
        );
    });
    add_action(win, ew, "print", |ew| {
        if let Some(tab) = ew.current_tab() {
            let cfg = ew.config.borrow().print.clone();
            print::print_document(
                &ew.window,
                &tab.document.buffer,
                &tab.view,
                &tab.document.title(),
                &cfg,
                false,
            );
        }
    });
    add_action(win, ew, "print-preview", |ew| {
        if let Some(tab) = ew.current_tab() {
            let cfg = ew.config.borrow().print.clone();
            print::print_document(
                &ew.window,
                &tab.document.buffer,
                &tab.view,
                &tab.document.title(),
                &cfg,
                true,
            );
        }
    });
    add_action(win, ew, "save-all", |ew| {
        for group in ew.groups.borrow().iter() {
            for tab in group.tabs.borrow().iter() {
                if tab.document.is_modified() && tab.document.path().is_some() {
                    ew.save_tab(tab, false);
                }
            }
        }
    });
    add_action(win, ew, "close-all", |ew| {
        let tabs: Vec<_> = ew
            .active_notebook()
            .tabs
            .borrow()
            .clone();
        for tab in tabs {
            ew.close_tab(&tab);
        }
    });
    add_action(win, ew, "new-tab-group", |ew| {
        ew.new_tab_group();
    });
    add_action(win, ew, "next-doc", |ew| {
        let nb = ew.active_notebook();
        let n = nb.len() as i32;
        if n == 0 {
            return;
        }
        let cur = nb.notebook.current_page().unwrap_or(0) as i32;
        nb.notebook.set_current_page(Some(((cur + 1) % n) as u32));
        ew.update_statusbar();
        ew.sync_terminal_cwd();
    });
    add_action(win, ew, "prev-doc", |ew| {
        let nb = ew.active_notebook();
        let n = nb.len() as i32;
        if n == 0 {
            return;
        }
        let cur = nb.notebook.current_page().unwrap_or(0) as i32;
        nb.notebook
            .set_current_page(Some(((cur - 1 + n) % n) as u32));
        ew.update_statusbar();
        ew.sync_terminal_cwd();
    });
    add_action(win, ew, "move-to-window", |ew| {
        let Some(tab) = ew.current_tab() else { return };
        move_tab_to_new_window(ew, &tab);
    });
    add_action(win, ew, "about", |ew| {
        let dialog = gtk::AboutDialog::builder()
            .transient_for(&ew.window)
            .modal(true)
            .program_name("GTK Edit")
            .version(env!("CARGO_PKG_VERSION"))
            .comments(
                "gtk-edit standalone text editor application, in Rust that edits plain text with GtkSourceView.",
            )
            .authors(["Created by Kevin Hinds"])
            .website("https://github.com/NeuronixOS/GTK-Apps")
            .website_label("github.com/NeuronixOS/GTK-Apps")
            .license_type(gtk::License::Gpl20)
            .build();
        dialog.present();
    });
}

fn add_action(
    win: &gtk::ApplicationWindow,
    ew: &Rc<EditorWindow>,
    name: &str,
    f: impl Fn(&Rc<EditorWindow>) + 'static,
) {
    let action = gio::SimpleAction::new(name, None);
    let ew = Rc::clone(ew);
    action.connect_activate(move |_, _| f(&ew));
    win.add_action(&action);
}

fn move_tab_to_new_window(ew: &Rc<EditorWindow>, tab: &Rc<EditorTab>) {
    let Some(app) = ew.window.application() else {
        return;
    };
    let new_ew = EditorWindow::new(&app, Rc::clone(&ew.config), Rc::clone(&ew.engine));
    if let Some(path) = tab.document.path() {
        new_ew.open_path(&path);
    } else {
        let new_tab = new_ew.new_tab();
        new_tab.document.set_text(&tab.document.text());
        new_tab.refresh_title();
    }
    ew.close_tab(tab);
    new_ew.present();
}

fn install_editor_tab_menu(ew: &Rc<EditorWindow>, tab: &Rc<EditorTab>) {
    let group = gio::SimpleActionGroup::new();
    {
        let ew = Rc::clone(ew);
        let tab = Rc::clone(tab);
        let close = gio::SimpleAction::new("close", None);
        close.connect_activate(move |_, _| {
            ew.close_tab(&tab);
        });
        group.add_action(&close);
    }
    {
        let ew = Rc::clone(ew);
        let tab = Rc::clone(tab);
        let move_act = gio::SimpleAction::new("new-window", None);
        move_act.connect_activate(move |_, _| {
            move_tab_to_new_window(&ew, &tab);
        });
        group.add_action(&move_act);
    }
    tab.tab_box.insert_action_group("edtab", Some(&group));

    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();
    icons.append(
        &menu,
        "Open in New Window",
        "edtab.new-window",
        "window-new-symbolic",
    );
    icons.append(
        &menu,
        "Close Tab",
        "edtab.close",
        "window-close-symbolic",
    );

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    icons.bind_popover(&popover);
    popover.set_parent(&tab.tab_box);
    popover.set_has_arrow(false);
    {
        let popover_weak = popover.downgrade();
        tab.tab_box.connect_destroy(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.unparent();
            }
        });
    }

    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    {
        let popover = popover.clone();
        let ew = Rc::clone(ew);
        let tab = Rc::clone(tab);
        gesture.connect_pressed(move |gesture, _n, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            // Focus this tab before showing the menu.
            for group in ew.groups.borrow().iter() {
                if let Some(idx) = group
                    .tabs
                    .borrow()
                    .iter()
                    .position(|t| Rc::ptr_eq(t, &tab))
                {
                    group.notebook.set_current_page(Some(idx as u32));
                    break;
                }
            }
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
    }
    tab.tab_box.add_controller(gesture);
}

fn wire_search(ew: &Rc<EditorWindow>) {
    let run_search = |ew: &Rc<EditorWindow>, forward: bool| {
        let Some(tab) = ew.current_tab() else {
            return;
        };
        let hl = ew.config.borrow().editor.search_highlighting;
        let ok = ew
            .search
            .find(&tab.document.buffer, &tab.view, forward, hl);
        if ew.search.is_goto_mode() {
            if !ok {
                ew.statusbar
                    .flash("Go to line: enter a line number (optional :column)");
            }
        } else if ok {
            ew.config
                .borrow_mut()
                .push_search_history(&ew.search.entry.text());
        }
    };

    {
        let ew2 = Rc::clone(ew);
        ew.search.next_btn.connect_clicked(move |_| {
            run_search(&ew2, true);
        });
    }
    {
        let ew2 = Rc::clone(ew);
        ew.search.prev_btn.connect_clicked(move |_| {
            run_search(&ew2, false);
        });
    }
    // Enter in the search/goto entry — do not rely on Button::emit_clicked.
    {
        let ew2 = Rc::clone(ew);
        ew.search.entry.connect_activate(move |_| {
            run_search(&ew2, true);
        });
    }
}

fn build_language_menu(ew: &Rc<EditorWindow>) {
    let menu = gio::Menu::new();
    menu.append(Some("Plain Text"), Some("win.lang::none"));
    let lm = sourceview5::LanguageManager::default();
    let mut ids: Vec<String> = lm.language_ids().into_iter().map(|s| s.to_string()).collect();
    ids.sort();
    for id in ids.iter().take(80) {
        if let Some(lang) = lm.language(id) {
            menu.append(
                Some(lang.name().as_str()),
                Some(&format!("win.lang::{id}")),
            );
        }
    }
    ew.statusbar.language_btn.set_menu_model(Some(&menu));

    let action = gio::SimpleAction::new_stateful("lang", Some(&String::static_variant_type()), &"none".to_variant());
    let ew2 = Rc::clone(ew);
    action.connect_activate(move |act, param| {
        let id = param
            .and_then(|v| v.get::<String>())
            .unwrap_or_else(|| "none".into());
        act.set_state(&id.to_variant());
        if let Some(tab) = ew2.current_tab() {
            if id == "none" {
                tab.document.set_language_id(None);
            } else {
                tab.document.set_language_id(Some(&id));
            }
            tab.sync_markdown_preview();
            ew2.update_statusbar();
        }
    });
    ew.window.add_action(&action);
}

fn build_tab_width_menu(ew: &Rc<EditorWindow>) {
    let menu = gio::Menu::new();
    for w in [2u32, 3, 4, 8] {
        menu.append(
            Some(&format!("{w}")),
            Some(&format!("win.tabwidth::{w}")),
        );
    }
    ew.statusbar.tab_width_btn.set_menu_model(Some(&menu));
    let action =
        gio::SimpleAction::new("tabwidth", Some(&u32::static_variant_type()));
    let ew2 = Rc::clone(ew);
    action.connect_change_state(move |act, state| {
        if let Some(v) = state {
            if let Some(w) = v.get::<u32>() {
                act.set_state(v);
                if let Some(tab) = ew2.current_tab() {
                    tab.view.set_tab_width(w);
                    ew2.statusbar.set_tab_width(w);
                }
            }
        }
    });
    // Also handle activate with target
    let ew3 = Rc::clone(ew);
    let action2 = gio::SimpleAction::new("tabwidth", Some(&u32::static_variant_type()));
    action2.connect_activate(move |_, param| {
        if let Some(w) = param.and_then(|v| v.get::<u32>()) {
            if let Some(tab) = ew3.current_tab() {
                tab.view.set_tab_width(w);
                ew3.statusbar.set_tab_width(w);
            }
        }
    });
    ew.window.add_action(&action2);
    let _ = action;
}

pub fn open_files(ew: &EditorWindow, files: &[PathBuf]) {
    if files.is_empty() {
        return;
    }
    // Close the initial untitled if unused
    if let Some(tab) = ew.current_tab() {
        if tab.document.path().is_none() && !tab.document.is_modified() && tab.document.text().is_empty() {
            ew.active_notebook().remove_tab(&tab);
        }
    }
    for f in files {
        ew.open_path(f);
    }
}

/// Non-empty selection from the current tab (capped so huge blocks aren't dumped into Find).
fn current_selection_text(ew: &EditorWindow) -> Option<String> {
    let tab = ew.current_tab()?;
    let buf = &tab.document.buffer;
    let (start, end) = buf.selection_bounds()?;
    let text = buf.text(&start, &end, false);
    if text.is_empty() {
        return None;
    }
    // Avoid stuffing multi-megabyte selections into the find entry.
    const MAX: usize = 2048;
    let s = text.as_str();
    if s.len() > MAX {
        Some(s.chars().take(MAX).collect())
    } else {
        Some(s.to_string())
    }
}

// Autosave timeout
pub fn start_autosave(ew: Rc<EditorWindow>) {
    glib::timeout_add_seconds_local(60, move || {
        let cfg = ew.config.borrow().editor.clone();
        if cfg.auto_save {
            for group in ew.groups.borrow().iter() {
                for tab in group.tabs.borrow().iter() {
                    let _ = io::maybe_autosave(&tab.document, &cfg);
                }
            }
        }
        glib::ControlFlow::Continue
    });
}
