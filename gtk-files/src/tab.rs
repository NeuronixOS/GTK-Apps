//! Folder tab: expandable tree list + grid views, filter, and sort.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::config::Config;
use crate::dnd;
use crate::thumbnails;
use crate::util::{
    self, can_write, content_type_description, display_name, format_mtime, format_size,
    icon_for_info, is_directory, is_hidden, title_for_location, FILE_ATTRIBUTES,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Grid,
}

pub struct FolderTab {
    pub root: gtk::Box,
    pub title: RefCell<String>,
    directory: gtk::DirectoryList,
    filter: gtk::CustomFilter,
    sorter: gtk::CustomSorter,
    /// Flat sorted model lives inside tree + grid selections (not stored).
    tree_selection: gtk::MultiSelection,
    grid_selection: gtk::MultiSelection,
    /// Anchor for Shift+click ranges (list / grid).
    tree_anchor: Rc<RefCell<Option<u32>>>,
    grid_anchor: Rc<RefCell<Option<u32>>>,
    stack: gtk::Stack,
    pub list_view: gtk::ColumnView,
    pub grid_view: gtk::GridView,
    status: gtk::Label,
    location: RefCell<gio::File>,
    history: RefCell<Vec<gio::File>>,
    history_index: RefCell<usize>,
    search_query: RefCell<String>,
    show_hidden: RefCell<bool>,
    sort_by: RefCell<String>,
    sort_folders_first: RefCell<bool>,
    sort_reversed: RefCell<bool>,
    view_mode: RefCell<ViewMode>,
    single_click: RefCell<bool>,
    icon_size: Rc<RefCell<i32>>,
    /// Shared with TreeListModel create_func for nested folders.
    tree_show_hidden: Rc<RefCell<bool>>,
    tree_search_query: Rc<RefCell<String>>,
    tree_sort_by: Rc<RefCell<String>>,
    tree_sort_folders_first: Rc<RefCell<bool>>,
    tree_sort_reversed: Rc<RefCell<bool>>,
    on_open: Rc<RefCell<Option<Rc<dyn Fn(gio::File, bool)>>>>,
    on_location: RefCell<Option<Rc<dyn Fn(gio::File)>>>,
    on_context: Rc<RefCell<Option<Rc<dyn Fn(Option<Vec<PathBuf>>, gtk::Widget, f64, f64)>>>>,
}

impl FolderTab {
    pub fn new(config: &Config, start: Option<gio::File>) -> Rc<Self> {
        let start_file = start.unwrap_or_else(|| gio::File::for_path(util::home_dir()));

        let directory = gtk::DirectoryList::new(Some(FILE_ATTRIBUTES), Some(&start_file));
        directory.set_monitored(true);

        let filter = gtk::CustomFilter::new(|_| true);
        let filter_model = gtk::FilterListModel::new(Some(directory.clone()), Some(filter.clone()));

        let sorter = gtk::CustomSorter::new(|_a, _b| gtk::Ordering::Equal);
        let flat_model = gtk::SortListModel::new(Some(filter_model), Some(sorter.clone()));

        let show_hidden_rc = Rc::new(RefCell::new(config.view.show_hidden));
        let search_query_rc = Rc::new(RefCell::new(String::new()));
        let sort_by_rc = Rc::new(RefCell::new(config.view.sort_by.clone()));
        let sort_folders_first_rc = Rc::new(RefCell::new(config.view.sort_folders_first));
        let sort_reversed_rc = Rc::new(RefCell::new(config.view.sort_reversed));

        let tree_model = {
            let show_hidden = Rc::clone(&show_hidden_rc);
            let search_query = Rc::clone(&search_query_rc);
            let sort_by = Rc::clone(&sort_by_rc);
            let sort_folders_first = Rc::clone(&sort_folders_first_rc);
            let sort_reversed = Rc::clone(&sort_reversed_rc);
            gtk::TreeListModel::new(flat_model.clone(), false, false, move |obj| {
                let Some(info) = obj.downcast_ref::<gio::FileInfo>() else {
                    return None;
                };
                if !is_directory(info) {
                    return None;
                }
                let file = file_from_info(info)?;
                Some(make_child_model(
                    &file,
                    *show_hidden.borrow(),
                    search_query.borrow().clone(),
                    sort_by.borrow().clone(),
                    *sort_folders_first.borrow(),
                    *sort_reversed.borrow(),
                ))
            })
        };

        let tree_selection = gtk::MultiSelection::new(Some(tree_model));
        let grid_selection = gtk::MultiSelection::new(Some(flat_model.clone()));
        let tree_anchor = Rc::new(RefCell::new(None));
        let grid_anchor = Rc::new(RefCell::new(None));

        let on_open: Rc<RefCell<Option<Rc<dyn Fn(gio::File, bool)>>>> =
            Rc::new(RefCell::new(None));
        let on_context: Rc<RefCell<Option<Rc<dyn Fn(Option<Vec<PathBuf>>, gtk::Widget, f64, f64)>>>> =
            Rc::new(RefCell::new(None));
        let on_refresh: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

        let icon_size = Rc::new(RefCell::new(config.view.icon_size));

        let list_view = build_tree_column_view(
            &tree_selection,
            Rc::clone(&tree_anchor),
            Rc::clone(&on_open),
            Rc::clone(&on_context),
            Rc::clone(&on_refresh),
        );
        let grid_view = build_grid_view(
            &grid_selection,
            Rc::clone(&grid_anchor),
            Rc::clone(&icon_size),
            Rc::clone(&on_open),
            Rc::clone(&on_context),
            Rc::clone(&on_refresh),
        );

        list_view.set_single_click_activate(false);
        grid_view.set_single_click_activate(false);
        list_view.set_enable_rubberband(false);
        grid_view.set_enable_rubberband(false);

        let list_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&list_view)
            .build();
        list_scroll.add_css_class("files-view");
        let grid_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&grid_view)
            .build();
        grid_scroll.add_css_class("files-view");

        let stack = gtk::Stack::new();
        stack.add_css_class("files-view");
        stack.add_named(&list_scroll, Some("list"));
        stack.add_named(&grid_scroll, Some("grid"));

        let mode = if config.is_grid() {
            ViewMode::Grid
        } else {
            ViewMode::List
        };
        stack.set_visible_child_name(if mode == ViewMode::Grid { "grid" } else { "list" });

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.add_css_class("dim-label");
        status.set_margin_start(8);
        status.set_margin_end(8);
        status.set_margin_top(2);
        status.set_margin_bottom(2);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&stack);
        root.append(&status);

        let tab = Rc::new(Self {
            root,
            title: RefCell::new(title_for_location(&start_file)),
            directory,
            filter,
            sorter,
            tree_selection,
            grid_selection,
            tree_anchor,
            grid_anchor,
            stack,
            list_view,
            grid_view,
            status,
            location: RefCell::new(start_file.clone()),
            history: RefCell::new(vec![start_file]),
            history_index: RefCell::new(0),
            search_query: RefCell::new(String::new()),
            show_hidden: RefCell::new(config.view.show_hidden),
            sort_by: RefCell::new(config.view.sort_by.clone()),
            sort_folders_first: RefCell::new(config.view.sort_folders_first),
            sort_reversed: RefCell::new(config.view.sort_reversed),
            view_mode: RefCell::new(mode),
            single_click: RefCell::new(config.behavior.single_click),
            icon_size,
            tree_show_hidden: show_hidden_rc,
            tree_search_query: search_query_rc,
            tree_sort_by: sort_by_rc,
            tree_sort_folders_first: sort_folders_first_rc,
            tree_sort_reversed: sort_reversed_rc,
            on_open,
            on_location: RefCell::new(None),
            on_context,
        });

        tab.reinstall_filter();
        tab.reinstall_sorter();
        // Clicks are wired per-row in the factories; keep activate as Enter-key backup.
        tab.bind_activation();
        tab.bind_selection_changed();
        tab.update_status();

        {
            let tab2 = Rc::clone(&tab);
            *on_refresh.borrow_mut() = Some(Rc::new(move || tab2.refresh()));
        }

        // Drop onto empty space in the current folder view.
        {
            let tab2 = Rc::clone(&tab);
            dnd::attach_drop_target(
                &list_scroll,
                move || tab2.location_path(),
                {
                    let tab2 = Rc::clone(&tab);
                    move || tab2.refresh()
                },
            );
        }
        {
            let tab2 = Rc::clone(&tab);
            dnd::attach_drop_target(
                &grid_scroll,
                move || tab2.location_path(),
                {
                    let tab2 = Rc::clone(&tab);
                    move || tab2.refresh()
                },
            );
        }

        // Right-click empty view space → background menu (New Folder, Paste, …).
        // Row gestures Claim button-3, so this only runs off-item. Attach to both
        // the scrolled window and the view so blank areas still receive clicks.
        attach_empty_context_menu(&list_scroll, &tab);
        attach_empty_context_menu(&grid_scroll, &tab);
        attach_empty_context_menu(&tab.list_view, &tab);
        attach_empty_context_menu(&tab.grid_view, &tab);

        {
            let tab2 = Rc::clone(&tab);
            tab.directory
                .connect_notify_local(Some("loading"), move |_, _| {
                    tab2.update_status();
                });
        }

        tab
    }

    pub fn set_on_open<F: Fn(gio::File, bool) + 'static>(&self, f: F) {
        *self.on_open.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_on_location<F: Fn(gio::File) + 'static>(&self, f: F) {
        *self.on_location.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_on_context<F: Fn(Option<Vec<PathBuf>>, gtk::Widget, f64, f64) + 'static>(&self, f: F) {
        *self.on_context.borrow_mut() = Some(Rc::new(f));
    }

    pub fn location(&self) -> gio::File {
        self.location.borrow().clone()
    }

    pub fn location_path(&self) -> Option<PathBuf> {
        self.location.borrow().path()
    }

    #[allow(dead_code)]
    pub fn is_trash(&self) -> bool {
        util::is_trash_location(&self.location.borrow())
    }

    pub fn navigate(&self, file: gio::File, push_history: bool) {
        // Skip reload when already here (sidebar may fire selected + activated).
        if let (Some(cur), Some(next)) = (self.location.borrow().path(), file.path()) {
            let cur_c = cur.canonicalize().unwrap_or(cur);
            let next_c = next.canonicalize().unwrap_or(next);
            if cur_c == next_c
                && !util::is_trash_location(&file)
                && !util::is_trash_location(&self.location.borrow())
            {
                if let Some(cb) = self.on_location.borrow().as_ref() {
                    cb(file);
                }
                return;
            }
        }

        if push_history {
            let mut hist = self.history.borrow_mut();
            let mut idx = self.history_index.borrow_mut();
            hist.truncate(*idx + 1);
            hist.push(file.clone());
            *idx = hist.len() - 1;
        }
        *self.location.borrow_mut() = file.clone();
        *self.title.borrow_mut() = title_for_location(&file);
        // Clear then set: TreeListModel can keep showing the previous folder when
        // DirectoryList.set_file is called with only the new location (sidebar
        // Recent / Home clicks updated the terminal via on_location but not the list).
        self.directory.set_file(None::<&gio::File>);
        self.directory.set_file(Some(&file));
        self.tree_selection.unselect_all();
        self.grid_selection.unselect_all();
        *self.tree_anchor.borrow_mut() = None;
        *self.grid_anchor.borrow_mut() = None;
        if let Some(cb) = self.on_location.borrow().as_ref() {
            cb(file);
        }
        self.update_status();
    }

    pub fn navigate_path(&self, path: &Path, push_history: bool) {
        if path.to_string_lossy() == "trash:///" || path.starts_with("trash:") {
            self.navigate(util::trash_file(), push_history);
        } else {
            self.navigate(gio::File::for_path(path), push_history);
        }
    }

    pub fn go_back(&self) -> bool {
        let idx = *self.history_index.borrow();
        if idx == 0 {
            return false;
        }
        *self.history_index.borrow_mut() = idx - 1;
        let file = self.history.borrow()[idx - 1].clone();
        self.navigate(file, false);
        true
    }

    pub fn go_forward(&self) -> bool {
        let idx = *self.history_index.borrow();
        let len = self.history.borrow().len();
        if idx + 1 >= len {
            return false;
        }
        *self.history_index.borrow_mut() = idx + 1;
        let file = self.history.borrow()[idx + 1].clone();
        self.navigate(file, false);
        true
    }

    pub fn go_up(&self) -> bool {
        let loc = self.location();
        if let Some(parent) = loc.parent() {
            self.navigate(parent, true);
            true
        } else {
            false
        }
    }

    pub fn can_back(&self) -> bool {
        *self.history_index.borrow() > 0
    }

    pub fn can_forward(&self) -> bool {
        *self.history_index.borrow() + 1 < self.history.borrow().len()
    }

    pub fn set_view_mode(&self, mode: ViewMode) {
        *self.view_mode.borrow_mut() = mode;
        self.stack
            .set_visible_child_name(if mode == ViewMode::Grid { "grid" } else { "list" });
    }

    pub fn toggle_view_mode(&self) {
        let next = if *self.view_mode.borrow() == ViewMode::List {
            ViewMode::Grid
        } else {
            ViewMode::List
        };
        self.set_view_mode(next);
    }

    pub fn view_mode(&self) -> ViewMode {
        *self.view_mode.borrow()
    }

    pub fn set_show_hidden(&self, show: bool) {
        *self.show_hidden.borrow_mut() = show;
        *self.tree_show_hidden.borrow_mut() = show;
        self.reinstall_filter();
        self.update_status();
    }

    pub fn show_hidden(&self) -> bool {
        *self.show_hidden.borrow()
    }

    pub fn set_search_query(&self, q: String) {
        *self.search_query.borrow_mut() = q.clone();
        *self.tree_search_query.borrow_mut() = q;
        self.reinstall_filter();
        self.update_status();
    }

    pub fn set_sort(&self, by: &str, folders_first: bool, reversed: bool) {
        *self.sort_by.borrow_mut() = by.to_string();
        *self.sort_folders_first.borrow_mut() = folders_first;
        *self.sort_reversed.borrow_mut() = reversed;
        *self.tree_sort_by.borrow_mut() = by.to_string();
        *self.tree_sort_folders_first.borrow_mut() = folders_first;
        *self.tree_sort_reversed.borrow_mut() = reversed;
        self.reinstall_sorter();
    }

    pub fn set_icon_size(&self, size: i32) {
        *self.icon_size.borrow_mut() = size;
        // Rebind visible grid items with the new size.
        self.refresh();
    }

    pub fn icon_size(&self) -> i32 {
        *self.icon_size.borrow()
    }

    pub fn apply_config(&self, config: &Config) {
        *self.single_click.borrow_mut() = config.behavior.single_click;
        self.set_show_hidden(config.view.show_hidden);
        self.set_sort(
            &config.view.sort_by,
            config.view.sort_folders_first,
            config.view.sort_reversed,
        );
        self.set_icon_size(config.view.icon_size);
        self.set_view_mode(if config.is_grid() {
            ViewMode::Grid
        } else {
            ViewMode::List
        });
    }

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        match *self.view_mode.borrow() {
            ViewMode::List => selected_paths_tree(&self.tree_selection),
            ViewMode::Grid => selected_paths_flat(&self.grid_selection, &self.location.borrow()),
        }
    }

    pub fn selected_files(&self) -> Vec<gio::File> {
        self.selected_paths()
            .into_iter()
            .map(|p| gio::File::for_path(p))
            .collect()
    }

    pub fn select_all(&self) {
        match *self.view_mode.borrow() {
            ViewMode::List => {
                self.tree_selection.select_all();
            }
            ViewMode::Grid => {
                self.grid_selection.select_all();
            }
        }
    }

    /// Navigate to the file’s parent folder and select it when the listing loads.
    pub fn reveal_path(self: &Rc<Self>, path: &Path) {
        let Some(parent) = path.parent() else {
            return;
        };
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
        else {
            return;
        };
        let already_there = self
            .location_path()
            .as_ref()
            .is_some_and(|p| p == parent);
        if !already_there {
            self.navigate_path(parent, true);
        }
        let tab = Rc::clone(self);
        let target = path.to_path_buf();
        let mut attempts = 0u32;
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            attempts += 1;
            if tab.try_select_name(&name, Some(&target)) || attempts >= 40 {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    fn try_select_name(&self, name: &str, full_path: Option<&Path>) -> bool {
        match *self.view_mode.borrow() {
            ViewMode::List => {
                let n = self.tree_selection.n_items();
                for i in 0..n {
                    let Some(item) = self.tree_selection.item(i) else {
                        continue;
                    };
                    let Some(info) = info_from_selection_item(&item) else {
                        continue;
                    };
                    if display_name(&info) != name {
                        continue;
                    }
                    if let Some(want) = full_path {
                        if let Some(file) = resolve_file(&info, None) {
                            if file.path().as_deref() != Some(want) {
                                continue;
                            }
                        }
                    }
                    self.tree_selection.unselect_all();
                    self.tree_selection.select_item(i, true);
                    *self.tree_anchor.borrow_mut() = Some(i);
                    return true;
                }
                false
            }
            ViewMode::Grid => {
                let n = self.grid_selection.n_items();
                let loc = self.location.borrow().clone();
                for i in 0..n {
                    let Some(info) = self.grid_selection.item(i).and_downcast::<gio::FileInfo>()
                    else {
                        continue;
                    };
                    if display_name(&info) != name {
                        continue;
                    }
                    if let Some(want) = full_path {
                        if let Some(file) = resolve_file(&info, Some(&loc)) {
                            if file.path().as_deref() != Some(want) {
                                continue;
                            }
                        }
                    }
                    self.grid_selection.unselect_all();
                    self.grid_selection.select_item(i, true);
                    *self.grid_anchor.borrow_mut() = Some(i);
                    return true;
                }
                false
            }
        }
    }

    pub fn refresh(&self) {
        let file = self.location();
        self.directory.set_file(None::<&gio::File>);
        self.directory.set_file(Some(&file));
        self.update_status();
    }

    /// Focus the visible file view (list or grid). Used after tab switches so
    /// keyboard shortcuts are not trapped by the terminal after cwd sync.
    pub fn grab_files_focus(&self) {
        match *self.view_mode.borrow() {
            ViewMode::List => {
                self.list_view.set_can_focus(true);
                self.list_view.grab_focus();
            }
            ViewMode::Grid => {
                self.grid_view.set_can_focus(true);
                self.grid_view.grab_focus();
            }
        }
    }

    fn reinstall_filter(&self) {
        let show_hidden = *self.show_hidden.borrow();
        let query = self.search_query.borrow().to_lowercase();
        self.filter.set_filter_func(move |obj| {
            let Some(info) = obj.downcast_ref::<gio::FileInfo>() else {
                return false;
            };
            if !show_hidden && is_hidden(info) {
                return false;
            }
            if !query.is_empty() {
                let name = display_name(info).to_lowercase();
                if !name.contains(&query) {
                    return false;
                }
            }
            true
        });
        self.filter.changed(gtk::FilterChange::Different);
    }

    fn reinstall_sorter(&self) {
        let sort_by = self.sort_by.borrow().clone();
        let folders_first = *self.sort_folders_first.borrow();
        let reversed = *self.sort_reversed.borrow();
        self.sorter.set_sort_func(move |a, b| compare_infos(a, b, &sort_by, folders_first, reversed));
        self.sorter.changed(gtk::SorterChange::Different);
    }

    fn bind_activation(&self) {
        // Enter / activate — multi-folder selection opens every selected item.
        {
            let selection = self.tree_selection.clone();
            let on_open = Rc::clone(&self.on_open);
            self.list_view.connect_activate(move |_, pos| {
                let paths = selected_paths_tree(&selection);
                if open_multi_folder_selection(&paths, &on_open) {
                    return;
                }
                activate_tree_at(&selection, &on_open, pos);
            });
        }
        {
            let selection = self.grid_selection.clone();
            let location = self.location.clone();
            let on_open = Rc::clone(&self.on_open);
            self.grid_view.connect_activate(move |_, pos| {
                let paths = selected_paths_flat(&selection, &location.borrow());
                if open_multi_folder_selection(&paths, &on_open) {
                    return;
                }
                activate_flat_at(&selection, &location, &on_open, pos);
            });
        }
    }

    fn bind_selection_changed(&self) {
        let status = self.status.clone();
        let tree = self.tree_selection.clone();
        let grid = self.grid_selection.clone();
        let view_mode = self.view_mode.clone();
        let update = move || {
            match *view_mode.borrow() {
                ViewMode::List => update_status_label(&status, &tree),
                ViewMode::Grid => update_status_label(&status, &grid),
            }
        };
        {
            let update = update.clone();
            self.tree_selection
                .connect_selection_changed(move |_, _, _| update());
        }
        {
            let update = update.clone();
            self.grid_selection
                .connect_selection_changed(move |_, _, _| update());
        }
    }

    pub fn update_status(&self) {
        match *self.view_mode.borrow() {
            ViewMode::List => update_status_label(&self.status, &self.tree_selection),
            ViewMode::Grid => update_status_label(&self.status, &self.grid_selection),
        }
    }
}

/// Right-click on empty view space: background menu (New Folder, Paste, …).
/// Row widgets Claim button-3, so this only fires off-item.
fn attach_empty_context_menu(widget: &impl IsA<gtk::Widget>, tab: &Rc<FolderTab>) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    gesture.set_propagation_phase(gtk::PropagationPhase::Bubble);
    let tab = Rc::clone(tab);
    let host = widget.clone().upcast::<gtk::Widget>();
    gesture.connect_pressed(move |g, _, x, y| {
        // Clear selection so Paste / New Folder target the current directory.
        tab.tree_selection.unselect_all();
        tab.grid_selection.unselect_all();
        if let Some(cb) = tab.on_context.borrow().as_ref() {
            cb(None, host.clone(), x, y);
        }
        g.set_state(gtk::EventSequenceState::Claimed);
    });
    widget.add_controller(gesture);
}

fn make_child_model(
    file: &gio::File,
    show_hidden: bool,
    search_query: String,
    sort_by: String,
    folders_first: bool,
    reversed: bool,
) -> gio::ListModel {
    let directory = gtk::DirectoryList::new(Some(FILE_ATTRIBUTES), Some(file));
    directory.set_monitored(true);

    let query = search_query.to_lowercase();
    let filter = gtk::CustomFilter::new(move |obj| {
        let Some(info) = obj.downcast_ref::<gio::FileInfo>() else {
            return false;
        };
        if !show_hidden && is_hidden(info) {
            return false;
        }
        if !query.is_empty() {
            let name = display_name(info).to_lowercase();
            if !name.contains(&query) {
                return false;
            }
        }
        true
    });
    let filter_model = gtk::FilterListModel::new(Some(directory), Some(filter));

    let sorter = gtk::CustomSorter::new(move |a, b| {
        compare_infos(a, b, &sort_by, folders_first, reversed)
    });
    let sort_model = gtk::SortListModel::new(Some(filter_model), Some(sorter));
    sort_model.upcast()
}

fn compare_infos(
    a: &glib::Object,
    b: &glib::Object,
    sort_by: &str,
    folders_first: bool,
    reversed: bool,
) -> gtk::Ordering {
    let Some(ia) = a.downcast_ref::<gio::FileInfo>() else {
        return gtk::Ordering::Equal;
    };
    let Some(ib) = b.downcast_ref::<gio::FileInfo>() else {
        return gtk::Ordering::Equal;
    };
    let mut ord = Ordering::Equal;
    if folders_first {
        let da = is_directory(ia);
        let db = is_directory(ib);
        ord = match (da, db) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => Ordering::Equal,
        };
    }
    if ord == Ordering::Equal {
        ord = match sort_by {
            "size" => ia.size().cmp(&ib.size()),
            "type" => content_type_description(ia).cmp(&content_type_description(ib)),
            "modified" => {
                let ta = ia.modification_date_time().map(|d| d.to_unix()).unwrap_or(0);
                let tb = ib.modification_date_time().map(|d| d.to_unix()).unwrap_or(0);
                ta.cmp(&tb)
            }
            _ => display_name(ia)
                .to_lowercase()
                .cmp(&display_name(ib).to_lowercase()),
        };
    }
    if reversed {
        ord = ord.reverse();
    }
    match ord {
        Ordering::Less => gtk::Ordering::Smaller,
        Ordering::Greater => gtk::Ordering::Larger,
        Ordering::Equal => gtk::Ordering::Equal,
    }
}

/// Corner badge overlaid on file/folder icons (GtkImage does not reliably
/// composite GEmblemedIcon, so we draw our own badges).
fn make_badge_emblem(
    pixel_size: i32,
    css_class: &str,
    halign: gtk::Align,
    valign: gtk::Align,
) -> gtk::Image {
    let emblem = gtk::Image::new();
    emblem.set_pixel_size(pixel_size);
    emblem.set_halign(halign);
    emblem.set_valign(valign);
    emblem.set_can_target(false);
    emblem.set_visible(false);
    emblem.add_css_class(css_class);
    emblem
}

fn make_symlink_emblem(pixel_size: i32) -> gtk::Image {
    // Bottom-right — classic symlink badge.
    make_badge_emblem(pixel_size, "symlink-emblem", gtk::Align::End, gtk::Align::End)
}

fn make_lock_emblem(pixel_size: i32) -> gtk::Image {
    // Bottom-left — no-write / read-only lock (keeps clear of the symlink badge).
    make_badge_emblem(pixel_size, "lock-emblem", gtk::Align::Start, gtk::Align::End)
}

fn overlay_badge(overlay: &gtk::Overlay, css_class: &str) -> Option<gtk::Image> {
    let mut child = overlay.first_child();
    while let Some(c) = child {
        let next = c.next_sibling();
        if let Ok(img) = c.downcast::<gtk::Image>() {
            if img.has_css_class(css_class) {
                return Some(img);
            }
        }
        child = next;
    }
    None
}

/// Show the symlink emblem for links; hide it otherwise (widgets are recycled).
fn apply_symlink_emblem(emblem: &gtk::Image, info: &gio::FileInfo) {
    if info.is_symlink() {
        emblem.set_icon_name(Some("emblem-symbolic-link"));
        emblem.set_visible(true);
    } else {
        emblem.set_visible(false);
    }
}

/// Show a lock when the current user cannot write/modify the item.
fn apply_lock_emblem(emblem: &gtk::Image, info: &gio::FileInfo) {
    if !can_write(info) {
        emblem.set_icon_name(Some("changes-prevent-symbolic"));
        emblem.set_visible(true);
    } else {
        emblem.set_visible(false);
    }
}

/// Tooltip for symlink and/or read-only; clear when neither applies (recycled rows).
fn apply_item_tooltip(widget: &impl IsA<gtk::Widget>, info: &gio::FileInfo) {
    let mut parts: Vec<String> = Vec::new();
    if info.is_symlink() {
        let target = info.symlink_target().map(|p| p.display().to_string());
        parts.push(match target {
            Some(t) if !t.is_empty() => format!("Symbolic link → {t}"),
            _ => "Symbolic link".to_string(),
        });
    }
    if !can_write(info) {
        parts.push("Read-only — you don't have permission to edit".to_string());
    }
    if parts.is_empty() {
        widget.set_tooltip_text(None);
    } else {
        widget.set_tooltip_text(Some(&parts.join("\n")));
    }
}

fn file_from_info(info: &gio::FileInfo) -> Option<gio::File> {
    if let Some(obj) = info.attribute_object("standard::file") {
        if let Ok(file) = obj.downcast::<gio::File>() {
            return Some(file);
        }
    }
    // Fallback: some models strip the object attribute — rebuild from URI if present.
    if let Some(uri) = info.attribute_string("standard::target-uri") {
        return Some(gio::File::for_uri(&uri));
    }
    None
}

fn resolve_file(info: &gio::FileInfo, fallback_dir: Option<&gio::File>) -> Option<gio::File> {
    if let Some(f) = file_from_info(info) {
        return Some(f);
    }
    fallback_dir.map(|d| util::file_from_dir_and_info(d, info))
}

fn info_from_selection_item(item: &glib::Object) -> Option<gio::FileInfo> {
    if let Some(row) = item.downcast_ref::<gtk::TreeListRow>() {
        return row.item().and_downcast::<gio::FileInfo>();
    }
    item.clone().downcast::<gio::FileInfo>().ok()
}

fn activate_tree_at(
    selection: &gtk::MultiSelection,
    on_open: &Rc<RefCell<Option<Rc<dyn Fn(gio::File, bool)>>>>,
    pos: u32,
) {
    let Some(item) = selection.item(pos) else {
        return;
    };
    let Some(info) = info_from_selection_item(&item) else {
        return;
    };
    let Some(file) = resolve_file(&info, None) else {
        eprintln!("gtk-files: could not resolve file for “{}”", display_name(&info));
        return;
    };
    if let Some(cb) = on_open.borrow().as_ref() {
        cb(file, is_directory(&info));
    }
}

fn activate_flat_at(
    selection: &gtk::MultiSelection,
    location: &RefCell<gio::File>,
    on_open: &Rc<RefCell<Option<Rc<dyn Fn(gio::File, bool)>>>>,
    pos: u32,
) {
    if let Some(info) = selection.item(pos).and_downcast::<gio::FileInfo>() {
        let file = resolve_file(&info, Some(&location.borrow()));
        let Some(file) = file else {
            return;
        };
        if let Some(cb) = on_open.borrow().as_ref() {
            cb(file, is_directory(&info));
        }
    }
}

fn open_resolved(
    on_open: &Rc<RefCell<Option<Rc<dyn Fn(gio::File, bool)>>>>,
    file: gio::File,
    is_dir: bool,
) {
    if let Some(cb) = on_open.borrow().as_ref() {
        cb(file, is_dir);
    }
}

/// When several folders are selected, open each selected path (folders → new tabs
/// via the window handler; files → default app). Returns true if handled.
fn open_multi_folder_selection(
    paths: &[PathBuf],
    on_open: &Rc<RefCell<Option<Rc<dyn Fn(gio::File, bool)>>>>,
) -> bool {
    let dir_count = paths.iter().filter(|p| p.is_dir()).count();
    if dir_count <= 1 {
        return false;
    }
    for path in paths {
        if path.is_dir() {
            open_resolved(on_open, gio::File::for_path(path), true);
        } else if path.is_file() {
            open_resolved(on_open, gio::File::for_path(path), false);
        }
    }
    true
}

fn selected_paths_tree(selection: &gtk::MultiSelection) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let n = selection.n_items();
    for i in 0..n {
        if !selection.is_selected(i) {
            continue;
        }
        if let Some(item) = selection.item(i) {
            if let Some(info) = info_from_selection_item(&item) {
                if let Some(file) = resolve_file(&info, None) {
                    if let Some(p) = file.path() {
                        paths.push(p);
                    }
                }
            }
        }
    }
    paths
}

fn selected_paths_flat(selection: &gtk::MultiSelection, dir: &gio::File) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let n = selection.n_items();
    for i in 0..n {
        if !selection.is_selected(i) {
            continue;
        }
        if let Some(info) = selection.item(i).and_downcast::<gio::FileInfo>() {
            if let Some(child) = resolve_file(&info, Some(dir)) {
                if let Some(p) = child.path() {
                    paths.push(p);
                }
            }
        }
    }
    paths
}

/// File-manager style selection: click, Ctrl+click toggle, Shift+click range.
fn apply_pointer_selection(
    selection: &gtk::MultiSelection,
    anchor: &RefCell<Option<u32>>,
    pos: u32,
    mods: gdk::ModifierType,
) {
    let n = selection.n_items();
    if n == 0 || pos >= n {
        return;
    }

    let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
    let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);

    if shift {
        let start = anchor.borrow().unwrap_or(pos).min(n - 1);
        let pos = pos.min(n - 1);
        let (lo, hi) = if start <= pos { (start, pos) } else { (pos, start) };
        // Shift replaces with the range; Ctrl+Shift adds the range.
        selection.select_range(lo, hi - lo + 1, !ctrl);
        return;
    }

    if ctrl {
        if selection.is_selected(pos) {
            selection.unselect_item(pos);
        } else {
            selection.select_item(pos, false);
        }
        *anchor.borrow_mut() = Some(pos);
        return;
    }

    selection.unselect_all();
    selection.select_item(pos, true);
    *anchor.borrow_mut() = Some(pos);
}

fn update_status_label(label: &gtk::Label, selection: &impl IsA<gtk::SelectionModel>) {
    let selection = selection.as_ref();
    let total = selection.n_items();
    let mut selected = 0u32;
    for i in 0..total {
        if selection.is_selected(i) {
            selected += 1;
        }
    }
    if selected == 0 {
        label.set_text(&format!(
            "{} item{}",
            total,
            if total == 1 { "" } else { "s" }
        ));
    } else {
        label.set_text(&format!("{selected} of {total} selected"));
    }
}

fn build_tree_column_view(
    selection: &gtk::MultiSelection,
    anchor: Rc<RefCell<Option<u32>>>,
    on_open: Rc<RefCell<Option<Rc<dyn Fn(gio::File, bool)>>>>,
    on_context: Rc<RefCell<Option<Rc<dyn Fn(Option<Vec<PathBuf>>, gtk::Widget, f64, f64)>>>>,
    on_refresh: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) -> gtk::ColumnView {
    let view = gtk::ColumnView::new(Some(selection.clone()));
    view.set_reorderable(false);
    view.set_show_column_separators(false);
    view.set_enable_rubberband(false);
    view.add_css_class("data-table");
    view.add_css_class("file-list");

    // Name column with TreeExpander (chevron) + double-click to open
    {
        let factory = gtk::SignalListItemFactory::new();
        let selection = selection.clone();
        let anchor = Rc::clone(&anchor);
        let on_refresh = Rc::clone(&on_refresh);
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            item.set_activatable(true);
            // Disable built-in exclusive select so Ctrl/Shift multi-select works.
            item.set_selectable(false);

            let expander = gtk::TreeExpander::new();
            // Indent by depth and reserve expander width for leaves so file icons
            // sit under the parent's folder icon, not under the chevron.
            expander.set_indent_for_depth(true);
            expander.set_indent_for_icon(true);
            expander.add_css_class("file-expander");

            let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            row.add_css_class("file-row-content");
            row.set_hexpand(true);
            row.set_margin_start(0);
            let icon = gtk::Image::new();
            icon.set_pixel_size(32);
            icon.set_can_target(false);
            let lock = make_lock_emblem(16);
            let symlink = make_symlink_emblem(16);
            let icon_overlay = gtk::Overlay::new();
            icon_overlay.set_child(Some(&icon));
            icon_overlay.add_overlay(&lock);
            icon_overlay.add_overlay(&symlink);
            icon_overlay.set_can_target(false);
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_hexpand(true);
            label.set_can_target(false);
            row.append(&icon_overlay);
            row.append(&label);

            let target: Rc<RefCell<Option<(gio::File, bool, PathBuf)>>> =
                Rc::new(RefCell::new(None));

            // Click / Ctrl+click / Shift+click; double-click opens.
            {
                let target = Rc::clone(&target);
                let on_open = Rc::clone(&on_open);
                let selection = selection.clone();
                let anchor = Rc::clone(&anchor);
                let list_item = item.clone();
                let row_focus = row.clone();
                let gesture = gtk::GestureClick::new();
                gesture.set_button(1);
                // Bubble (not Capture) + no Claim on single-press so DragSource
                // can start a click-and-hold drag to other apps / folders.
                gesture.connect_pressed(move |g, n_press, _, _| {
                    let pos = list_item.position();
                    let mods = g.current_event_state();
                    apply_pointer_selection(&selection, &anchor, pos, mods);
                    // Take focus from the terminal so keyboard ops target files.
                    if let Some(view) = row_focus.ancestor(gtk::ColumnView::static_type()) {
                        view.grab_focus();
                    }
                    let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
                    let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
                    // Claim Ctrl toggles only. Do NOT claim Shift — Shift+drag is
                    // MOVE in file managers; claiming steals the sequence from
                    // DragSource and crashes GTK.
                    if ctrl {
                        g.set_state(gtk::EventSequenceState::Claimed);
                    }
                    if n_press >= 2 && !ctrl && !shift {
                        let paths = selected_paths_tree(&selection);
                        if !open_multi_folder_selection(&paths, &on_open) {
                            if let Some((file, is_dir, _)) = target.borrow().clone() {
                                open_resolved(&on_open, file, is_dir);
                            }
                        }
                        g.set_state(gtk::EventSequenceState::Claimed);
                    }
                });
                row.add_controller(gesture);
            }
            // Right-click: keep multi-selection if this item is already selected.
            // Gesture lives on the row (not TreeExpander): with hide_expander for
            // files, clicks on the icon/name never reached an expander controller,
            // so the empty-view menu stole the event and no file Properties menu appeared.
            {
                let target = Rc::clone(&target);
                let on_context = Rc::clone(&on_context);
                let selection = selection.clone();
                let list_item = item.clone();
                let anchor = row.clone();
                let gesture = gtk::GestureClick::new();
                gesture.set_button(3);
                gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
                gesture.connect_pressed(move |g, _, x, y| {
                    let pos = list_item.position();
                    if !selection.is_selected(pos) {
                        selection.unselect_all();
                        selection.select_item(pos, true);
                    }
                    let mut paths = selected_paths_tree(&selection);
                    if paths.is_empty() {
                        if let Some((_, _, p)) = target.borrow().clone() {
                            paths = vec![p];
                        }
                    }
                    let paths = if paths.is_empty() { None } else { Some(paths) };
                    if let Some(cb) = on_context.borrow().as_ref() {
                        cb(paths, anchor.clone().upcast(), x, y);
                    }
                    g.set_state(gtk::EventSequenceState::Claimed);
                });
                row.add_controller(gesture);
            }

            // Drag files out; drop onto folders.
            {
                let target = Rc::clone(&target);
                let selection = selection.clone();
                let list_item = item.clone();
                dnd::attach_drag_source(&row, move || {
                    let Some((_, _, this)) = target.borrow().clone() else {
                        return Vec::new();
                    };
                    let mut paths = selected_paths_tree(&selection);
                    if !paths.iter().any(|p| p == &this) {
                        selection.unselect_all();
                        selection.select_item(list_item.position(), true);
                        paths = vec![this];
                    }
                    paths
                });
            }
            {
                let on_refresh = Rc::clone(&on_refresh);
                dnd::attach_folder_drop_target(&row, Rc::clone(&target), move || {
                    if let Some(cb) = on_refresh.borrow().as_ref() {
                        cb();
                    }
                });
            }

            unsafe {
                row.set_data("file-target", target);
            }

            expander.set_child(Some(&row));
            item.set_child(Some(&expander));
        });
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(tree_row) = item.item().and_downcast::<gtk::TreeListRow>() else {
                return;
            };
            let Some(info) = tree_row.item().and_downcast::<gio::FileInfo>() else {
                return;
            };
            let Some(expander) = item.child().and_downcast::<gtk::TreeExpander>() else {
                return;
            };
            expander.set_list_row(Some(&tree_row));
            expander.set_hide_expander(!is_directory(&info));

            let Some(row) = expander.child().and_downcast::<gtk::Box>() else {
                return;
            };
            let Some(icon_overlay) = row.first_child().and_downcast::<gtk::Overlay>() else {
                return;
            };
            let Some(icon) = icon_overlay.first_child().and_downcast::<gtk::Image>() else {
                return;
            };
            let Some(symlink) = overlay_badge(&icon_overlay, "symlink-emblem") else {
                return;
            };
            let Some(lock) = overlay_badge(&icon_overlay, "lock-emblem") else {
                return;
            };
            let Some(label) = icon_overlay
                .next_sibling()
                .and_downcast::<gtk::Label>()
            else {
                return;
            };
            label.set_text(&display_name(&info));
            apply_item_tooltip(&row, &info);
            apply_symlink_emblem(&symlink, &info);
            apply_lock_emblem(&lock, &info);

            if let Some(file) = resolve_file(&info, None) {
                let path = file.path().unwrap_or_default();
                let is_dir = is_directory(&info);
                // List view: compact thumbs
                thumbnails::apply_thumbnail(&icon, &file, &info, 32);
                unsafe {
                    if let Some(ptr) =
                        row.data::<Rc<RefCell<Option<(gio::File, bool, PathBuf)>>>>("file-target")
                    {
                        *ptr.as_ref().borrow_mut() = Some((file, is_dir, path));
                    }
                }
            } else {
                icon.set_from_gicon(&icon_for_info(&info, false));
                icon.set_pixel_size(32);
            }
        });
        let col = gtk::ColumnViewColumn::new(Some("Name"), Some(factory));
        col.set_expand(true);
        view.append_column(&col);
    }

    append_info_columns(&view);
    view
}

fn append_info_columns(view: &gtk::ColumnView) {
    // Size
    {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let label = gtk::Label::new(None);
            label.set_xalign(1.0);
            label.add_css_class("dim-label");
            item.set_child(Some(&label));
        });
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(info) = item
                .item()
                .and_then(|o| info_from_selection_item(&o))
            else {
                return;
            };
            let label = item.child().and_downcast::<gtk::Label>().unwrap();
            if is_directory(&info) {
                label.set_text("—");
            } else {
                label.set_text(&format_size(info.size() as u64));
            }
        });
        let col = gtk::ColumnViewColumn::new(Some("Size"), Some(factory));
        col.set_fixed_width(100);
        view.append_column(&col);
    }

    // Type
    {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            label.add_css_class("dim-label");
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            item.set_child(Some(&label));
        });
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(info) = item
                .item()
                .and_then(|o| info_from_selection_item(&o))
            else {
                return;
            };
            let label = item.child().and_downcast::<gtk::Label>().unwrap();
            let mut text = content_type_description(&info);
            if info.is_symlink() {
                text.push_str(" (link)");
            }
            label.set_text(&text);
        });
        let col = gtk::ColumnViewColumn::new(Some("Type"), Some(factory));
        col.set_fixed_width(140);
        view.append_column(&col);
    }

    // Modified
    {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            label.add_css_class("dim-label");
            item.set_child(Some(&label));
        });
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(info) = item
                .item()
                .and_then(|o| info_from_selection_item(&o))
            else {
                return;
            };
            let label = item.child().and_downcast::<gtk::Label>().unwrap();
            label.set_text(&format_mtime(&info));
        });
        let col = gtk::ColumnViewColumn::new(Some("Modified"), Some(factory));
        col.set_fixed_width(140);
        view.append_column(&col);
    }
}

fn build_grid_view(
    selection: &gtk::MultiSelection,
    anchor: Rc<RefCell<Option<u32>>>,
    icon_size: Rc<RefCell<i32>>,
    on_open: Rc<RefCell<Option<Rc<dyn Fn(gio::File, bool)>>>>,
    on_context: Rc<RefCell<Option<Rc<dyn Fn(Option<Vec<PathBuf>>, gtk::Widget, f64, f64)>>>>,
    on_refresh: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) -> gtk::GridView {
    let factory = gtk::SignalListItemFactory::new();
    let selection_for_factory = selection.clone();
    let anchor_for_factory = Rc::clone(&anchor);
    let icon_size_setup = Rc::clone(&icon_size);
    let on_refresh = Rc::clone(&on_refresh);
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        item.set_activatable(true);
        // Disable built-in exclusive select so Ctrl/Shift multi-select works.
        item.set_selectable(false);

        let size = *icon_size_setup.borrow();
        let box_ = gtk::Box::new(gtk::Orientation::Vertical, 4);
        // Room for the largest preset so recycled widgets can grow.
        box_.set_size_request(192 + 24, 192 + 48);
        box_.set_halign(gtk::Align::Center);
        box_.add_css_class("file-row-content");
        let icon = gtk::Image::new();
        icon.set_pixel_size(size);
        icon.set_halign(gtk::Align::Center);
        icon.set_can_target(false);
        let lock = make_lock_emblem((size / 3).clamp(16, 48));
        let symlink = make_symlink_emblem((size / 3).clamp(16, 48));
        let icon_overlay = gtk::Overlay::new();
        icon_overlay.set_child(Some(&icon));
        icon_overlay.add_overlay(&lock);
        icon_overlay.add_overlay(&symlink);
        icon_overlay.set_halign(gtk::Align::Center);
        icon_overlay.set_can_target(false);
        let label = gtk::Label::new(None);
        label.set_wrap(true);
        label.set_justify(gtk::Justification::Center);
        label.set_lines(2);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(12);
        label.set_can_target(false);
        box_.append(&icon_overlay);
        box_.append(&label);

        let target: Rc<RefCell<Option<(gio::File, bool, PathBuf)>>> = Rc::new(RefCell::new(None));
        {
            let target = Rc::clone(&target);
            let on_open = Rc::clone(&on_open);
            let selection = selection_for_factory.clone();
            let anchor = Rc::clone(&anchor_for_factory);
            let list_item = item.clone();
            let box_focus = box_.clone();
            let gesture = gtk::GestureClick::new();
            gesture.set_button(1);
            // Bubble (not Capture) + no Claim on single-press so DragSource
            // can start a click-and-hold drag to other apps / folders.
            gesture.connect_pressed(move |g, n_press, _, _| {
                let pos = list_item.position();
                let mods = g.current_event_state();
                apply_pointer_selection(&selection, &anchor, pos, mods);
                if let Some(view) = box_focus.ancestor(gtk::GridView::static_type()) {
                    view.grab_focus();
                }
                let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
                let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
                // Claim Ctrl only — Shift+drag must reach DragSource (MOVE).
                if ctrl {
                    g.set_state(gtk::EventSequenceState::Claimed);
                }
                if n_press >= 2 && !ctrl && !shift {
                    let parent = target
                        .borrow()
                        .as_ref()
                        .and_then(|(_, _, p)| p.parent().map(gio::File::for_path))
                        .unwrap_or_else(|| gio::File::for_path("/"));
                    let paths = selected_paths_flat(&selection, &parent);
                    if !open_multi_folder_selection(&paths, &on_open) {
                        if let Some((file, is_dir, _)) = target.borrow().clone() {
                            open_resolved(&on_open, file, is_dir);
                        }
                    }
                    g.set_state(gtk::EventSequenceState::Claimed);
                }
            });
            box_.add_controller(gesture);
        }
        {
            let target = Rc::clone(&target);
            let on_context = Rc::clone(&on_context);
            let selection = selection_for_factory.clone();
            let list_item = item.clone();
            let box_widget = box_.clone();
            let gesture = gtk::GestureClick::new();
            gesture.set_button(3);
            gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
            gesture.connect_pressed(move |g, _, x, y| {
                let pos = list_item.position();
                if !selection.is_selected(pos) {
                    selection.unselect_all();
                    selection.select_item(pos, true);
                }
                let parent = target
                    .borrow()
                    .as_ref()
                    .and_then(|(_, _, p)| p.parent().map(gio::File::for_path))
                    .unwrap_or_else(|| gio::File::for_path("/"));
                let mut paths = selected_paths_flat(&selection, &parent);
                if paths.is_empty() {
                    if let Some((_, _, p)) = target.borrow().clone() {
                        paths = vec![p];
                    }
                }
                let paths = if paths.is_empty() { None } else { Some(paths) };
                if let Some(cb) = on_context.borrow().as_ref() {
                    cb(paths, box_widget.clone().upcast(), x, y);
                }
                g.set_state(gtk::EventSequenceState::Claimed);
            });
            box_.add_controller(gesture);
        }

        {
            let target = Rc::clone(&target);
            let selection = selection_for_factory.clone();
            let list_item = item.clone();
            dnd::attach_drag_source(&box_, move || {
                let Some((_, _, this)) = target.borrow().clone() else {
                    return Vec::new();
                };
                let parent = this
                    .parent()
                    .map(gio::File::for_path)
                    .unwrap_or_else(|| gio::File::for_path(&this));
                let mut paths = selected_paths_flat(&selection, &parent);
                if !paths.iter().any(|p| p == &this) {
                    selection.unselect_all();
                    selection.select_item(list_item.position(), true);
                    paths = vec![this];
                }
                paths
            });
        }
        {
            let on_refresh = Rc::clone(&on_refresh);
            dnd::attach_folder_drop_target(&box_, Rc::clone(&target), move || {
                if let Some(cb) = on_refresh.borrow().as_ref() {
                    cb();
                }
            });
        }

        unsafe {
            box_.set_data("file-target", target);
        }
        item.set_child(Some(&box_));
    });
    let icon_size_bind = Rc::clone(&icon_size);
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let Some(info) = item.item().and_downcast::<gio::FileInfo>() else {
            return;
        };
        let Some(box_) = item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let Some(icon_overlay) = box_.first_child().and_downcast::<gtk::Overlay>() else {
            return;
        };
        let Some(icon) = icon_overlay.first_child().and_downcast::<gtk::Image>() else {
            return;
        };
        let Some(symlink) = overlay_badge(&icon_overlay, "symlink-emblem") else {
            return;
        };
        let Some(lock) = overlay_badge(&icon_overlay, "lock-emblem") else {
            return;
        };
        let Some(label) = icon_overlay
            .next_sibling()
            .and_downcast::<gtk::Label>()
        else {
            return;
        };
        label.set_text(&display_name(&info));
        apply_item_tooltip(&box_, &info);

        let size = *icon_size_bind.borrow();
        let badge = (size / 3).clamp(16, 48);
        symlink.set_pixel_size(badge);
        lock.set_pixel_size(badge);
        apply_symlink_emblem(&symlink, &info);
        apply_lock_emblem(&lock, &info);
        if let Some(file) = resolve_file(&info, None) {
            let path = file.path().unwrap_or_default();
            let is_dir = is_directory(&info);
            thumbnails::apply_thumbnail(&icon, &file, &info, size);
            unsafe {
                if let Some(ptr) =
                    box_.data::<Rc<RefCell<Option<(gio::File, bool, PathBuf)>>>>("file-target")
                {
                    *ptr.as_ref().borrow_mut() = Some((file, is_dir, path));
                }
            }
        } else {
            icon.set_from_gicon(&icon_for_info(&info, false));
            icon.set_pixel_size(size);
        }
    });

    let view = gtk::GridView::new(Some(selection.clone()), Some(factory));
    view.set_enable_rubberband(false);
    view.set_max_columns(12);
    view.set_min_columns(2);
    view.add_css_class("file-grid");
    view
}
