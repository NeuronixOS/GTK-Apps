//! Main application window — rusty counterpart to eog's EogWindow.
//!
//! Header bar, image view, status bar, GActions for open / navigate / zoom /
//! rotate / fullscreen / trash / copy.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::glib::prelude::*;
use gtk::prelude::*;

use crate::config::{self, Config};
use crate::image_list::{self, ImageList};
use crate::image_view::ImageView;

pub struct ImageWindow {
    pub window: gtk::ApplicationWindow,
    list: Rc<RefCell<ImageList>>,
    view: ImageView,
    title_label: gtk::Label,
    position_label: gtk::Label,
    placeholder: gtk::Label,
    stack: gtk::Stack,
}

impl ImageWindow {
    pub fn new(app: &gtk::Application, cfg: &Config) -> Rc<Self> {
        let view = ImageView::new(cfg.zoom_min, cfg.zoom_max, cfg.zoom_step, cfg.best_fit);

        let placeholder = gtk::Label::new(Some("Open an image to get started"));
        placeholder.add_css_class("dim-label");
        placeholder.set_halign(gtk::Align::Center);
        placeholder.set_valign(gtk::Align::Center);
        placeholder.set_wrap(true);

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_vexpand(true);
        stack.add_css_class("gtk-content");
        stack.add_named(&placeholder, Some("empty"));
        stack.add_named(&view.root, Some("image"));
        stack.set_visible_child_name("empty");

        let title_label = gtk::Label::new(Some("GTK Image"));
        title_label.add_css_class("title");
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);

        let position_label = gtk::Label::new(None);
        position_label.add_css_class("dim-label");
        position_label.set_margin_start(12);

        let status_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        status_box.add_css_class("statusbar");
        status_box.set_margin_start(10);
        status_box.set_margin_end(10);
        status_box.set_margin_top(4);
        status_box.set_margin_bottom(4);
        status_box.append(&position_label);
        status_box.append(view.status_label());

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main_box.append(&stack);
        main_box.append(&status_box);

        let header = gtk::HeaderBar::new();
        header.set_title_widget(Some(&title_label));

        let open_btn = gtk::Button::from_icon_name("document-open-symbolic");
        open_btn.set_tooltip_text(Some("Open Image (Ctrl+O)"));
        open_btn.set_action_name(Some("win.open"));
        header.pack_start(&open_btn);

        let prev_btn = gtk::Button::from_icon_name("go-previous-symbolic");
        prev_btn.set_tooltip_text(Some("Previous (Left)"));
        prev_btn.set_action_name(Some("win.go-previous"));
        header.pack_start(&prev_btn);

        let next_btn = gtk::Button::from_icon_name("go-next-symbolic");
        next_btn.set_tooltip_text(Some("Next (Right)"));
        next_btn.set_action_name(Some("win.go-next"));
        header.pack_start(&next_btn);

        let zoom_out = gtk::Button::from_icon_name("zoom-out-symbolic");
        zoom_out.set_tooltip_text(Some("Zoom Out"));
        zoom_out.set_action_name(Some("win.zoom-out"));
        header.pack_end(&zoom_out);

        let zoom_in = gtk::Button::from_icon_name("zoom-in-symbolic");
        zoom_in.set_tooltip_text(Some("Zoom In"));
        zoom_in.set_action_name(Some("win.zoom-in"));
        header.pack_end(&zoom_in);

        let fit_btn = gtk::Button::from_icon_name("zoom-fit-best-symbolic");
        fit_btn.set_tooltip_text(Some("Best Fit (F)"));
        fit_btn.set_action_name(Some("win.zoom-fit"));
        header.pack_end(&fit_btn);

        let fs_btn = gtk::Button::from_icon_name("view-fullscreen-symbolic");
        fs_btn.set_tooltip_text(Some("Fullscreen (F11)"));
        fs_btn.set_action_name(Some("win.fullscreen"));
        header.pack_end(&fs_btn);

        let menu_button = build_menu_button();
        header.pack_end(&menu_button);

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("GTK Image")
            .default_width(cfg.window_width)
            .default_height(cfg.window_height)
            .build();
        window.set_titlebar(Some(&header));
        window.set_child(Some(&main_box));

        let iw = Rc::new(Self {
            window,
            list: Rc::new(RefCell::new(ImageList::new())),
            view,
            title_label,
            position_label,
            placeholder,
            stack,
        });

        install_actions(&iw);
        setup_context_menu(&iw);
        let drag_source = setup_drag_source(&iw);
        setup_drop_target(&iw, drag_source);

        {
            let iw_weak = Rc::downgrade(&iw);
            iw.window.connect_close_request(move |win| {
                if !win.is_maximized() && !win.is_fullscreen() {
                    let w = win.width();
                    let h = win.height();
                    if w > 0 && h > 0 {
                        config::save_window_size(w, h);
                    }
                }
                let _ = iw_weak;
                glib::Propagation::Proceed
            });
        }

        iw
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn open_path(&self, path: &Path) {
        if path.is_dir() {
            self.list.borrow_mut().open_directory(path);
        } else {
            self.list.borrow_mut().open_file(path);
        }
        self.reload_current();
    }

    fn reload_current(&self) {
        let path = self.list.borrow().current().map(|p| p.to_path_buf());
        match path {
            Some(path) => match image_list::load_pixbuf(&path) {
                Ok(pb) => {
                    self.view.set_pixbuf(pb);
                    self.stack.set_visible_child_name("image");
                    self.update_chrome(&path);
                }
                Err(err) => {
                    self.view.clear();
                    self.placeholder.set_text(&err);
                    self.stack.set_visible_child_name("empty");
                    self.title_label.set_text("GTK Image");
                    self.window.set_title(Some("GTK Image"));
                    self.position_label.set_text("");
                    eprintln!("gtk-image: {err}");
                }
            },
            None => {
                self.view.clear();
                self.placeholder
                    .set_text("Open an image to get started");
                self.stack.set_visible_child_name("empty");
                self.title_label.set_text("GTK Image");
                self.window.set_title(Some("GTK Image"));
                self.position_label.set_text("");
            }
        }
        self.refresh_nav_actions();
    }

    fn update_chrome(&self, path: &Path) {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("GTK Image");
        self.title_label.set_text(name);
        self.window.set_title(Some(name));
        self.position_label
            .set_text(&self.list.borrow().position_label());
    }

    fn refresh_nav_actions(&self) {
        let list = self.list.borrow();
        let idx = list.index();
        let len = list.len();
        let set_enabled = |name: &str, enabled: bool| {
            if let Some(action) = self.window.lookup_action(name) {
                if let Ok(sa) = action.downcast::<gio::SimpleAction>() {
                    sa.set_enabled(enabled);
                }
            }
        };
        match idx {
            Some(i) => {
                set_enabled("go-previous", i > 0);
                set_enabled("go-next", i + 1 < len);
                set_enabled("go-first", i > 0);
                set_enabled("go-last", i + 1 < len);
            }
            None => {
                set_enabled("go-previous", false);
                set_enabled("go-next", false);
                set_enabled("go-first", false);
                set_enabled("go-last", false);
            }
        }
        let has = self.view.has_image();
        for name in [
            "zoom-in",
            "zoom-out",
            "zoom-normal",
            "zoom-fit",
            "rotate-cw",
            "rotate-ccw",
            "flip-horizontal",
            "flip-vertical",
            "copy",
            "trash",
            "save-as",
            "show-in-folder",
        ] {
            set_enabled(name, has);
        }
    }
}

fn build_menu_button() -> gtk::MenuButton {
    let mut icons = gtk_theme::IconMenu::new();
    let menu = gio::Menu::new();

    let file = gio::Menu::new();
    icons.append_action(&file, "Open…", "win.open");
    icons.append_action(&file, "Open Folder…", "win.open-folder");
    icons.append_action(&file, "Save As…", "win.save-as");
    menu.append_section(None, &file);

    let edit = gio::Menu::new();
    icons.append_action(&edit, "Copy", "win.copy");
    icons.append_action(&edit, "Move to Trash", "win.trash");
    icons.append(
        &edit,
        "Show in Folder",
        "win.show-in-folder",
        "folder-symbolic",
    );
    menu.append_section(None, &edit);

    let transform = gio::Menu::new();
    icons.append(
        &transform,
        "Rotate Clockwise",
        "win.rotate-cw",
        "object-rotate-right-symbolic",
    );
    icons.append(
        &transform,
        "Rotate Counterclockwise",
        "win.rotate-ccw",
        "object-rotate-left-symbolic",
    );
    icons.append_action(&transform, "Flip Horizontal", "win.flip-horizontal");
    icons.append(
        &transform,
        "Flip Vertical",
        "win.flip-vertical",
        "object-flip-vertical-symbolic",
    );
    menu.append_section(None, &transform);

    let view = gio::Menu::new();
    icons.append_action(&view, "Zoom In", "win.zoom-in");
    icons.append_action(&view, "Zoom Out", "win.zoom-out");
    icons.append(
        &view,
        "Normal Size",
        "win.zoom-normal",
        "zoom-original-symbolic",
    );
    icons.append_action(&view, "Best Fit", "win.zoom-fit");
    icons.append_action(&view, "Full Screen", "win.fullscreen");
    gtk_theme::append_profile_menu(&view, "win.theme");
    menu.append_section(None, &view);

    let help = gio::Menu::new();
    icons.append_action(&help, "Keyboard Shortcuts", "app.shortcuts");
    icons.append_action(&help, "About", "app.about");
    menu.append_section(None, &help);

    let button = gtk::MenuButton::new();
    button.set_icon_name("open-menu-symbolic");
    button.set_tooltip_text(Some("Menu"));
    button.set_menu_model(Some(&menu));
    icons.bind_menu_button(&button);
    button
}

fn install_actions(iw: &Rc<ImageWindow>) {
    let window = &iw.window;

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
        window.add_action(&act);
        // Follow ~/.config/gtk-apps/theme.toml when any suite app changes profile.
        gtk_theme::watch_theme_sync_action(window, "theme");
        gtk_theme::install_open_theme_editor_action(window);
    }

    // ---- open ----
    {
        let iw = Rc::clone(iw);
        bind(window, "open", move |_| {
            let parent = iw.window.clone();
            let iw = Rc::clone(&iw);
            let filter = image_filter();
            gtk_theme::present_file_chooser(
                Some(&parent),
                "Open Image",
                gtk::FileChooserAction::Open,
                "Open",
                Some(&filter),
                None,
                move |file| {
                    if let Some(path) = file_to_path(file) {
                        iw.open_path(&path);
                    }
                },
            );
        });
    }

    // ---- open-folder ----
    {
        let iw = Rc::clone(iw);
        bind(window, "open-folder", move |_| {
            let parent = iw.window.clone();
            let iw = Rc::clone(&iw);
            gtk_theme::present_file_chooser(
                Some(&parent),
                "Open Folder",
                gtk::FileChooserAction::SelectFolder,
                "Open",
                None,
                None,
                move |file| {
                    if let Some(path) = file_to_path(file) {
                        iw.open_path(&path);
                    }
                },
            );
        });
    }

    // ---- navigation ----
    for (name, nav) in [
        ("go-previous", Nav::Previous),
        ("go-next", Nav::Next),
        ("go-first", Nav::First),
        ("go-last", Nav::Last),
    ] {
        let iw = Rc::clone(iw);
        bind(window, name, move |_| {
            let changed = match nav {
                Nav::Previous => iw.list.borrow_mut().go_previous(),
                Nav::Next => iw.list.borrow_mut().go_next(),
                Nav::First => iw.list.borrow_mut().go_first(),
                Nav::Last => iw.list.borrow_mut().go_last(),
            };
            if changed {
                iw.reload_current();
            }
        });
    }

    // ---- zoom ----
    {
        let iw = Rc::clone(iw);
        bind(window, "zoom-in", move |_| iw.view.zoom_in());
    }
    {
        let iw = Rc::clone(iw);
        bind(window, "zoom-out", move |_| iw.view.zoom_out());
    }
    {
        let iw = Rc::clone(iw);
        bind(window, "zoom-normal", move |_| iw.view.zoom_normal());
    }
    {
        let iw = Rc::clone(iw);
        bind(window, "zoom-fit", move |_| iw.view.zoom_best_fit());
    }

    // ---- transform ----
    {
        let iw = Rc::clone(iw);
        bind(window, "rotate-cw", move |_| iw.view.rotate_cw());
    }
    {
        let iw = Rc::clone(iw);
        bind(window, "rotate-ccw", move |_| iw.view.rotate_ccw());
    }
    {
        let iw = Rc::clone(iw);
        bind(window, "flip-horizontal", move |_| iw.view.flip_horizontal());
    }
    {
        let iw = Rc::clone(iw);
        bind(window, "flip-vertical", move |_| iw.view.flip_vertical());
    }

    // ---- fullscreen ----
    {
        let window_weak = window.downgrade();
        let action = gio::SimpleAction::new_stateful("fullscreen", None, &false.to_variant());
        action.connect_change_state(move |action, state| {
            let active = state.and_then(|s| s.get::<bool>()).unwrap_or(false);
            if let Some(window) = window_weak.upgrade() {
                if active {
                    window.fullscreen();
                } else {
                    window.unfullscreen();
                }
            }
            action.set_state(&active.to_variant());
        });
        // Also toggle when activated without state change (F11).
        {
            let action2 = action.clone();
            action.connect_activate(move |_, _| {
                let current = action2.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
                let _ = action2.change_state(&(!current).to_variant());
            });
        }
        window.add_action(&action);
    }

    // ---- copy ----
    {
        let iw = Rc::clone(iw);
        bind(window, "copy", move |_| {
            if let Some(pb) = iw.view.current_pixbuf() {
                let texture = gdk::Texture::for_pixbuf(&pb);
                iw.window.clipboard().set_texture(&texture);
            }
        });
    }

    // ---- trash ----
    {
        let iw = Rc::clone(iw);
        bind(window, "trash", move |_| {
            let path = iw.list.borrow().current().map(|p| p.to_path_buf());
            let Some(path) = path else {
                return;
            };
            let file = gio::File::for_path(&path);
            match file.trash(gio::Cancellable::NONE) {
                Ok(()) => {
                    iw.list.borrow_mut().remove_current();
                    iw.reload_current();
                }
                Err(e) => eprintln!("gtk-image: trash failed: {e}"),
            }
        });
    }

    // ---- save-as ----
    {
        let iw = Rc::clone(iw);
        bind(window, "save-as", move |_| {
            let Some(pb) = iw.view.current_pixbuf() else {
                return;
            };
            let suggested = iw
                .list
                .borrow()
                .current()
                .and_then(|p| p.file_name().map(|n| n.to_os_string()))
                .unwrap_or_else(|| std::ffi::OsString::from("image.png"));

            let parent = iw.window.clone();
            let iw2 = Rc::clone(&iw);
            let name = suggested.to_string_lossy().into_owned();
            gtk_theme::present_file_chooser(
                Some(&parent),
                "Save Image As",
                gtk::FileChooserAction::Save,
                "Save",
                None,
                Some(&name),
                move |file| {
                    if let Some(path) = file.and_then(|f| f.path()) {
                        if let Err(e) = save_pixbuf(&pb, &path) {
                            eprintln!("gtk-image: save failed: {e}");
                            iw2.placeholder.set_text(&format!("Save failed: {e}"));
                        }
                    }
                },
            );
        });
    }

    // ---- show-in-folder ----
    {
        let iw = Rc::clone(iw);
        bind(window, "show-in-folder", move |_| {
            let path = iw.list.borrow().current().map(|p| p.to_path_buf());
            let Some(path) = path else {
                return;
            };
            let folder = path.parent().unwrap_or(Path::new("."));
            if let Err(e) = std::process::Command::new("xdg-open").arg(folder).spawn() {
                eprintln!("gtk-image: show-in-folder failed: {e}");
            }
        });
    }

    iw.refresh_nav_actions();
}

#[derive(Clone, Copy)]
enum Nav {
    Previous,
    Next,
    First,
    Last,
}

fn bind<F>(window: &gtk::ApplicationWindow, name: &str, f: F)
where
    F: Fn(Option<&glib::Variant>) + 'static,
{
    let action = gio::SimpleAction::new(name, None);
    action.connect_activate(move |_, param| f(param));
    window.add_action(&action);
}

fn file_to_path(file: Option<gio::File>) -> Option<PathBuf> {
    let file = file?;
    if let Some(path) = file.path() {
        return Some(path);
    }
    // Fallback for URI-only results (rare with in-app chooser).
    let uri = file.uri();
    if let Ok(url) = glib::Uri::parse(&uri, glib::UriFlags::PARSE_RELAXED) {
        if url.scheme().as_str() == "file" {
            let path = url.path();
            if !path.is_empty() {
                return Some(PathBuf::from(path.as_str()));
            }
        }
    }
    eprintln!("gtk-image: cannot resolve local path for {uri}");
    None
}

fn image_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Images"));
    for mime in [
        "image/jpeg",
        "image/png",
        "image/gif",
        "image/webp",
        "image/bmp",
        "image/tiff",
        "image/svg+xml",
        "image/x-icon",
        "image/heic",
        "image/heif",
        "image/avif",
        "image/jxl",
    ] {
        filter.add_mime_type(mime);
    }
    filter.add_pixbuf_formats();
    filter
}

fn save_pixbuf(pb: &gtk::gdk_pixbuf::Pixbuf, path: &Path) -> Result<(), String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let format = match ext.as_str() {
        "jpg" | "jpeg" | "jpe" => "jpeg",
        "png" => "png",
        "bmp" => "bmp",
        "tif" | "tiff" => "tiff",
        "ico" => "ico",
        other => other,
    };
    pb.savev(path, format, &[])
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn setup_context_menu(iw: &Rc<ImageWindow>) {
    let mut icons = gtk_theme::IconMenu::new();
    let menu = gio::Menu::new();
    icons.append_action(&menu, "Copy", "win.copy");
    icons.append_action(&menu, "Save As…", "win.save-as");
    icons.append_action(&menu, "Move to Trash", "win.trash");
    icons.append(
        &menu,
        "Show in Folder",
        "win.show-in-folder",
        "folder-symbolic",
    );

    let transform = gio::Menu::new();
    icons.append(
        &transform,
        "Rotate Clockwise",
        "win.rotate-cw",
        "object-rotate-right-symbolic",
    );
    icons.append(
        &transform,
        "Rotate Counterclockwise",
        "win.rotate-ccw",
        "object-rotate-left-symbolic",
    );
    menu.append_section(None, &transform);

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    icons.bind_popover(&popover);
    popover.set_parent(&iw.view.root);
    popover.set_has_arrow(false);

    {
        let popover_weak = popover.downgrade();
        iw.view.root.connect_destroy(move |_| {
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
            let rect = gdk::Rectangle::new(x as i32, y as i32, 1, 1);
            popover.set_pointing_to(Some(&rect));
            popover.popup();
        });
    }
    iw.view.root.add_controller(gesture);
}

/// Drag the current image out to a file manager or other app (EOG style).
///
/// Button-1 click-and-hold always starts a drag (file URI + image bytes).
/// Pan uses the middle mouse button so it never blocks export.
fn setup_drag_source(iw: &Rc<ImageWindow>) -> gtk::DragSource {
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::COPY);
    drag.set_exclusive(true);

    let iw_prep = Rc::clone(iw);
    drag.connect_prepare(move |_, _x, _y| {
        let path = iw_prep.list.borrow().current()?.to_path_buf();
        content_for_image(&path, iw_prep.view.current_pixbuf().as_ref())
    });

    let iw_begin = Rc::clone(iw);
    drag.connect_drag_begin(move |source, _drag| {
        if let Some(pb) = iw_begin.view.current_pixbuf() {
            let icon = thumbnail_texture(&pb, 128);
            source.set_icon(Some(&icon), icon.width() / 2, icon.height() / 2);
        }
    });

    iw.view.root.add_controller(drag.clone());
    drag
}

fn content_for_image(
    path: &Path,
    pixbuf: Option<&gtk::gdk_pixbuf::Pixbuf>,
) -> Option<gdk::ContentProvider> {
    let file = gio::File::for_path(path);
    let list = gdk::FileList::from_array(&[file.clone()]);
    let typed = gdk::ContentProvider::for_value(&list.to_value());

    let uri_text = format!("{}\r\n", file.uri());
    let uris = gdk::ContentProvider::for_bytes(
        "text/uri-list",
        &glib::Bytes::from(uri_text.as_bytes()),
    );

    if let Some(pb) = pixbuf {
        let texture = gdk::Texture::for_pixbuf(pb);
        let image = gdk::ContentProvider::for_value(&texture.to_value());
        Some(gdk::ContentProvider::new_union(&[typed, uris, image]))
    } else {
        Some(gdk::ContentProvider::new_union(&[typed, uris]))
    }
}

fn thumbnail_texture(pb: &gtk::gdk_pixbuf::Pixbuf, max_edge: i32) -> gdk::Texture {
    use gtk::gdk_pixbuf::InterpType;

    let w = pb.width().max(1);
    let h = pb.height().max(1);
    let scale = (max_edge as f64 / w.max(h) as f64).min(1.0);
    let nw = ((w as f64) * scale).round().max(1.0) as i32;
    let nh = ((h as f64) * scale).round().max(1.0) as i32;
    let scaled = pb
        .scale_simple(nw, nh, InterpType::Bilinear)
        .unwrap_or_else(|| pb.clone());
    gdk::Texture::for_pixbuf(&scaled)
}

fn is_self_drag(drop_target: &gtk::DropTarget, drag_source: &gtk::DragSource) -> bool {
    let Some(drop) = drop_target.current_drop() else {
        return false;
    };
    match (drop.drag(), drag_source.drag()) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

fn setup_drop_target(iw: &Rc<ImageWindow>, drag_source: gtk::DragSource) {
    // Attach on the content stack (capture phase) so drops aren't swallowed by
    // the picture / pan gesture. Accept FileList (file managers) and Gio.File.
    let drop =
        gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY | gdk::DragAction::MOVE);
    drop.set_types(&[gdk::FileList::static_type(), gio::File::static_type()]);
    drop.set_preload(true);
    drop.set_propagation_phase(gtk::PropagationPhase::Capture);

    let iw_drop = Rc::clone(iw);
    let drag_stack = drag_source.clone();
    drop.connect_drop(move |drop_target, value, _x, _y| {
        if is_self_drag(drop_target, &drag_stack) {
            return false;
        }
        let paths = paths_from_drop_value(value);
        let Some(path) = paths.into_iter().find(|p| {
            p.is_dir() || crate::image_list::is_supported_image(p) || p.is_file()
        }) else {
            return false;
        };
        iw_drop.open_path(&path);
        true
    });
    iw.stack.add_controller(drop);

    // Also on the window for drops on the header/empty chrome.
    let drop_win =
        gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY | gdk::DragAction::MOVE);
    drop_win.set_types(&[gdk::FileList::static_type(), gio::File::static_type()]);
    drop_win.set_preload(true);
    let iw_win = Rc::clone(iw);
    let drag_win = drag_source;
    drop_win.connect_drop(move |drop_target, value, _x, _y| {
        if is_self_drag(drop_target, &drag_win) {
            return false;
        }
        let paths = paths_from_drop_value(value);
        let Some(path) = paths.into_iter().next() else {
            return false;
        };
        iw_win.open_path(&path);
        true
    });
    iw.window.add_controller(drop_win);
}

fn paths_from_drop_value(value: &glib::Value) -> Vec<std::path::PathBuf> {
    if let Ok(list) = value.get::<gdk::FileList>() {
        return list.files().into_iter().filter_map(|f| f.path()).collect();
    }
    if let Ok(file) = value.get::<gio::File>() {
        if let Some(p) = file.path() {
            return vec![p];
        }
    }
    Vec::new()
}

/// Open paths from the command line / HANDLES_OPEN into this window.
pub fn open_files(iw: &ImageWindow, files: &[gio::File]) {
    let mut paths: Vec<PathBuf> = files.iter().filter_map(|f| f.path()).collect();
    if paths.is_empty() {
        return;
    }
    // Prefer the first path; if multiple files from same dir, open first and
    // let directory scan pick up siblings.
    paths.sort();
    iw.open_path(&paths[0]);
}
