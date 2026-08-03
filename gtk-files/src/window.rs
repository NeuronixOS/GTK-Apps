//! Main application window: sidebar, toolbar, tabs, actions.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::glib::prelude::*;
use gtk::prelude::*;

use crate::clipboard::{self, ClipOp, SharedClipboard};
use crate::config::Config;
use crate::file_ops;
use crate::find_in_files;
use crate::network;
use crate::open_with;
use crate::pathbar::PathBar;
use crate::places;
use crate::prefs;
use crate::properties;
use crate::scripts::{self, ConvertFormat};
use crate::search::SearchBar;
use crate::sidebar::{Place, Sidebar};
use crate::sync_setup;
use crate::sync_status;
use crate::tab::{FolderTab, ViewMode};
use crate::templates;
use crate::terminal_panel::TerminalPanel;
use crate::util::{self, show_error};

pub struct FilesWindow {
    pub window: gtk::ApplicationWindow,
    pub config: Rc<RefCell<Config>>,
    pub(crate) clipboard: SharedClipboard,
    notebook: gtk::Notebook,
    tabs: RefCell<Vec<Rc<FolderTab>>>,
    sidebar: Rc<Sidebar>,
    pathbar: Rc<PathBar>,
    search: Rc<SearchBar>,
    terminal: Rc<TerminalPanel>,
    find_in_files: Rc<find_in_files::FindInFilesPanel>,
    tools_notebook: gtk::Notebook,
    /// Page index of the Find in Files tab in `tools_notebook`.
    find_page: u32,
    back_btn: gtk::Button,
    forward_btn: gtk::Button,
    /// Horizontal: Places sidebar | (files + bottom tools)
    paned: gtk::Paned,
    /// Vertical: file view | bottom tools (terminal / find in files)
    content_paned: gtk::Paned,
    /// Paths captured when a file-view context menu opens (selection can clear
    /// when the popover steals focus).
    context_paths: RefCell<Option<Vec<PathBuf>>>,
    /// Header chip left of search: syncing status + current file names.
    sync_header: gtk::Box,
    sync_header_icon: gtk::Image,
    sync_header_label: gtk::Label,
}

impl FilesWindow {
    pub fn new(
        app: &gtk::Application,
        config: Rc<RefCell<Config>>,
        clipboard: SharedClipboard,
    ) -> Rc<Self> {
        let cfg = config.borrow().clone();
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("GTK Files")
            .default_width(cfg.window.width)
            .default_height(cfg.window.height)
            .build();

        let header = gtk::HeaderBar::new();
        gtk_theme::prepare_headerbar(&header);

        let back_btn = gtk::Button::from_icon_name("go-previous-symbolic");
        back_btn.set_tooltip_text(Some("Back"));
        let forward_btn = gtk::Button::from_icon_name("go-next-symbolic");
        forward_btn.set_tooltip_text(Some("Forward"));
        let up_btn = gtk::Button::from_icon_name("go-up-symbolic");
        up_btn.set_tooltip_text(Some("Parent folder"));

        let nav = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        nav.add_css_class("linked");
        nav.append(&back_btn);
        nav.append(&forward_btn);
        nav.append(&up_btn);

        header.pack_start(&nav);

        let pathbar = PathBar::new();
        // Keep breadcrumbs on the left (not as a centered title widget) so the
        // sync chip on the right can update without shifting folder location.
        pathbar.root.set_hexpand(true);
        pathbar.root.set_halign(gtk::Align::Fill);
        pathbar.root.set_margin_start(8);
        pathbar.root.set_margin_end(8);
        header.pack_start(&pathbar.root);

        let search_btn = gtk::Button::from_icon_name("edit-find-symbolic");
        search_btn.set_tooltip_text(Some("Search current folder"));
        let view_btn = gtk::Button::from_icon_name("view-grid-symbolic");
        view_btn.set_tooltip_text(Some("Toggle list/grid view"));
        let new_folder_btn = gtk::Button::from_icon_name("folder-new-symbolic");
        new_folder_btn.set_tooltip_text(Some("New folder"));

        let sync_header_icon = gtk::Image::from_icon_name("view-refresh-symbolic");
        sync_header_icon.set_pixel_size(16);
        sync_header_icon.set_halign(gtk::Align::Start);
        let sync_header_label = gtk::Label::new(None);
        sync_header_label.add_css_class("caption");
        sync_header_label.add_css_class("sync-header-label");
        sync_header_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        // Fixed character cell so filename changes do not reflow the header.
        sync_header_label.set_width_chars(24);
        sync_header_label.set_max_width_chars(24);
        sync_header_label.set_xalign(0.0);
        sync_header_label.set_halign(gtk::Align::Start);
        let sync_header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        sync_header.add_css_class("sync-header-status");
        sync_header.set_valign(gtk::Align::Center);
        sync_header.set_halign(gtk::Align::End);
        // Icon (16) + spacing (6) + ~24 monospace cells — keep allocation stable.
        sync_header.set_size_request(220, -1);
        sync_header.set_hexpand(false);
        sync_header.append(&sync_header_icon);
        sync_header.append(&sync_header_label);
        sync_header.set_visible(false);

        let (app_menu, app_icons) = build_app_menu();
        let menu_btn = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&app_menu)
            .build();
        app_icons.bind_menu_button(&menu_btn);

        header.pack_end(&menu_btn);
        header.pack_end(&view_btn);
        header.pack_end(&new_folder_btn);
        header.pack_end(&search_btn);
        // Fixed-width chip immediately left of the magnifying-glass.
        header.pack_end(&sync_header);
        window.set_titlebar(Some(&header));

        let sidebar = Sidebar::new();
        let search = SearchBar::new();

        let notebook = gtk::Notebook::new();
        notebook.set_scrollable(true);
        notebook.set_hexpand(true);
        notebook.set_vexpand(true);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.set_hexpand(true);
        content.set_vexpand(true);
        content.append(&search.root);
        content.append(&notebook);

        let terminal = TerminalPanel::new(&util::home_dir());

        // Placeholder; real panel wired after `fw` exists (needs callbacks).
        let tools_notebook = gtk::Notebook::new();
        tools_notebook.set_scrollable(true);
        tools_notebook.set_hexpand(true);
        tools_notebook.set_vexpand(true);
        // Soft floor for the tools strip; shrink-end must be true below or VTE/Find
        // natural heights lock the divider and the handle feels stuck.
        tools_notebook.set_size_request(-1, 120);
        tools_notebook.add_css_class("gtk-content");
        terminal.root.set_hexpand(true);
        terminal.root.set_vexpand(true);
        terminal.root.set_size_request(-1, 80);
        let term_label = gtk::Label::new(Some("Terminal"));
        tools_notebook.append_page(&terminal.root, Some(&term_label));

        // Bottom tools (terminal / find in files) under the file view.
        let content_paned = gtk::Paned::new(gtk::Orientation::Vertical);
        content_paned.set_hexpand(true);
        content_paned.set_vexpand(true);
        content_paned.set_start_child(Some(&content));
        content_paned.set_end_child(Some(&tools_notebook));
        content_paned.set_resize_start_child(true);
        content_paned.set_shrink_start_child(true);
        // Keep bottom height stable on window resize, but allow the user to drag
        // past VTE / Find-in-Files natural size (shrink must be true).
        content_paned.set_resize_end_child(false);
        content_paned.set_shrink_end_child(true);
        content_paned.set_wide_handle(true);

        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_start_child(Some(&sidebar.root));
        paned.set_end_child(Some(&content_paned));
        paned.set_resize_start_child(false);
        paned.set_shrink_start_child(false);
        paned.set_position(cfg.window.sidebar_width);

        window.set_child(Some(&paned));

        // Callbacks filled once `fw` exists (see `fw_slot` below).
        let fw_slot: Rc<RefCell<Option<Rc<FilesWindow>>>> = Rc::new(RefCell::new(None));
        let on_reveal = {
            let slot = Rc::clone(&fw_slot);
            Rc::new(move |path: PathBuf| {
                let Some(fw) = slot.borrow().clone() else {
                    return;
                };
                fw.current_tab().reveal_path(&path);
                fw.sync_chrome();
            }) as Rc<dyn Fn(PathBuf)>
        };
        let on_open_folder = {
            let slot = Rc::clone(&fw_slot);
            Rc::new(move |folder: PathBuf| {
                let Some(fw) = slot.borrow().clone() else {
                    return;
                };
                fw.current_tab().navigate_path(&folder, true);
                fw.sync_chrome();
            }) as Rc<dyn Fn(PathBuf)>
        };
        let find_in_files =
            find_in_files::FindInFilesPanel::new(&window, on_reveal, on_open_folder);
        let find_label = gtk::Label::new(Some("Find in Files"));
        let find_page = tools_notebook.append_page(&find_in_files.root, Some(&find_label));

        let fw = Rc::new(Self {
            window: window.clone(),
            config: Rc::clone(&config),
            clipboard: {
                clipboard::set_active(Rc::clone(&clipboard));
                clipboard
            },
            notebook: notebook.clone(),
            tabs: RefCell::new(Vec::new()),
            sidebar: Rc::clone(&sidebar),
            pathbar: Rc::clone(&pathbar),
            search: Rc::clone(&search),
            terminal: Rc::clone(&terminal),
            find_in_files: Rc::clone(&find_in_files),
            tools_notebook: tools_notebook.clone(),
            find_page,
            back_btn: back_btn.clone(),
            forward_btn: forward_btn.clone(),
            paned: paned.clone(),
            content_paned: content_paned.clone(),
            context_paths: RefCell::new(None),
            sync_header: sync_header.clone(),
            sync_header_icon: sync_header_icon.clone(),
            sync_header_label: sync_header_label.clone(),
        });
        *fw_slot.borrow_mut() = Some(Rc::clone(&fw));
        fw.update_sync_header();

        // Place the divider so the bottom tools keep their configured height.
        {
            let vpaned = content_paned.clone();
            let term_h = cfg.window.terminal_height.max(120);
            window.connect_map(move |_| {
                let vpaned = vpaned.clone();
                glib::idle_add_local_once(move || {
                    let total = vpaned.height();
                    if total > term_h + 160 {
                        vpaned.set_position(total - term_h);
                    }
                });
            });
        }

        fw.install_actions(app);
        install_clipboard_shortcuts(&fw);

        {
            let fw2 = Rc::clone(&fw);
            sidebar.set_on_activate(move |place| {
                fw2.open_place_in_current_tab(&place);
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            sidebar.set_on_open_tab(move |place| {
                fw2.open_place_in_new_tab(&place);
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            sidebar.set_on_open_window(move |place| {
                fw2.open_place_in_new_window(&place);
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            sidebar.set_on_remove_sync_client(move || {
                let fw3 = Rc::clone(&fw2);
                sync_setup::confirm_remove_client(Some(&fw2.window), move || {
                    fw3.sidebar.rebuild();
                    fw3.sidebar.refresh_sync_soon();
                });
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            sidebar.set_on_remove_sync_server(move || {
                let fw3 = Rc::clone(&fw2);
                sync_setup::confirm_remove_server(Some(&fw2.window), move || {
                    fw3.sidebar.refresh_sync_soon();
                });
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            sidebar.set_on_client_status(move || {
                for tab in fw2.tabs.borrow().iter() {
                    tab.refresh_sync_ui();
                }
                fw2.update_sync_header();
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            sync_setup::set_on_setup_progress(move || {
                fw2.sidebar.rebuild();
                fw2.sidebar.refresh_sync_if_changed();
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            pathbar.set_on_navigate(move |path| {
                fw2.current_tab().navigate_path(&path, true);
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            pathbar.set_on_open_tab(move |path| {
                fw2.open_path_in_new_tab(&path);
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            pathbar.set_on_open_window(move |path| {
                fw2.open_path_in_new_window(&path);
            });
        }

        // After pathbar/sidebar callbacks are wired — first tab syncs chrome into them.
        fw.add_tab(Some(gio::File::for_path(util::home_dir())));

        {
            let fw2 = Rc::clone(&fw);
            search.set_on_changed(move |q| {
                fw2.current_tab().set_search_query(q);
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            back_btn.connect_clicked(move |_| {
                fw2.current_tab().go_back();
                fw2.sync_chrome();
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            forward_btn.connect_clicked(move |_| {
                fw2.current_tab().go_forward();
                fw2.sync_chrome();
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            up_btn.connect_clicked(move |_| {
                fw2.current_tab().go_up();
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            search_btn.connect_clicked(move |_| fw2.search.toggle());
        }
        {
            let fw2 = Rc::clone(&fw);
            view_btn.connect_clicked(move |_| {
                fw2.current_tab().toggle_view_mode();
                let mode = fw2.current_tab().view_mode();
                fw2.config.borrow_mut().view.mode = match mode {
                    ViewMode::Grid => "grid".into(),
                    ViewMode::List => "list".into(),
                };
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            new_folder_btn.connect_clicked(move |_| fw2.action_new_folder());
        }
        {
            let fw2 = Rc::clone(&fw);
            notebook.connect_switch_page(move |_, page, _| {
                let fw3 = Rc::clone(&fw2);
                let page = page.clone();
                glib::idle_add_local_once(move || {
                    // Sync terminal to the tab being shown (not only pathbar/sidebar).
                    if let Some(tab) = fw3.tab_for_page(&page) {
                        if let Some(p) = tab.location().path() {
                            fw3.terminal.sync_cwd_force(&p);
                        }
                        // feed_cd can leave focus in the VTE; reclaim after a beat
                        // so the terminal finishes processing input first.
                        let tab_focus = Rc::clone(&tab);
                        glib::timeout_add_local_once(
                            std::time::Duration::from_millis(75),
                            move || {
                                tab_focus.grab_files_focus();
                            },
                        );
                    }
                    fw3.sync_chrome();
                });
            });
        }
        {
            let fw2 = Rc::clone(&fw);
            window.connect_close_request(move |w| {
                let mut c = fw2.config.borrow_mut();
                c.window.width = w.width();
                c.window.height = w.height();
                c.window.sidebar_width = fw2.paned.position();
                c.window.sidebar_visible = true;
                let total = fw2.content_paned.height();
                let pos = fw2.content_paned.position();
                if total > pos + 100 {
                    c.window.terminal_height = total - pos;
                }
                let tab = fw2.current_tab();
                c.view.show_hidden = tab.show_hidden();
                c.view.icon_size = tab.icon_size();
                c.view.thumbnail_size =
                    crate::config::thumbnail_name_for_pixels(tab.icon_size()).into();
                c.view.mode = match tab.view_mode() {
                    ViewMode::Grid => "grid".into(),
                    ViewMode::List => "list".into(),
                };
                c.save();
                glib::Propagation::Proceed
            });
        }

        // Open folder in a new tab; for files, open with the MIME default app.
        {
            let action = gio::SimpleAction::new("open-in-tab", None);
            let fw2 = Rc::clone(&fw);
            action.connect_activate(move |_, _| {
                fw2.action_open_in_tab();
            });
            fw.window.add_action(&action);
        }

        fw.sync_chrome();
        fw
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn add_tab(self: &Rc<Self>, start: Option<gio::File>) -> Rc<FolderTab> {
        // Need Rc<Self> for wiring — callers always have the window in an Rc.
        // Use a lightweight approach: wire via callbacks set below from `new`/`add_tab_rc`.
        let tab = FolderTab::new(&self.config.borrow(), start);

        let title = gtk::Label::new(Some(tab.title.borrow().as_str()));
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");
        close.set_focus_on_click(false);

        let tab_label = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        tab_label.append(&title);
        tab_label.append(&close);

        let page_idx = self.notebook.append_page(&tab.root, Some(&tab_label));
        self.notebook.set_tab_reorderable(&tab.root, true);
        self.notebook.set_current_page(Some(page_idx));

        self.tabs.borrow_mut().push(Rc::clone(&tab));
        install_files_tab_menu(self, &tab_label, &tab.root);

        {
            let fw_self = Rc::clone(self);
            let notebook = self.notebook.clone();
            let root = tab.root.clone();
            close.connect_clicked(move |_| {
                if let Some(idx) = page_index_of(&notebook, &root) {
                    fw_self.close_tab_at(idx);
                }
            });
        }

        {
            let title = title.clone();
            let sidebar = Rc::clone(&self.sidebar);
            let pathbar = Rc::clone(&self.pathbar);
            let back = self.back_btn.clone();
            let forward = self.forward_btn.clone();
            let window = self.window.clone();
            let terminal = Rc::clone(&self.terminal);
            let tab_ref = Rc::clone(&tab);
            tab.set_on_location(move |file| {
                let name = util::title_for_location(&file);
                title.set_text(&name);
                window.set_title(Some(&format!("{name} — GTK Files")));
                if util::is_trash_location(&file) {
                    sidebar.select_trash();
                    pathbar.set_location(Path::new("trash:///"));
                } else if let Some(p) = file.path() {
                    if places::record_recent_folder(&p) {
                        sidebar.rebuild();
                    }
                    sidebar.select_path(&p);
                    pathbar.set_location(&p);
                    terminal.sync_cwd(&p);
                }
                back.set_sensitive(tab_ref.can_back());
                forward.set_sensitive(tab_ref.can_forward());
            });
        }

        {
            let fw = Rc::clone(self);
            let tab2 = Rc::clone(&tab);
            tab.set_on_open(move |file, is_dir| {
                fw.open_activated(&tab2, file, is_dir);
            });
        }

        {
            let fw = Rc::clone(self);
            tab.set_on_context(move |selection, anchor, x, y| {
                show_context_menu(&fw, &anchor, selection, x, y);
            });
        }

        tab
    }

    /// Prefer paths captured for an open context menu; otherwise use the view selection.
    fn target_paths(&self) -> Vec<PathBuf> {
        self.context_paths
            .borrow()
            .clone()
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| self.current_tab().selected_paths())
    }

    pub fn current_tab(&self) -> Rc<FolderTab> {
        // Resolve by notebook page widget — tab reorder would desync a Vec index.
        let page = self.notebook.current_page().unwrap_or(0);
        if let Some(widget) = self.notebook.nth_page(Some(page)) {
            if let Some(tab) = self.tab_for_page(&widget) {
                return tab;
            }
        }
        let tabs = self.tabs.borrow();
        Rc::clone(tabs.last().expect("at least one tab"))
    }

    fn tab_for_page(&self, page: &gtk::Widget) -> Option<Rc<FolderTab>> {
        let tabs = self.tabs.borrow();
        tabs.iter()
            .find(|t| {
                let root: gtk::Widget = t.root.clone().upcast();
                &root == page
            })
            .cloned()
    }

    pub fn sync_chrome(&self) {
        let tab = self.current_tab();
        let file = tab.location();
        self.window.set_title(Some(&format!(
            "{} — GTK Files",
            util::title_for_location(&file)
        )));
        if util::is_trash_location(&file) {
            self.sidebar.select_trash();
            self.pathbar.set_location(Path::new("trash:///"));
        } else if let Some(p) = file.path() {
            self.sidebar.select_path(&p);
            self.pathbar.set_location(&p);
            self.terminal.sync_cwd_force(&p);
        } else {
            // Remote URI (sftp:// …) — show the URI in the path bar.
            let uri = file.uri().to_string();
            self.pathbar.set_location(Path::new(&uri));
        }
        self.back_btn.set_sensitive(tab.can_back());
        self.forward_btn.set_sensitive(tab.can_forward());
        self.update_sync_header();
    }

    /// Update the header-bar sync chip (fixed on the right of the path bar).
    pub fn update_sync_header(&self) {
        let Some(status) = sync_status::load_client_status() else {
            self.sync_header.set_visible(false);
            return;
        };
        // Only show when a client is configured / status file is live.
        if sync_status::client_sync_root().is_none() {
            self.sync_header.set_visible(false);
            return;
        }
        let Some(msg) = status.header_message() else {
            self.sync_header.set_visible(false);
            return;
        };
        // Text is already padded to a fixed width; keep icon stable while busy
        // so the chip allocation does not twitch between files.
        self.sync_header_label.set_text(&msg);
        self.sync_header.set_tooltip_text(Some(&status.header_tooltip()));
        self.sync_header_icon
            .set_icon_name(Some("view-refresh-symbolic"));
        self.sync_header.set_visible(true);
    }

    fn place_to_file(place: &Place) -> gio::File {
        match place {
            Place::Path(p) => gio::File::for_path(p),
            Place::Uri(uri) => gio::File::for_uri(uri),
            Place::Trash => util::trash_file(),
            Place::ConnectNetwork | Place::SetupSync => {
                gio::File::for_path(network::network_home_dir())
            }
        }
    }

    fn open_place_in_current_tab(self: &Rc<Self>, place: &Place) {
        match place {
            Place::ConnectNetwork => {
                self.action_connect_server();
                return;
            }
            Place::SetupSync => {
                self.action_setup_sync();
                return;
            }
            Place::Path(p) => self.current_tab().navigate_path(p, true),
            Place::Uri(uri) => {
                // Mount if needed, then open.
                let fw = Rc::clone(self);
                let uri = uri.clone();
                network::mount_and_open(&self.window, &uri, Rc::new(move |file| {
                    fw.current_tab().navigate(file, true);
                    fw.sidebar.rebuild();
                    fw.sync_chrome();
                }));
                return;
            }
            Place::Trash => self.current_tab().navigate(util::trash_file(), true),
        }
        self.sync_chrome();
    }

    fn open_place_in_new_tab(self: &Rc<Self>, place: &Place) {
        if matches!(place, Place::ConnectNetwork) {
            self.action_connect_server();
            return;
        }
        if matches!(place, Place::SetupSync) {
            self.action_setup_sync();
            return;
        }
        self.add_tab(Some(Self::place_to_file(place)));
        self.sync_chrome();
    }

    fn open_place_in_new_window(self: &Rc<Self>, place: &Place) {
        if matches!(place, Place::ConnectNetwork) {
            self.action_connect_server();
            return;
        }
        if matches!(place, Place::SetupSync) {
            self.action_setup_sync();
            return;
        }
        let Some(app) = self.window.application() else {
            return;
        };
        let new = FilesWindow::new(&app, Rc::clone(&self.config), Rc::clone(&self.clipboard));
        new.current_tab().navigate(Self::place_to_file(place), true);
        new.sync_chrome();
        new.present();
    }

    fn open_path_in_new_tab(self: &Rc<Self>, path: &Path) {
        if !path.exists() && path.to_string_lossy() != "trash:///" {
            return;
        }
        self.add_tab(Some(gio::File::for_path(path)));
        self.sync_chrome();
    }

    fn open_path_in_new_window(&self, path: &Path) {
        if !path.exists() && path.to_string_lossy() != "trash:///" {
            return;
        }
        let Some(app) = self.window.application() else {
            return;
        };
        let new = FilesWindow::new(&app, Rc::clone(&self.config), Rc::clone(&self.clipboard));
        new.current_tab()
            .navigate(gio::File::for_path(path), true);
        new.sync_chrome();
        new.present();
    }

    fn install_actions(self: &Rc<Self>, _app: &gtk::Application) {
        let win = &self.window;

        {
            let act_theme = gio::SimpleAction::new_stateful(
                "theme",
                Some(glib::VariantTy::STRING),
                &gtk_theme::load_theme_id().to_variant(),
            );
            act_theme.connect_activate(move |action, param| {
                let Some(id) = param.and_then(|p| p.get::<String>()) else {
                    return;
                };
                gtk_theme::select_theme(&id, |_| {});
                action.set_state(&id.to_variant());
            });
            win.add_action(&act_theme);

            let fw = Rc::clone(self);
            gtk_theme::watch_theme(move |profile| {
                fw.terminal.apply_theme_profile(profile);
                if let Some(action) = fw.window.lookup_action("theme") {
                    action
                        .downcast_ref::<gio::SimpleAction>()
                        .map(|a| a.set_state(&profile.id.to_variant()));
                }
            });
            gtk_theme::install_open_theme_editor_action(win);
        }

        bind(win, "setup-sync", self, |fw, _, _| {
            fw.action_setup_sync();
        });

        bind(win, "new-tab", self, |fw, _, _| {
            fw.add_tab(Some(fw.current_tab().location()));
        });
        bind(win, "close-tab", self, |fw, _, _| {
            fw.close_current_tab();
        });
        bind(win, "new-window", self, |fw, _, _| {
            let app = fw.window.application().unwrap();
            let new = FilesWindow::new(&app, Rc::clone(&fw.config), Rc::clone(&fw.clipboard));
            new.present();
        });
        bind(win, "go-back", self, |fw, _, _| {
            fw.current_tab().go_back();
            fw.sync_chrome();
        });
        bind(win, "go-forward", self, |fw, _, _| {
            fw.current_tab().go_forward();
            fw.sync_chrome();
        });
        bind(win, "go-up", self, |fw, _, _| {
            fw.current_tab().go_up();
        });
        bind(win, "go-home", self, |fw, _, _| {
            fw.current_tab()
                .navigate(gio::File::for_path(util::home_dir()), true);
        });
        bind(win, "reload", self, |fw, _, _| {
            fw.current_tab().refresh();
        });
        bind(win, "edit-location", self, |fw, _, _| {
            fw.pathbar.show_entry();
        });
        bind(win, "open-folder", self, |fw, _, _| fw.action_open_folder());
        bind(win, "search", self, |fw, _, _| {
            fw.search.toggle();
        });
        bind(win, "find-in-files", self, |fw, _, _| {
            fw.action_find_in_files();
        });
        bind(win, "toggle-view", self, |fw, _, _| {
            fw.current_tab().toggle_view_mode();
        });
        bind(win, "show-hidden", self, |fw, _, _| {
            let tab = fw.current_tab();
            let next = !tab.show_hidden();
            tab.set_show_hidden(next);
            fw.config.borrow_mut().view.show_hidden = next;
        });
        for (name, size) in [
            ("thumb-small", "small"),
            ("thumb-medium", "medium"),
            ("thumb-large", "large"),
            ("thumb-larger", "larger"),
            ("thumb-largest", "largest"),
        ] {
            let size = size.to_string();
            bind(win, name, self, move |fw, _, _| {
                fw.config.borrow_mut().set_thumbnail_size(&size);
                let px = fw.config.borrow().view.icon_size;
                for tab in fw.tabs.borrow().iter() {
                    tab.set_icon_size(px);
                }
                fw.config.borrow().save();
            });
        }
        bind(win, "select-all", self, |fw, _, _| {
            fw.current_tab().select_all();
        });
        bind(win, "new-folder", self, |fw, _, _| fw.action_new_folder());
        bind(win, "new-file", self, |fw, _, _| fw.action_new_file());
        {
            let action = gio::SimpleAction::new(
                "create-from-template",
                Some(glib::VariantTy::STRING),
            );
            let fw = Rc::clone(self);
            action.connect_activate(move |_, param| {
                let Some(name) = param.and_then(|p| p.get::<String>()) else {
                    return;
                };
                fw.action_create_from_template(&name);
            });
            win.add_action(&action);
        }
        bind(win, "cut", self, |fw, _, _| fw.action_cut_copy(ClipOp::Cut));
        bind(win, "copy", self, |fw, _, _| fw.action_cut_copy(ClipOp::Copy));
        bind(win, "copy-name", self, |fw, _, _| fw.action_copy_name());
        bind(win, "copy-path", self, |fw, _, _| fw.action_copy_path());
        bind(win, "copy-name-path", self, |fw, _, _| fw.action_copy_name_path());
        bind(win, "copy-link-target", self, |fw, _, _| fw.action_copy_link_target());
        bind(win, "show-link-target", self, |fw, _, _| fw.action_show_link_target());
        bind(win, "goto-link-target", self, |fw, _, _| fw.action_goto_link_target());
        bind(win, "add-favorite", self, |fw, _, _| fw.action_add_favorite());
        bind(win, "add-bookmark", self, |fw, _, _| fw.action_add_bookmark());
        bind(win, "connect-server", self, |fw, _, _| fw.action_connect_server());
        bind(win, "show-deleted", self, |fw, _, _| fw.action_toggle_show_deleted());
        bind(win, "restore-version", self, |fw, _, _| fw.action_restore_version());
        bind(win, "paste", self, |fw, _, _| fw.action_paste());
        bind(win, "paste-into", self, |fw, _, _| fw.action_paste_into());
        bind(win, "duplicate", self, |fw, _, _| fw.action_duplicate());
        bind(win, "create-link", self, |fw, _, _| fw.action_create_link());
        bind(win, "trash", self, |fw, _, _| fw.action_trash());
        bind(win, "delete", self, |fw, _, _| fw.action_delete());
        bind(win, "rename", self, |fw, _, _| fw.action_rename());
        bind(win, "properties", self, |fw, _, _| fw.action_properties());
        bind(win, "open", self, |fw, _, _| fw.action_open());
        bind(win, "open-with", self, |fw, _, _| fw.action_open_with());
        for format in [
            ConvertFormat::Jpeg,
            ConvertFormat::Png,
            ConvertFormat::Pdf,
            ConvertFormat::Webp,
        ] {
            bind(win, format.action_name(), self, move |fw, _, _| {
                fw.action_convert(format);
            });
        }
        bind(win, "preferences", self, |fw, _, _| {
            let config = Rc::clone(&fw.config);
            let fw2 = Rc::clone(fw);
            prefs::show_preferences(Some(&fw.window), config, move |theme| {
                let cfg = fw2.config.borrow().clone();
                for tab in fw2.tabs.borrow().iter() {
                    tab.apply_config(&cfg);
                }
                if let Some(profile) = theme {
                    fw2.terminal.apply_theme_profile(profile);
                }
            });
        });
        bind(win, "empty-trash", self, |fw, _, _| {
            let tab = fw.current_tab();
            file_ops::empty_trash(Some(&fw.window), move || tab.refresh());
        });

        for (name, key) in [
            ("sort-name", "name"),
            ("sort-size", "size"),
            ("sort-type", "type"),
            ("sort-modified", "modified"),
        ] {
            let key = key.to_string();
            bind(win, name, self, move |fw, _, _| {
                let folders = fw.config.borrow().view.sort_folders_first;
                let rev = fw.config.borrow().view.sort_reversed;
                fw.current_tab().set_sort(&key, folders, rev);
                fw.config.borrow_mut().view.sort_by = key.clone();
            });
        }
    }

    fn close_current_tab(self: &Rc<Self>) {
        let page = self.notebook.current_page().unwrap_or(0);
        self.close_tab_at(page);
    }

    fn close_tab_at(self: &Rc<Self>, page: u32) {
        let page_widget = self.notebook.nth_page(Some(page));
        self.notebook.remove_page(Some(page));
        {
            if let Some(widget) = page_widget {
                self.tabs.borrow_mut().retain(|t| {
                    let root: gtk::Widget = t.root.clone().upcast();
                    root != widget
                });
            }
            if self.tabs.borrow().is_empty() {
                // No permanent home tab — closing the last tab closes the window.
                self.window.close();
            } else {
                self.sync_chrome();
            }
        }
    }

    /// Folders → new tab; files → default app for their MIME type.
    fn action_open_in_tab(self: &Rc<Self>) {
        let paths = self.target_paths();
        if paths.is_empty() {
            return;
        }
        let mut rebuilt = false;
        for path in paths {
            if path.is_dir() {
                self.add_tab(Some(gio::File::for_path(&path)));
            } else if path.is_file() {
                if places::record_recent_for_file(&path) {
                    rebuilt = true;
                }
                util::open_file_default(Some(&self.window), &gio::File::for_path(&path));
            }
        }
        if rebuilt {
            self.sidebar.rebuild();
        }
    }

    /// Move a folder tab into a brand-new window (shell stays on the remaining tabs).
    fn move_tab_to_new_window(self: &Rc<Self>, page: u32) {
        let location = self
            .notebook
            .nth_page(Some(page))
            .and_then(|widget| {
                let tabs = self.tabs.borrow();
                tabs.iter()
                    .find(|t| {
                        let root: gtk::Widget = t.root.clone().upcast();
                        root == widget
                    })
                    .map(|t| t.location())
            });
        let Some(location) = location else {
            return;
        };
        let Some(app) = self.window.application() else {
            return;
        };
        // Don't leave the source window empty without a tab.
        if self.notebook.n_pages() <= 1 {
            let new = FilesWindow::new(&app, Rc::clone(&self.config), Rc::clone(&self.clipboard));
            // Replace the default home tab with this location.
            if let Some(p) = location.path() {
                new.current_tab().navigate_path(&p, true);
            } else {
                new.current_tab().navigate(location, true);
            }
            new.present();
            return;
        }
        self.close_tab_at(page);
        let new = FilesWindow::new(&app, Rc::clone(&self.config), Rc::clone(&self.clipboard));
        if let Some(p) = location.path() {
            new.current_tab().navigate_path(&p, true);
        } else {
            new.current_tab().navigate(location, true);
        }
        new.present();
    }

    fn action_cut_copy(&self, op: ClipOp) {
        let selected = self.current_tab().selected_paths();
        let context = self
            .context_paths
            .borrow()
            .clone()
            .filter(|p| !p.is_empty())
            .unwrap_or_default();
        let mut paths = selected;
        if paths.is_empty() {
            paths = context;
        }
        if paths.is_empty() {
            return;
        }
        clipboard::set_files(&self.clipboard, paths, op, &self.window);
        self.refresh_clipboard_visuals();
    }

    fn refresh_clipboard_visuals(&self) {
        for tab in self.tabs.borrow().iter() {
            tab.refresh_clipboard_ui();
        }
    }

    fn action_copy_name(&self) {
        let paths = self.target_paths();
        if paths.is_empty() {
            return;
        }
        let names = paths.iter().filter_map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
        });
        self.window
            .clipboard()
            .set_text(&places::quoted_list(names));
    }

    fn action_copy_path(&self) {
        let paths = self.target_paths();
        if paths.is_empty() {
            return;
        }
        let items = paths.iter().map(|p| p.display().to_string());
        self.window
            .clipboard()
            .set_text(&places::quoted_list(items));
    }

    fn action_copy_name_path(&self) {
        let paths = self.target_paths();
        if paths.is_empty() {
            return;
        }
        // Full paths, quoted and space-separated:
        // "/path/to/a" "/path/to/b"
        let items = paths.iter().map(|p| p.display().to_string());
        self.window
            .clipboard()
            .set_text(&places::quoted_list(items));
    }

    /// Copy a symlink's destination path (as stored in the link) to the clipboard.
    fn action_copy_link_target(&self) {
        let paths = self.target_paths();
        let Some(path) = paths.first() else {
            return;
        };
        let Some(target) = util::symlink_target(path) else {
            show_error(
                Some(&self.window),
                "Copy Link Target",
                "This item is not a symbolic link.",
            );
            return;
        };
        self.window
            .clipboard()
            .set_text(&target.to_string_lossy());
    }

    /// Show a dialog with the link's raw target and its resolved absolute path.
    fn action_show_link_target(&self) {
        let paths = self.target_paths();
        let Some(path) = paths.first() else {
            return;
        };
        let Some(raw) = util::symlink_target(path) else {
            show_error(
                Some(&self.window),
                "Show Link Target",
                "This item is not a symbolic link.",
            );
            return;
        };
        let resolved = util::resolved_symlink_target(path);
        let link_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let mut detail = format!("{link_name}\n\nLink target:\n{}", raw.display());
        if let Some(resolved) = resolved {
            if resolved != raw {
                detail.push_str(&format!("\n\nResolved path:\n{}", resolved.display()));
            }
            if !resolved.exists() {
                detail.push_str("\n\n⚠ Target does not exist (broken link).");
            }
        }
        let dialog = gtk::AlertDialog::builder()
            .modal(true)
            .message("Symbolic Link")
            .detail(detail)
            .buttons(["OK"])
            .build();
        dialog.show(Some(&self.window));
    }

    /// Navigate to a symlink's target: directories open, files are revealed.
    fn action_goto_link_target(&self) {
        let paths = self.target_paths();
        let Some(path) = paths.first() else {
            return;
        };
        let Some(resolved) = util::resolved_symlink_target(path) else {
            show_error(
                Some(&self.window),
                "Go to Link Target",
                "This item is not a symbolic link.",
            );
            return;
        };
        if !resolved.exists() {
            show_error(
                Some(&self.window),
                "Go to Link Target",
                &format!("Target does not exist:\n{}", resolved.display()),
            );
            return;
        }
        if resolved.is_dir() {
            self.current_tab().navigate_path(&resolved, true);
        } else {
            self.current_tab().reveal_path(&resolved);
        }
        self.sync_chrome();
    }

    fn action_add_favorite(&self) {
        let paths = self.target_paths();
        let mut added = false;
        for path in paths {
            let target = if path.is_dir() {
                path
            } else {
                path.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or(path)
            };
            if places::add_favorite(&target) {
                added = true;
            }
        }
        if added {
            self.sidebar.rebuild();
        }
    }

    fn action_add_bookmark(&self) {
        let paths = self.target_paths();
        let mut added = false;
        for path in paths {
            let target = if path.is_dir() {
                path
            } else {
                path.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or(path)
            };
            if places::add_bookmark(&target) {
                added = true;
            }
        }
        if added {
            self.sidebar.rebuild();
        }
    }

    fn action_connect_server(self: &Rc<Self>) {
        let fw = Rc::clone(self);
        network::show_network_picker(
            &self.window,
            Rc::new(move |file| {
                fw.current_tab().navigate(file, true);
                fw.sidebar.rebuild();
                fw.sync_chrome();
            }),
        );
    }

    fn action_setup_sync(self: &Rc<Self>) {
        let folder = self.current_tab().location_path();
        sync_setup::launch_setup_sync(Some(&self.window), folder.as_deref());
        self.sidebar.refresh_sync_soon();
    }

    fn action_toggle_show_deleted(&self) {
        let tab = self.current_tab();
        let Some(dir) = tab.location_path() else {
            return;
        };
        if !sync_status::is_under_sync_root(&dir) {
            show_error(
                Some(&self.window),
                "Show deleted",
                "Open a folder inside the gtk-sync client sync root first.",
            );
            return;
        }
        tab.set_show_deleted(!tab.show_deleted());
    }

    fn action_restore_version(self: &Rc<Self>) {
        let paths = self.target_paths();
        let Some(path) = paths.first() else {
            show_error(
                Some(&self.window),
                "Restore",
                "Select a file in the sync folder.",
            );
            return;
        };
        if sync_status::path_under_sync_root(path).is_none() {
            show_error(
                Some(&self.window),
                "Restore",
                "This path is not inside the active gtk-sync client folder.",
            );
            return;
        }
        let fw = Rc::clone(self);
        sync_status::show_restore_dialog(&self.window, path, move || {
            fw.current_tab().refresh();
            fw.current_tab().refresh_sync_ui();
            fw.sidebar.refresh_sync_soon();
        });
    }

    /// Paste into the folder currently shown in the tab (Ctrl+V / Paste).
    fn action_paste(&self) {
        let tab = self.current_tab();
        let Some(dir) = tab.location_path() else {
            show_error(Some(&self.window), "Paste", "Cannot paste into this location");
            return;
        };
        self.paste_to(&dir, &tab);
    }

    /// Paste into a single right-clicked folder (Paste Into Folder).
    fn action_paste_into(&self) {
        let tab = self.current_tab();
        let targets = self.target_paths();
        let Some(dir) = targets.into_iter().next().filter(|p| p.is_dir()) else {
            show_error(
                Some(&self.window),
                "Paste Into Folder",
                "Select a folder to paste into",
            );
            return;
        };
        self.paste_to(&dir, &tab);
    }

    fn paste_to(&self, dir: &Path, tab: &Rc<FolderTab>) {
        let clip = Rc::clone(&self.clipboard);
        let window = self.window.clone();
        let refresh_tab = Rc::clone(tab);
        let dest = dir.to_path_buf();
        if !clipboard::is_empty(&clip) {
            let tabs_snap: Vec<Rc<FolderTab>> = self.tabs.borrow().iter().cloned().collect();
            file_ops::paste_into(Some(&window), &dest, &clip, move || {
                refresh_tab.refresh();
                for t in &tabs_snap {
                    t.refresh_clipboard_ui();
                }
            });
            return;
        }
        // Fall back to system clipboard (files copied from other apps).
        let window2 = window.clone();
        clipboard::read_paths_from_gdk(&window, move |paths| {
            if paths.is_empty() {
                show_error(
                    Some(&window2),
                    "Paste",
                    "Nothing to paste — copy or cut files first",
                );
                return;
            }
            file_ops::drop_into(Some(&window2), &dest, &paths, false, move || {
                refresh_tab.refresh();
            });
        });
    }

    fn action_duplicate(&self) {
        let tab = self.current_tab();
        for path in self.target_paths() {
            if let Err(e) = file_ops::duplicate(&path) {
                show_error(Some(&self.window), "Duplicate failed", &e);
            }
        }
        tab.refresh();
    }

    fn action_create_link(&self) {
        let tab = self.current_tab();
        let Some(dir) = tab.location_path() else {
            show_error(Some(&self.window), "Create Link", "Cannot create link here");
            return;
        };
        for path in self.target_paths() {
            if let Err(e) = file_ops::create_link(&path, &dir) {
                show_error(Some(&self.window), "Create Link failed", &e);
            }
        }
        tab.refresh();
    }

    fn action_trash(&self) {
        let tab = self.current_tab();
        let paths = self.target_paths();
        let confirm = self.config.borrow().behavior.confirm_trash;
        file_ops::trash_paths(Some(&self.window), &paths, confirm, move || tab.refresh());
    }

    fn action_delete(&self) {
        let tab = self.current_tab();
        let paths = self.target_paths();
        let confirm = self.config.borrow().behavior.confirm_delete;
        file_ops::delete_permanent(Some(&self.window), &paths, confirm, move || tab.refresh());
    }

    fn action_open_folder(&self) {
        let parent = self.window.clone();
        let tab = self.current_tab();
        let start = tab.location();
        let sidebar = Rc::clone(&self.sidebar);
        gtk_theme::present_file_chooser_at(
            Some(&parent),
            "Open Folder",
            gtk::FileChooserAction::SelectFolder,
            "Open",
            None,
            None,
            Some(&start),
            move |file| {
                let Some(file) = file else {
                    return;
                };
                if let Some(p) = file.path() {
                    if places::record_recent_folder(&p) {
                        sidebar.rebuild();
                    }
                    sidebar.select_path(&p);
                }
                tab.navigate(file, true);
            },
        );
    }

    /// Open paths from the context menu or current selection.
    /// One folder → navigate this tab; multiple folders → one new tab each;
    /// files → MIME default application.
    fn action_open(self: &Rc<Self>) {
        self.open_paths(&self.target_paths());
    }

    fn open_paths(self: &Rc<Self>, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        let dirs: Vec<&PathBuf> = paths.iter().filter(|p| p.is_dir()).collect();
        let files: Vec<&PathBuf> = paths.iter().filter(|p| p.is_file()).collect();

        if dirs.len() > 1 {
            for d in dirs {
                self.add_tab(Some(gio::File::for_path(d)));
            }
        } else if let Some(d) = dirs.first() {
            self.current_tab()
                .navigate(gio::File::for_path(d), true);
        }

        let mut rebuilt = false;
        for f in files {
            if places::record_recent_for_file(f) {
                rebuilt = true;
            }
            util::open_file_default(Some(&self.window), &gio::File::for_path(f));
        }
        if rebuilt {
            self.sidebar.rebuild();
        }
    }

    /// Activate (Enter / double-click): multi-folder selection opens each in a new tab.
    fn open_activated(self: &Rc<Self>, tab: &Rc<FolderTab>, file: gio::File, is_dir: bool) {
        if is_dir {
            let multi_dirs = tab.selected_paths().iter().filter(|p| p.is_dir()).count() > 1;
            if multi_dirs {
                self.add_tab(Some(file));
            } else {
                tab.navigate(file, true);
            }
            return;
        }
        if let Some(p) = file.path() {
            if places::record_recent_for_file(&p) {
                self.sidebar.rebuild();
            }
        }
        util::open_file_default(Some(&self.window), &file);
    }

    fn action_open_with(&self) {
        let paths = self.target_paths();
        if paths.is_empty() {
            return;
        }
        open_with::show_open_with(Some(&self.window), &paths);
    }

    fn action_convert(&self, format: ConvertFormat) {
        let paths = self.target_paths();
        if paths.is_empty() {
            return;
        }
        let n = scripts::convert_files(Some(&self.window), &paths, format);
        if n > 0 {
            self.current_tab().refresh();
        }
    }

    fn action_find_in_files(self: &Rc<Self>) {
        let directory = self
            .current_tab()
            .location_path()
            .unwrap_or_else(util::home_dir);
        self.find_in_files.set_directory(&directory);
        self.tools_notebook.set_current_page(Some(self.find_page));
        // Ensure the bottom tools strip is visible / tall enough to use.
        let paned = self.content_paned.clone();
        let saved = self.config.borrow().window.terminal_height.max(180);
        glib::idle_add_local_once(move || {
            let total = paned.height();
            if total > saved + 160 {
                let pos = paned.position();
                if total - pos < 140 {
                    paned.set_position(total - saved);
                }
            }
        });
        self.find_in_files.focus_search();
    }

    fn action_properties(&self) {
        let paths = self.target_paths();
        if !paths.is_empty() {
            properties::show_properties(Some(&self.window), &paths);
        } else if let Some(p) = self.current_tab().location_path() {
            properties::show_properties(Some(&self.window), &[p]);
        }
    }

    fn action_rename(&self) {
        let tab = self.current_tab();
        let paths = self.target_paths();
        let Some(path) = paths.first().cloned() else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let dialog = gtk::Window::builder()
            .title("Rename")
            .modal(true)
            .transient_for(&self.window)
            .default_width(360)
            .build();
        gtk_theme::style_dialog(&dialog);
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 12);
        vbox.set_margin_start(16);
        vbox.set_margin_end(16);
        vbox.set_margin_top(16);
        vbox.set_margin_bottom(16);
        let entry = gtk::Entry::builder().text(&name).hexpand(true).build();
        entry.select_region(0, -1);
        vbox.append(&gtk::Label::new(Some("New name:")));
        vbox.append(&entry);
        let error = gtk::Label::new(None);
        error.add_css_class("error");
        error.add_css_class("dim-label");
        error.set_wrap(true);
        error.set_xalign(0.0);
        error.set_visible(false);
        vbox.append(&error);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        let cancel =
            gtk_theme::labeled_button(gtk_theme::icon_for_label("Cancel"), "Cancel");
        let ok = gtk_theme::labeled_button(gtk_theme::icon_for_label("Rename"), "Rename");
        ok.add_css_class("suggested-action");
        buttons.append(&cancel);
        buttons.append(&ok);
        vbox.append(&buttons);
        dialog.set_child(Some(&vbox));

        {
            let d = dialog.clone();
            cancel.connect_clicked(move |_| d.close());
        }

        let rename_once = Rc::new(RefCell::new(Some((
            path,
            entry.clone(),
            dialog.clone(),
            error.clone(),
            Rc::clone(&tab),
        ))));

        let run = {
            let rename_once = Rc::clone(&rename_once);
            move || {
                let taken = rename_once.borrow_mut().take();
                let Some((path, entry, dialog, error, tab)) = taken else {
                    return;
                };
                let new_name = entry.text().to_string();
                entry.remove_css_class("error");
                error.set_visible(false);
                match file_ops::rename(&path, &new_name) {
                    Ok(_) => {
                        error.set_visible(false);
                        dialog.close();
                        // Defer refresh so the modal tear-down finishes first —
                        // refreshing mid-dialog has crashed the directory model.
                        glib::idle_add_local_once(move || {
                            tab.refresh();
                        });
                    }
                    Err(e) => {
                        error.set_text(&e);
                        error.set_visible(true);
                        entry.add_css_class("error");
                        entry.grab_focus();
                        entry.select_region(0, -1);
                        // Put state back so the user can fix the name and retry.
                        *rename_once.borrow_mut() =
                            Some((path, entry, dialog, error, tab));
                    }
                }
            }
        };
        {
            let run = run.clone();
            ok.connect_clicked(move |_| run());
        }
        {
            entry.connect_activate(move |_| run());
        }
        dialog.present();
        entry.grab_focus();
    }

    fn action_new_folder(&self) {
        self.prompt_new_item("New Folder", "untitled folder", true);
    }

    fn action_new_file(&self) {
        self.prompt_new_item("New Document", "Untitled Document", false);
    }

    /// Destination for "Create New File": right-clicked folder, else current location.
    fn template_dest_dir(&self) -> Option<PathBuf> {
        let targets = self.target_paths();
        if targets.len() == 1 && targets[0].is_dir() {
            return Some(targets[0].clone());
        }
        self.current_tab().location_path()
    }

    fn action_create_from_template(&self, template_name: &str) {
        let Some(dest_dir) = self.template_dest_dir() else {
            show_error(
                Some(&self.window),
                "Create New File",
                "Cannot create a file here",
            );
            return;
        };
        let src = templates::templates_dir().join(template_name);
        match templates::create_from_template(&src, &dest_dir) {
            Ok(_) => self.current_tab().refresh(),
            Err(e) => show_error(Some(&self.window), "Create New File", &e),
        }
    }

    fn prompt_new_item(&self, title: &str, default_name: &str, is_folder: bool) {
        let tab = self.current_tab();
        let Some(dir) = tab.location_path() else {
            show_error(Some(&self.window), title, "Cannot create here");
            return;
        };

        let dialog = gtk::Window::builder()
            .title(title)
            .modal(true)
            .transient_for(&self.window)
            .default_width(360)
            .build();
        gtk_theme::style_dialog(&dialog);
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 12);
        vbox.set_margin_start(16);
        vbox.set_margin_end(16);
        vbox.set_margin_top(16);
        vbox.set_margin_bottom(16);
        let entry = gtk::Entry::builder()
            .text(default_name)
            .hexpand(true)
            .build();
        entry.select_region(0, -1);
        vbox.append(&entry);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        let cancel =
            gtk_theme::labeled_button(gtk_theme::icon_for_label("Cancel"), "Cancel");
        let ok = gtk_theme::labeled_button(gtk_theme::icon_for_label("Create"), "Create");
        ok.add_css_class("suggested-action");
        buttons.append(&cancel);
        buttons.append(&ok);
        vbox.append(&buttons);
        dialog.set_child(Some(&vbox));

        {
            let d = dialog.clone();
            cancel.connect_clicked(move |_| d.close());
        }

        let state = Rc::new(RefCell::new(Some((
            dir,
            entry.clone(),
            dialog.clone(),
            self.window.clone(),
            Rc::clone(&tab),
            is_folder,
        ))));

        let run = {
            let state = Rc::clone(&state);
            move || {
                if let Some((dir, entry, dialog, window, tab, is_folder)) =
                    state.borrow_mut().take()
                {
                    let name = entry.text().to_string();
                    let res = if is_folder {
                        file_ops::create_folder(&dir, &name)
                    } else {
                        file_ops::create_empty_file(&dir, &name)
                    };
                    match res {
                        Ok(_) => {
                            tab.refresh();
                            dialog.close();
                        }
                        Err(e) => {
                            *state.borrow_mut() =
                                Some((dir, entry, dialog, window.clone(), tab, is_folder));
                            show_error(Some(&window), "Could not create", &e);
                        }
                    }
                }
            }
        };
        {
            let run = run.clone();
            ok.connect_clicked(move |_| run());
        }
        {
            entry.connect_activate(move |_| run());
        }
        dialog.present();
        entry.grab_focus();
    }
}

fn bind<F>(win: &gtk::ApplicationWindow, name: &str, fw: &Rc<FilesWindow>, f: F)
where
    F: Fn(&Rc<FilesWindow>, &gio::SimpleAction, Option<&glib::Variant>) + 'static,
{
    let action = gio::SimpleAction::new(name, None);
    let fw = Rc::clone(fw);
    action.connect_activate(move |a, v| f(&fw, a, v));
    win.add_action(&action);
}

/// Ctrl+C/X/V/A for the file view.
///
/// Dual path: ShortcutController (Managed) + EventControllerKey (Capture) so
/// copy/paste still works when focus is on the tree, list, or other chrome.
fn install_clipboard_shortcuts(fw: &Rc<FilesWindow>) {
    let controller = gtk::ShortcutController::new();
    controller.set_scope(gtk::ShortcutScope::Managed);
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);

    add_clip_shortcut(&controller, fw, "<Control>c", |fw| {
        if focus_is_text_field(fw) {
            return glib::Propagation::Proceed;
        }
        if fw.current_tab().selected_paths().is_empty() {
            return glib::Propagation::Proceed;
        }
        fw.action_cut_copy(ClipOp::Copy);
        glib::Propagation::Stop
    });
    add_clip_shortcut(&controller, fw, "<Control>x", |fw| {
        if focus_is_text_field(fw) {
            return glib::Propagation::Proceed;
        }
        if fw.current_tab().selected_paths().is_empty() {
            return glib::Propagation::Proceed;
        }
        fw.action_cut_copy(ClipOp::Cut);
        glib::Propagation::Stop
    });
    add_clip_shortcut(&controller, fw, "<Control>v", |fw| {
        if focus_is_text_field(fw) {
            return glib::Propagation::Proceed;
        }
        fw.action_paste();
        glib::Propagation::Stop
    });
    add_clip_shortcut(&controller, fw, "<Control>a", |fw| {
        if focus_is_text_field(fw) || focus_is_terminal(fw) {
            return glib::Propagation::Proceed;
        }
        fw.current_tab().select_all();
        glib::Propagation::Stop
    });

    fw.window.add_controller(controller);

    // Raw key capture as a fallback when ShortcutController misses the event.
    let key = gtk::EventControllerKey::new();
    key.set_propagation_phase(gtk::PropagationPhase::Capture);
    let fw_keys = Rc::clone(fw);
    key.connect_key_pressed(move |_, keyval, _keycode, state| {
        let mods = state.intersection(gtk::accelerator_get_default_mod_mask());
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        if !(ctrl && !shift) {
            return glib::Propagation::Proceed;
        }
        if !matches!(
            keyval,
            gdk::Key::c
                | gdk::Key::C
                | gdk::Key::v
                | gdk::Key::V
                | gdk::Key::x
                | gdk::Key::X
                | gdk::Key::a
                | gdk::Key::A
        ) {
            return glib::Propagation::Proceed;
        }

        if focus_is_text_field(&fw_keys) {
            return glib::Propagation::Proceed;
        }

        match keyval {
            gdk::Key::c | gdk::Key::C => {
                if fw_keys.current_tab().selected_paths().is_empty() {
                    return glib::Propagation::Proceed;
                }
                fw_keys.action_cut_copy(ClipOp::Copy);
                glib::Propagation::Stop
            }
            gdk::Key::x | gdk::Key::X => {
                if fw_keys.current_tab().selected_paths().is_empty() {
                    return glib::Propagation::Proceed;
                }
                fw_keys.action_cut_copy(ClipOp::Cut);
                glib::Propagation::Stop
            }
            gdk::Key::v | gdk::Key::V => {
                fw_keys.action_paste();
                glib::Propagation::Stop
            }
            gdk::Key::a | gdk::Key::A => {
                if focus_is_terminal(&fw_keys) {
                    return glib::Propagation::Proceed;
                }
                fw_keys.current_tab().select_all();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    fw.window.add_controller(key);
}

fn add_clip_shortcut<F>(controller: &gtk::ShortcutController, fw: &Rc<FilesWindow>, trigger: &str, f: F)
where
    F: Fn(&FilesWindow) -> glib::Propagation + 'static,
{
    let Some(trigger) = gtk::ShortcutTrigger::parse_string(trigger) else {
        return;
    };
    let fw = Rc::clone(fw);
    let action = gtk::CallbackAction::new(move |_, _| f(&fw));
    controller.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
}

fn focus_widget(fw: &FilesWindow) -> Option<gtk::Widget> {
    gtk::prelude::GtkWindowExt::focus(&fw.window)
}

fn focus_is_terminal(fw: &FilesWindow) -> bool {
    focus_widget(fw).is_some_and(|f| is_descendant_of(&f, fw.terminal.root.upcast_ref()))
}

fn focus_is_text_field(fw: &FilesWindow) -> bool {
    let Some(focus) = focus_widget(fw) else {
        return false;
    };
    // Path bar / search / rename — not the VTE (that is handled separately).
    focus.ancestor(gtk::Entry::static_type()).is_some()
        || focus.ancestor(gtk::Text::static_type()).is_some()
        || focus.ancestor(gtk::TextView::static_type()).is_some()
}

fn is_descendant_of(widget: &impl IsA<gtk::Widget>, ancestor: &gtk::Widget) -> bool {
    let mut current = Some(widget.clone().upcast::<gtk::Widget>());
    while let Some(w) = current {
        if w == *ancestor {
            return true;
        }
        current = w.parent();
    }
    false
}

fn install_files_tab_menu(
    fw: &Rc<FilesWindow>,
    tab_label: &gtk::Box,
    page_root: &impl IsA<gtk::Widget>,
) {
    let group = gio::SimpleActionGroup::new();
    {
        let fw = Rc::clone(fw);
        let root = page_root.clone().upcast::<gtk::Widget>();
        let close = gio::SimpleAction::new("close", None);
        close.connect_activate(move |_, _| {
            if let Some(idx) = page_index_of(&fw.notebook, &root) {
                fw.close_tab_at(idx);
            }
        });
        group.add_action(&close);
    }
    {
        let fw = Rc::clone(fw);
        let root = page_root.clone().upcast::<gtk::Widget>();
        let move_act = gio::SimpleAction::new("new-window", None);
        move_act.connect_activate(move |_, _| {
            if let Some(idx) = page_index_of(&fw.notebook, &root) {
                fw.move_tab_to_new_window(idx);
            }
        });
        group.add_action(&move_act);
    }
    tab_label.insert_action_group("ftab", Some(&group));

    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();
    icons.append_action(&menu, "Open in New Window", "ftab.new-window");
    icons.append_action(&menu, "Close Tab", "ftab.close");

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    icons.bind_popover(&popover);
    popover.set_parent(tab_label);
    popover.set_has_arrow(false);
    {
        let popover_weak = popover.downgrade();
        tab_label.connect_destroy(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.unparent();
            }
        });
    }

    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    {
        let popover = popover.clone();
        let fw = Rc::clone(fw);
        let root = page_root.clone().upcast::<gtk::Widget>();
        gesture.connect_pressed(move |gesture, _n, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(idx) = page_index_of(&fw.notebook, &root) {
                fw.notebook.set_current_page(Some(idx));
            }
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
    }
    tab_label.add_controller(gesture);
}

fn page_index_of(notebook: &gtk::Notebook, widget: &impl IsA<gtk::Widget>) -> Option<u32> {
    let n = notebook.n_pages();
    for i in 0..n {
        if let Some(page) = notebook.nth_page(Some(i)) {
            if page == *widget.upcast_ref::<gtk::Widget>() {
                return Some(i);
            }
        }
    }
    None
}

fn build_app_menu() -> (gio::Menu, gtk_theme::IconMenu) {
    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();

    let file = gio::Menu::new();
    icons.append_action(&file, "New Window", "win.new-window");
    icons.append_action(&file, "New Tab", "win.new-tab");
    icons.append_action(&file, "New Folder…", "win.new-folder");
    icons.append_action(&file, "New Document", "win.new-file");
    icons.append_action(&file, "Open Folder…", "win.open-folder");
    icons.append_action(&file, "Open With...", "win.open-with");
    icons.append_action(&file, "Close Tab", "win.close-tab");
    menu.append_submenu(Some("_File"), &file);

    let edit = gio::Menu::new();
    icons.append_action(&edit, "Cut", "win.cut");
    icons.append_action(&edit, "Copy", "win.copy");
    icons.append_action(&edit, "Copy Name(s)", "win.copy-name");
    icons.append_action(&edit, "Copy Path", "win.copy-path");
    icons.append_action(&edit, "Copy Name(s) and Path(s)", "win.copy-name-path");
    icons.append_action(&edit, "Paste", "win.paste");
    icons.append_action(&edit, "Duplicate", "win.duplicate");
    icons.append_action(&edit, "Create Link", "win.create-link");
    icons.append_action(&edit, "Add to Favorites", "win.add-favorite");
    icons.append_action(&edit, "Add Bookmark", "win.add-bookmark");
    icons.append_action(&edit, "Select All", "win.select-all");
    icons.append_action(&edit, "Rename…", "win.rename");
    icons.append_action(&edit, "Move to Trash", "win.trash");
    icons.append_action(&edit, "Delete Permanently…", "win.delete");
    icons.append_action(&edit, "Preferences…", "win.preferences");
    menu.append_submenu(Some("_Edit"), &edit);

    let view = gio::Menu::new();
    icons.append_action(&view, "Reload", "win.reload");
    icons.append_action(&view, "Toggle List/Grid", "win.toggle-view");
    // Toggle item: plain append so GTK can show a checkmark.
    view.append(Some("Show Hidden Files"), Some("win.show-hidden"));
    icons.append_action(&view, "Search…", "win.search");
    icons.append_action(&view, "Find in Files…", "win.find-in-files");
    // Radio leaves stay plain; only the submenu parent gets an icon.
    let thumbs = gio::Menu::new();
    thumbs.append(Some("Small"), Some("win.thumb-small"));
    thumbs.append(Some("Medium"), Some("win.thumb-medium"));
    thumbs.append(Some("Large"), Some("win.thumb-large"));
    thumbs.append(Some("Larger"), Some("win.thumb-larger"));
    thumbs.append(Some("Largest"), Some("win.thumb-largest"));
    icons.append_submenu(&view, "Thumbnail Size", &thumbs, "image-x-generic-symbolic");
    let sort = gio::Menu::new();
    sort.append(Some("By Name"), Some("win.sort-name"));
    sort.append(Some("By Size"), Some("win.sort-size"));
    sort.append(Some("By Type"), Some("win.sort-type"));
    sort.append(Some("By Modified"), Some("win.sort-modified"));
    icons.append_submenu(&view, "Sort", &sort, "view-sort-ascending-symbolic");
    gtk_theme::append_profile_menu(&view, "win.theme");
    menu.append_submenu(Some("_View"), &view);

    let go = gio::Menu::new();
    icons.append_action(&go, "Back", "win.go-back");
    icons.append_action(&go, "Forward", "win.go-forward");
    icons.append_action(&go, "Parent Folder", "win.go-up");
    icons.append_action(&go, "Home", "win.go-home");
    icons.append_action(&go, "Enter Location…", "win.edit-location");
    icons.append_action(&go, "Connect to Server…", "win.connect-server");
    icons.append(
        &go,
        "Setup Sync…",
        "win.setup-sync",
        "list-add-symbolic",
    );
    menu.append_submenu(Some("_Go"), &go);

    icons.append_action(&menu, "Keyboard Shortcuts", "app.shortcuts");
    icons.append_action(&menu, "About", "app.about");

    (menu, icons)
}

fn show_context_menu(
    fw: &Rc<FilesWindow>,
    anchor: &impl IsA<gtk::Widget>,
    selection: Option<Vec<PathBuf>>,
    x: f64,
    y: f64,
) {
    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();
    if let Some(paths) = selection.as_ref() {
        let open = gio::Menu::new();
        icons.append_action(&open, "Open", "win.open");
        // Show Open With for files (folders keep the item; dialog explains).
        if paths.iter().any(|p| p.is_file()) || !paths.is_empty() {
            icons.append_action(&open, "Open With...", "win.open-with");
        }
        // Folders → new tab; files → MIME default app (same action).
        let open_tab_label = if paths.iter().all(|p| p.is_dir()) {
            "Open in New Tab"
        } else if paths.iter().all(|p| p.is_file()) {
            "Open with Default Application"
        } else {
            "Open in New Tab / Default App"
        };
        icons.append_action(&open, open_tab_label, "win.open-in-tab");
        menu.append_section(None, &open);

        let edit = gio::Menu::new();
        icons.append_action(&edit, "Cut", "win.cut");
        icons.append_action(&edit, "Copy", "win.copy");
        icons.append_action(&edit, "Copy Name(s)", "win.copy-name");
        icons.append_action(&edit, "Copy Path", "win.copy-path");
        icons.append_action(&edit, "Copy Name(s) and Path(s)", "win.copy-name-path");
        icons.append_action(&edit, "Paste", "win.paste");
        if paths.len() == 1 && paths[0].is_dir() {
            icons.append_action(&edit, "Paste Into Folder", "win.paste-into");
        }
        icons.append_action(&edit, "Duplicate", "win.duplicate");
        icons.append_action(&edit, "Create Link", "win.create-link");
        icons.append_action(&edit, "Add to Favorites", "win.add-favorite");
        icons.append_action(&edit, "Add Bookmark", "win.add-bookmark");
        icons.append_action(&edit, "Rename...", "win.rename");
        menu.append_section(None, &edit);

        if paths.len() == 1 && paths[0].is_dir() {
            append_create_new_file_menu(&menu, &mut icons);
        }

        if paths.iter().any(|p| p.is_file()) {
            let scripts_menu = gio::Menu::new();
            icons.append_action(&scripts_menu, "Convert to JPEG", "win.convert-to-jpeg");
            icons.append_action(&scripts_menu, "Convert to PNG", "win.convert-to-png");
            icons.append_action(&scripts_menu, "Convert to PDF", "win.convert-to-pdf");
            icons.append_action(&scripts_menu, "Convert to WebP", "win.convert-to-webp");
            icons.append_submenu(&menu, "Scripts", &scripts_menu, "system-run-symbolic");
        }

        // Symbolic-link actions only make sense for a single link.
        if paths.len() == 1 && util::is_symlink_path(&paths[0]) {
            let link = gio::Menu::new();
            icons.append_action(&link, "Copy Link Target", "win.copy-link-target");
            icons.append_action(&link, "Show Link Target", "win.show-link-target");
            icons.append_action(&link, "Go to Link Target", "win.goto-link-target");
            menu.append_section(None, &link);
        }

        let under_sync = paths
            .iter()
            .any(|p| sync_status::path_under_sync_root(p).is_some());
        if under_sync {
            let sync = gio::Menu::new();
            if paths.len() == 1 {
                let label = if sync_status::path_is_deleted_folder(&paths[0]) {
                    "Restore Deleted Folder…"
                } else if sync_status::path_is_sync_deleted(&paths[0]) || !paths[0].exists()
                {
                    "Restore Deleted…"
                } else {
                    "Restore Previous Version…"
                };
                icons.append_action(&sync, label, "win.restore-version");
            }
            let show_label = if fw.current_tab().show_deleted() {
                "Hide Deleted"
            } else {
                "Show Deleted"
            };
            icons.append_action(&sync, show_label, "win.show-deleted");
            menu.append_section(None, &sync);
        }

        let danger = gio::Menu::new();
        icons.append_action(&danger, "Move to Trash", "win.trash");
        icons.append_action(&danger, "Delete Permanently...", "win.delete");
        icons.append_action(&danger, "Properties", "win.properties");
        menu.append_section(None, &danger);
    } else {
        icons.append_action(&menu, "New Folder...", "win.new-folder");
        append_create_new_file_menu(&menu, &mut icons);
        icons.append_action(&menu, "Paste", "win.paste");
        icons.append_action(&menu, "Select All", "win.select-all");
        icons.append_action(&menu, "Properties", "win.properties");
        icons.append_action(&menu, "Empty Trash...", "win.empty-trash");
        if fw
            .current_tab()
            .location_path()
            .map(|p| sync_status::is_under_sync_root(&p))
            .unwrap_or(false)
        {
            let sync = gio::Menu::new();
            let show_label = if fw.current_tab().show_deleted() {
                "Hide Deleted"
            } else {
                "Show Deleted"
            };
            icons.append_action(&sync, show_label, "win.show-deleted");
            menu.append_section(None, &sync);
        }
    }

    // Keep the popover alive; dropping it immediately destroys the menu.
    thread_local! {
        static ACTIVE: RefCell<Option<gtk::PopoverMenu>> = const { RefCell::new(None) };
    }

    ACTIVE.with(|slot| {
        if let Some(old) = slot.borrow_mut().take() {
            old.popdown();
            old.unparent();
        }

        // Set after dismissing any previous menu so its `closed` handler cannot
        // clear the paths for this new invocation.
        *fw.context_paths.borrow_mut() = selection.clone();

        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        icons.bind_popover(&popover);
        popover.set_has_arrow(false);
        popover.set_autohide(true);

        // PopoverMenu is its own native surface; explicitly re-export the window
        // action map so custom IconMenu rows can resolve `win.*`.
        popover.insert_action_group("win", Some(fw.window.upcast_ref::<gio::ActionGroup>()));

        let parent: gtk::Widget = fw.window.clone().upcast();
        let (px, py) = anchor
            .translate_coordinates(&parent, x, y)
            .unwrap_or((x, y));

        popover.set_parent(&parent);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
            px.round() as i32,
            py.round() as i32,
            1,
            1,
        )));
        // Prefer the side with more free space so a tall menu near the bottom
        // opens upward instead of being crushed into a scrolled strip.
        popover.set_position(popover_position_for_point(&parent, px, py));
        popover.set_halign(gtk::Align::Start);

        let fw_close = Rc::clone(fw);
        popover.connect_closed(move |_| {
            // Defer clear so Actionable activation still sees context_paths.
            let fw = Rc::clone(&fw_close);
            glib::idle_add_local_once(move || {
                *fw.context_paths.borrow_mut() = None;
            });
        });

        popover.popup();
        *slot.borrow_mut() = Some(popover);
    });
}

/// Prefer Top when there isn’t enough room below for a tall file context menu.
fn popover_position_for_point(parent: &impl IsA<gtk::Widget>, _px: f64, py: f64) -> gtk::PositionType {
    // Approximate height of the file context menu (Open/Edit/Scripts/Trash…).
    const MENU_HINT_PX: f64 = 420.0;
    let height = parent.as_ref().height().max(1) as f64;
    let space_above = py.max(0.0);
    let space_below = (height - py).max(0.0);
    if space_below < MENU_HINT_PX && space_above > space_below {
        gtk::PositionType::Top
    } else {
        gtk::PositionType::Bottom
    }
}

/// Submenu of files from `~/Templates`.
fn append_create_new_file_menu(menu: &gio::Menu, icons: &mut gtk_theme::IconMenu) {
    let templates = templates::list_templates();
    if templates.is_empty() {
        icons.append_action(menu, "New Document", "win.new-file");
        return;
    }
    let sub = gio::Menu::new();
    for path in templates {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let detailed =
            gio::Action::print_detailed_name("win.create-from-template", Some(&name.to_variant()));
        icons.append_action(&sub, &name, &detailed);
    }
    icons.append_submenu(menu, "Create New File", &sub, "document-new-symbolic");
}
