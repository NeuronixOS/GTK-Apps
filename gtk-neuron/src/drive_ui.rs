//! Self Driving — right-hand side panel + header toggle (ꔮ).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use gtk4 as gtk;
use gtk::gio;
use gtk::prelude::*;
use serde_json::Value;

use crate::capabilities;
use crate::client::{
    drive_symbolic_image, ensure_drive_icon, CapabilityHandler, NeuronClient, DRIVE_ICON_NAME,
};
use crate::protocol::{CapabilitySpec, ProviderId, ProviderStatus};

static SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

const PANEL_DEFAULT_WIDTH: i32 = 380;
const PANEL_MIN_WIDTH: i32 = 260;

pub struct DriveContext {
    pub app_id: String,
    pub capabilities: Vec<CapabilitySpec>,
    pub handler: CapabilityHandler,
}

/// Header toggle + right-side Self Driving panel.
pub struct DrivingMode {
    pub button: gtk::ToggleButton,
    pub panel: gtk::Box,
    last_width: Rc<Cell<i32>>,
}

impl DrivingMode {
    /// Show/hide the panel; for a plain Box shell (no drag resize).
    pub fn bind(&self) {
        let panel = self.panel.clone();
        self.button.connect_toggled(move |btn| {
            panel.set_visible(btn.is_active());
        });
    }

    /// Show/hide and keep a resizable GtkPaned split (remembers width).
    pub fn bind_paned(&self, paned: &gtk::Paned) {
        let panel = self.panel.clone();
        let paned = paned.clone();
        let last_width = self.last_width.clone();

        {
            let last_width = last_width.clone();
            let panel = panel.clone();
            paned.connect_notify_local(Some("position"), move |p, _| {
                if !panel.is_visible() {
                    return;
                }
                let w = p.width();
                let pos = p.position();
                if w > pos + PANEL_MIN_WIDTH {
                    last_width.set((w - pos).max(PANEL_MIN_WIDTH));
                }
            });
        }

        self.button.connect_toggled(move |btn| {
            let on = btn.is_active();
            panel.set_visible(on);
            if on {
                apply_driving_split(&paned, last_width.get());
            }
        });
    }

    /// Register `win.driving-mode` (Ctrl+D) so menus and shortcuts can open the panel.
    pub fn install_window_action(&self, window: &gtk::ApplicationWindow) {
        let btn = self.button.clone();
        let action = gio::SimpleAction::new_stateful(
            "driving-mode",
            None,
            &false.to_variant(),
        );
        {
            let btn = btn.clone();
            action.connect_activate(move |a, _| {
                let next = !btn.is_active();
                btn.set_active(next);
                a.set_state(&next.to_variant());
            });
        }
        {
            let action = action.clone();
            self.button.connect_toggled(move |b| {
                action.set_state(&b.is_active().to_variant());
            });
        }
        window.add_action(&action);
        if let Some(app) = window.application() {
            app.set_accels_for_action("win.driving-mode", &["<Control>d"]);
        }
    }
}

/// Keep the main app on the left; give Self Driving a right-hand strip.
fn apply_driving_split(paned: &gtk::Paned, panel_w: i32) {
    let w = paned.width().max(1);
    let max_panel = (w * 2 / 3).max(PANEL_MIN_WIDTH);
    let pw = panel_w.clamp(PANEL_MIN_WIDTH, max_panel);
    let pos = (w - pw).max(120.min(w.saturating_sub(1)));
    paned.set_position(pos);
}

/// Append a "Self Driving" entry to a menu (hamburger / menubar section).
pub fn append_driving_menu_item(icons: &mut gtk_theme::IconMenu, menu: &gio::Menu) {
    let _ = ensure_drive_icon();
    icons.append(
        menu,
        "Self Driving",
        "win.driving-mode",
        DRIVE_ICON_NAME,
    );
}

/// Build Self Driving UI (panel starts hidden).
pub fn create_driving_mode(ctx: DriveContext) -> DrivingMode {
    gtk_theme::ensure_adwaita_icons();
    let _ = ensure_drive_icon();

    let button = gtk::ToggleButton::new();
    button.set_tooltip_text(Some("Self Driving (Ctrl+D)"));
    button.add_css_class("flat");
    button.add_css_class("neuron-drive-button");
    if ensure_drive_icon().is_some() {
        button.set_child(Some(&drive_symbolic_image()));
    } else {
        button.set_label("ꔮ");
    }

    let panel = build_driving_panel(ctx);
    panel.set_visible(false);
    panel.set_size_request(PANEL_MIN_WIDTH, -1);
    panel.set_hexpand(false);
    panel.set_vexpand(true);

    DrivingMode {
        button,
        panel,
        last_width: Rc::new(Cell::new(PANEL_DEFAULT_WIDTH)),
    }
}

/// Horizontal shell: `main` on the left, Self Driving panel on the right (draggable).
pub fn wrap_with_driving_panel(
    main: &impl IsA<gtk::Widget>,
    panel: &impl IsA<gtk::Widget>,
) -> gtk::Paned {
    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.add_css_class("neuron-driving-shell");
    paned.set_start_child(Some(main));
    paned.set_end_child(Some(panel));
    paned.set_resize_start_child(true);
    paned.set_shrink_start_child(true);
    // End child keeps its width when the window grows; the handle still lets
    // the user resize Self Driving.
    paned.set_resize_end_child(false);
    paned.set_shrink_end_child(true);
    paned.set_wide_handle(true);
    paned.set_hexpand(true);
    paned.set_vexpand(true);
    paned.connect_map(|p| {
        if p.position() <= 0 {
            apply_driving_split(p, PANEL_DEFAULT_WIDTH);
        }
    });
    paned
}

/// Pack a Self Driving toggle and wrap the window body with a right panel.
pub fn attach_driving_mode(
    header: &gtk::HeaderBar,
    window: &gtk::ApplicationWindow,
    main: &impl IsA<gtk::Widget>,
    ctx: DriveContext,
) -> DrivingMode {
    let mode = create_driving_mode(ctx);
    let shell = wrap_with_driving_panel(main, &mode.panel);
    mode.bind_paned(&shell);
    mode.install_window_action(window);
    header.pack_end(&mode.button);
    window.set_child(Some(&shell));
    mode
}

/// Pack a driving-mode toggle at the end of the header (panel not attached).
pub fn attach_drive_button(header: &gtk::HeaderBar, ctx: DriveContext) {
    let mode = create_driving_mode(ctx);
    header.pack_end(&mode.button);
    unsafe {
        mode.button
            .set_data("gtk-neuron-driving-panel", mode.panel);
    }
}

pub fn make_drive_button(ctx: DriveContext) -> gtk::ToggleButton {
    create_driving_mode(ctx).button
}

struct ProviderTab {
    provider: ProviderId,
    session_id: String,
    status_dot: gtk::Label,
    status_text: gtk::Label,
    transcript: gtk::TextView,
    key_row: gtk::Box,
    key_entry: gtk::Entry,
    connect_btn: gtk::Button,
    prompt: gtk::Entry,
    send: gtk::Button,
    waiting_row: gtk::Box,
    waiting_label: gtk::Label,
    /// True while transcript ends with a "..." placeholder awaiting the first delta.
    pending_ellipsis: Cell<bool>,
    page: gtk::Box,
}

fn build_driving_panel(ctx: DriveContext) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
    root.add_css_class("neuron-driving-panel");
    root.add_css_class("gtk-content");
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);

    let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let glyph = gtk::Label::new(Some("ꔮ"));
    glyph.add_css_class("title-1");
    let title = gtk::Label::new(Some("Self Driving"));
    title.add_css_class("title-2");
    title.set_halign(gtk::Align::Start);
    title.set_hexpand(true);
    title_row.append(&glyph);
    title_row.append(&title);
    root.append(&title_row);

    let blurb = gtk::Label::new(Some(
        "AI can operate this app through its capability API",
    ));
    blurb.set_wrap(true);
    blurb.set_xalign(0.0);
    blurb.add_css_class("dim-label");
    root.append(&blurb);

    let help = gtk::Label::new(Some(capabilities::starter_help(&ctx.app_id)));
    help.set_wrap(true);
    help.set_xalign(0.0);
    help.add_css_class("caption");
    root.append(&help);

    let daemon_status = gtk::Label::new(Some("Connecting to gtk-neurond…"));
    daemon_status.set_xalign(0.0);
    daemon_status.set_wrap(true);
    daemon_status.add_css_class("caption");
    root.append(&daemon_status);

    let notebook = gtk::Notebook::new();
    notebook.set_scrollable(true);
    notebook.set_vexpand(true);
    notebook.set_hexpand(true);

    let panel_seq = SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut tabs: Vec<ProviderTab> = Vec::new();

    for provider in ProviderId::all() {
        let tab = build_provider_tab(&ctx.app_id, &ctx.capabilities, panel_seq, provider);
        let label = provider_tab_label(provider);
        notebook.append_page(&tab.page, Some(&label));
        tabs.push(tab);
    }

    // Default to Gemini tab
    notebook.set_current_page(Some(1));
    root.append(&notebook);

    let confirm_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    confirm_box.set_visible(false);
    let confirm_label = gtk::Label::new(None);
    confirm_label.set_wrap(true);
    confirm_label.set_xalign(0.0);
    let confirm_btns = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let allow_btn = gtk::Button::with_label("Allow");
    allow_btn.add_css_class("suggested-action");
    let deny_btn = gtk::Button::with_label("Deny");
    confirm_btns.append(&allow_btn);
    confirm_btns.append(&deny_btn);
    confirm_box.append(&confirm_label);
    confirm_box.append(&confirm_btns);
    root.append(&confirm_box);

    let tabs = Rc::new(RefCell::new(tabs));
    let pending_id = Rc::new(RefCell::new(None::<String>));
    let session_to_provider: HashMap<String, ProviderId> = {
        let t = tabs.borrow();
        t.iter()
            .map(|tab| (tab.session_id.clone(), tab.provider))
            .collect()
    };
    let session_to_provider = Rc::new(session_to_provider);

    let client = match NeuronClient::start(&ctx.app_id, ctx.capabilities, ctx.handler) {
        Ok(c) => c,
        Err(e) => {
            daemon_status.set_text(&format!("Daemon unavailable: {e}"));
            for tab in tabs.borrow().iter() {
                append_text(
                    &tab.transcript,
                    &format!("ꔮ could not start Self Driving: {e}\n"),
                );
            }
            return root;
        }
    };
    daemon_status.set_text("gtk-neurond connected");

    {
        let tabs = tabs.clone();
        client.set_on_providers(move |list: &[ProviderStatus]| {
            for tab in tabs.borrow().iter() {
                let configured = list
                    .iter()
                    .find(|p| p.id == tab.provider)
                    .map(|p| p.configured)
                    .unwrap_or(false);
                set_connection_status(&tab.status_dot, &tab.status_text, configured);
            }
        });
    }

    {
        let tabs = tabs.clone();
        let session_to_provider = session_to_provider.clone();
        client.set_on_delta(move |sid, text| {
            if let Some(provider) = session_to_provider.get(sid) {
                if let Some(tab) = tabs.borrow().iter().find(|t| t.provider == *provider) {
                    if tab.waiting_row.is_visible() {
                        tab.waiting_label.set_text("Generating…");
                    }
                    if tab.pending_ellipsis.get() {
                        replace_trailing_ellipsis(&tab.transcript, text);
                        tab.pending_ellipsis.set(false);
                    } else {
                        append_text(&tab.transcript, text);
                    }
                }
            }
        });
    }
    {
        let tabs = tabs.clone();
        let session_to_provider = session_to_provider.clone();
        client.set_on_done(move |sid, _text| {
            if let Some(provider) = session_to_provider.get(sid) {
                if let Some(tab) = tabs.borrow().iter().find(|t| t.provider == *provider) {
                    if tab.pending_ellipsis.get() {
                        replace_trailing_ellipsis(&tab.transcript, "");
                        tab.pending_ellipsis.set(false);
                    }
                    append_text(&tab.transcript, "\n");
                    set_tab_busy(tab, false);
                }
            }
        });
    }
    {
        let tabs = tabs.clone();
        let session_to_provider = session_to_provider.clone();
        client.set_on_error(move |sid, msg| {
            // Empty sid → broadcast to active-looking tabs / all
            if sid.is_empty() {
                for tab in tabs.borrow().iter() {
                    if tab.pending_ellipsis.get() {
                        replace_trailing_ellipsis(&tab.transcript, "");
                        tab.pending_ellipsis.set(false);
                    }
                    append_text(&tab.transcript, &format!("\n[error] {msg}\n"));
                    set_tab_busy(tab, false);
                }
                return;
            }
            if let Some(provider) = session_to_provider.get(sid) {
                if let Some(tab) = tabs.borrow().iter().find(|t| t.provider == *provider) {
                    if tab.pending_ellipsis.get() {
                        replace_trailing_ellipsis(&tab.transcript, "");
                        tab.pending_ellipsis.set(false);
                    }
                    append_text(&tab.transcript, &format!("\n[error] {msg}\n"));
                    set_tab_busy(tab, false);
                }
            }
        });
    }

    {
        let confirm_box = confirm_box.clone();
        let confirm_label = confirm_label.clone();
        let pending_id = pending_id.clone();
        let tabs = tabs.clone();
        let notebook = notebook.clone();
        client.set_on_propose(move |id, name, args, requires| {
            *pending_id.borrow_mut() = Some(id);
            let summary = format!("Allow tool {name}?\n{}", pretty_args(&args));
            confirm_label.set_text(&summary);
            confirm_box.set_visible(requires || true);
            if let Some(page) = notebook.current_page() {
                if let Some(tab) = tabs.borrow().get(page as usize) {
                    append_text(&tab.transcript, &format!("\nꔮ proposes {name}\n"));
                }
            }
        });
    }

    {
        let client = client.clone();
        let pending_id = pending_id.clone();
        let confirm_box = confirm_box.clone();
        allow_btn.connect_clicked(move |_| {
            if let Some(id) = pending_id.borrow_mut().take() {
                let _ = client.confirm_capability(&id, true);
            }
            confirm_box.set_visible(false);
        });
    }
    {
        let client = client.clone();
        let pending_id = pending_id.clone();
        let confirm_box = confirm_box.clone();
        deny_btn.connect_clicked(move |_| {
            if let Some(id) = pending_id.borrow_mut().take() {
                let _ = client.confirm_capability(&id, false);
            }
            confirm_box.set_visible(false);
        });
    }

    // Wire Connect + Send for each tab
    wire_provider_tabs(&client, &tabs);
    let _ = client.send(crate::protocol::Message::ProvidersList);

    root
}

fn provider_tab_label(provider: ProviderId) -> gtk::Label {
    gtk::Label::new(Some(provider.label()))
}

fn show_driving_help(
    parent: &impl IsA<gtk::Widget>,
    app_id: &str,
    provider: ProviderId,
    caps: &[CapabilitySpec],
) {
    let provider_name = provider.label();
    let app_name = capabilities::app_display_name(app_id);
    let connect = capabilities::provider_connect_help(provider);

    let dialog = gtk::Window::builder()
        .title(&format!("Self Driving Help — {app_name}"))
        .modal(true)
        .default_width(560)
        .default_height(640)
        .build();
    if let Some(win) = parent
        .root()
        .and_then(|r| r.downcast::<gtk::Window>().ok())
    {
        dialog.set_transient_for(Some(&win));
        dialog.set_destroy_with_parent(true);
    }

    let header = gtk::HeaderBar::new();
    gtk_theme::prepare_headerbar(&header);
    let close_hdr = gtk::Button::with_label("Close");
    close_hdr.add_css_class("flat");
    {
        let dialog = dialog.clone();
        close_hdr.connect_clicked(move |_| dialog.close());
    }
    header.pack_end(&close_hdr);
    dialog.set_titlebar(Some(&header));

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outer.add_css_class("gtk-content");
    outer.set_hexpand(true);
    outer.set_vexpand(true);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(18);
    content.set_margin_end(18);

    let hero = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let glyph = gtk::Label::new(Some("ꔮ"));
    glyph.add_css_class("title-1");
    let hero_text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let hero_title = gtk::Label::new(Some(&format!("{provider_name} + {app_name}")));
    hero_title.add_css_class("title-2");
    hero_title.set_halign(gtk::Align::Start);
    let hero_sub = gtk::Label::new(Some(
        "Connect this provider, then ask in plain language — Self Driving uses the app’s local API.",
    ));
    hero_sub.add_css_class("dim-label");
    hero_sub.set_wrap(true);
    hero_sub.set_xalign(0.0);
    hero_text.append(&hero_title);
    hero_text.append(&hero_sub);
    hero.append(&glyph);
    hero.append(&hero_text);
    content.append(&hero);

    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    content.append(&help_section_heading(connect.title));
    content.append(&help_body_label(connect.intro));

    let link = gtk::LinkButton::with_label(connect.link_url, connect.link_label);
    link.set_halign(gtk::Align::Start);
    link.set_margin_top(4);
    link.set_margin_bottom(4);
    content.append(&link);

    for (i, step) in connect.steps.iter().enumerate() {
        content.append(&help_step_row(i + 1, step));
    }
    for note in connect.notes {
        let n = help_body_label(note);
        n.add_css_class("caption");
        n.set_margin_top(6);
        content.append(&n);
    }

    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    content.append(&help_section_heading(&format!("Local API — {app_name}")));
    content.append(&help_body_label(
        "Ask naturally; the model may call these capability tools. Items marked Confirm show Allow / Deny first.",
    ));

    let caps_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    caps_box.set_margin_top(4);
    for cap in caps {
        caps_box.append(&help_capability_row(cap));
    }
    content.append(&caps_box);

    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    content.append(&help_section_heading("Examples"));
    content.append(&help_body_label(capabilities::starter_help(app_id)));

    scroll.set_child(Some(&content));
    outer.append(&scroll);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.set_margin_top(10);
    footer.set_margin_bottom(12);
    footer.set_margin_start(18);
    footer.set_margin_end(18);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    footer.append(&spacer);
    let close = gtk::Button::with_label("Close");
    close.add_css_class("suggested-action");
    {
        let dialog = dialog.clone();
        close.connect_clicked(move |_| dialog.close());
    }
    footer.append(&close);
    outer.append(&footer);

    dialog.set_child(Some(&outer));
    dialog.present();
}

fn help_section_heading(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("heading");
    label.set_halign(gtk::Align::Start);
    label.set_xalign(0.0);
    label
}

fn help_body_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.set_halign(gtk::Align::Start);
    label.add_css_class("dim-label");
    label
}

fn help_step_row(n: usize, text: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.set_margin_top(2);
    let num = gtk::Label::new(Some(&format!("{n}.")));
    num.add_css_class("heading");
    num.set_width_chars(3);
    num.set_halign(gtk::Align::Start);
    num.set_valign(gtk::Align::Start);
    let body = gtk::Label::new(Some(text));
    body.set_wrap(true);
    body.set_xalign(0.0);
    body.set_hexpand(true);
    body.set_halign(gtk::Align::Start);
    row.append(&num);
    row.append(&body);
    row
}

fn help_capability_row(cap: &CapabilitySpec) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 2);

    let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let name = gtk::Label::new(None);
    name.set_markup(&format!(
        "<tt><b>{}</b></tt>",
        glib_markup_escape(&cap.name)
    ));
    name.set_halign(gtk::Align::Start);
    name.set_selectable(true);
    title_row.append(&name);
    if cap.requires_confirm {
        let badge = gtk::Label::new(Some("Confirm"));
        badge.add_css_class("caption");
        badge.add_css_class("accent");
        badge.set_tooltip_text(Some("Requires Allow / Deny before running"));
        title_row.append(&badge);
    }
    row.append(&title_row);

    let desc = gtk::Label::new(Some(&cap.description));
    desc.set_wrap(true);
    desc.set_xalign(0.0);
    desc.set_halign(gtk::Align::Start);
    desc.add_css_class("dim-label");
    desc.set_margin_start(4);
    row.append(&desc);
    row
}

fn glib_markup_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;")
}

fn set_connection_status(dot: &gtk::Label, text: &gtk::Label, connected: bool) {
    if connected {
        dot.set_markup("<span foreground=\"#3fb950\" size=\"x-large\">●</span>");
        text.set_text("Connected");
    } else {
        dot.set_markup("<span foreground=\"#f85149\" size=\"x-large\">●</span>");
        text.set_text("Not connected");
    }
}

fn set_tab_busy(tab: &ProviderTab, busy: bool) {
    tab.waiting_row.set_visible(busy);
    if busy {
        tab.waiting_label.set_text("Thinking…");
    }
    tab.send.set_sensitive(!busy);
    tab.prompt.set_sensitive(!busy);
}

/// Adwaita symbolic loading glyph (recolors with panel fg).
fn loading_symbolic_image() -> gtk::Image {
    gtk_theme::ensure_adwaita_icons();
    let image = gtk_theme::symbolic_image("content-loading-symbolic");
    if let Some(display) = gtk::gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        let paintable = theme.lookup_icon(
            "content-loading-symbolic",
            &[],
            gtk_theme::SYMBOLIC_PIXEL_SIZE,
            1,
            gtk::TextDirection::Ltr,
            gtk::IconLookupFlags::FORCE_SYMBOLIC,
        );
        image.set_paintable(Some(&paintable));
    }
    image
}

fn build_provider_tab(
    app_id: &str,
    caps: &[CapabilitySpec],
    panel_seq: u64,
    provider: ProviderId,
) -> ProviderTab {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 8);
    page.set_margin_top(8);
    page.set_margin_bottom(4);
    page.set_margin_start(4);
    page.set_margin_end(4);

    let status_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let status_dot = gtk::Label::new(None);
    let status_text = gtk::Label::new(Some("Not connected"));
    status_text.set_halign(gtk::Align::Start);
    status_text.set_hexpand(true);
    set_connection_status(&status_dot, &status_text, false);

    let status_tip = "AI can operate this app through its capability API";
    status_row.set_tooltip_text(Some(status_tip));
    status_dot.set_tooltip_text(Some(status_tip));
    status_text.set_tooltip_text(Some(status_tip));

    let help_btn = gtk::Button::with_label("Help");
    help_btn.add_css_class("flat");
    help_btn.set_tooltip_text(Some("How to connect and which commands this app supports"));
    help_btn.set_valign(gtk::Align::Center);

    let edit_btn = gtk::Button::with_label("Edit");
    edit_btn.add_css_class("flat");
    edit_btn.set_tooltip_text(Some("Edit API key"));
    edit_btn.set_valign(gtk::Align::Center);

    status_row.append(&status_dot);
    status_row.append(&status_text);
    status_row.append(&help_btn);
    status_row.append(&edit_btn);
    page.append(&status_row);

    {
        let app_id = app_id.to_string();
        let caps = caps.to_vec();
        let page = page.clone();
        help_btn.connect_clicked(move |_| {
            show_driving_help(&page, &app_id, provider, &caps);
        });
    }

    let key_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    key_row.set_visible(false);
    let key_entry = gtk::Entry::new();
    key_entry.set_placeholder_text(Some(provider.key_placeholder()));
    key_entry.set_visibility(false);
    key_entry.set_hexpand(true);
    let connect_btn = gtk::Button::with_label("Save");
    connect_btn.add_css_class("suggested-action");
    key_row.append(&key_entry);
    key_row.append(&connect_btn);
    page.append(&key_row);

    {
        let key_row = key_row.clone();
        let key_entry = key_entry.clone();
        edit_btn.connect_clicked(move |_| {
            let show = !key_row.is_visible();
            key_row.set_visible(show);
            if show {
                key_entry.grab_focus();
            }
        });
    }

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    let transcript = gtk::TextView::new();
    transcript.set_editable(false);
    transcript.set_wrap_mode(gtk::WrapMode::WordChar);
    transcript.set_top_margin(6);
    transcript.set_bottom_margin(6);
    transcript.set_left_margin(6);
    transcript.set_right_margin(6);
    scroll.set_child(Some(&transcript));
    page.append(&scroll);

    let waiting_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    waiting_row.set_visible(false);
    waiting_row.set_halign(gtk::Align::Start);
    let loading_icon = loading_symbolic_image();
    loading_icon.set_valign(gtk::Align::Center);
    let waiting_label = gtk::Label::new(Some("Thinking…"));
    waiting_label.add_css_class("dim-label");
    waiting_label.set_xalign(0.0);
    waiting_row.append(&loading_icon);
    waiting_row.append(&waiting_label);
    page.append(&waiting_row);

    let prompt_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let prompt = gtk::Entry::new();
    prompt.set_placeholder_text(Some("Ask Self Driving…"));
    prompt.set_hexpand(true);
    let send = gtk::Button::with_label("Send");
    send.add_css_class("suggested-action");
    prompt_row.append(&prompt);
    prompt_row.append(&send);
    page.append(&prompt_row);

    let session_id = format!("{}-{}-{}", app_id, provider.as_str(), panel_seq);

    ProviderTab {
        provider,
        session_id,
        status_dot,
        status_text,
        transcript,
        key_row,
        key_entry,
        connect_btn,
        prompt,
        send,
        waiting_row,
        waiting_label,
        pending_ellipsis: Cell::new(false),
        page,
    }
}

fn wire_provider_tabs(client: &Rc<NeuronClient>, tabs: &Rc<RefCell<Vec<ProviderTab>>>) {
    let n = tabs.borrow().len();
    for i in 0..n {
        let client_c = client.clone();
        let client_s = client.clone();
        let tabs_c = tabs.clone();

        let (key_entry, key_row, connect_btn, prompt, send, provider, session_id, transcript) = {
            let tabs = tabs.borrow();
            let tab = &tabs[i];
            (
                tab.key_entry.clone(),
                tab.key_row.clone(),
                tab.connect_btn.clone(),
                tab.prompt.clone(),
                tab.send.clone(),
                tab.provider,
                tab.session_id.clone(),
                tab.transcript.clone(),
            )
        };

        {
            let transcript = transcript.clone();
            let key_entry = key_entry.clone();
            let key_row = key_row.clone();
            let tabs_c = tabs_c.clone();
            connect_btn.connect_clicked(move |_| {
                let key = key_entry.text().to_string();
                if key.is_empty() {
                    return;
                }
                match client_c.set_credential(provider, &key) {
                    Ok(()) => {
                        key_entry.set_text("");
                        key_row.set_visible(false);
                        append_text(
                            &transcript,
                            &format!("Connected {} key.\n", provider.as_str()),
                        );
                        if let Some(tab) = tabs_c.borrow().iter().find(|t| t.provider == provider) {
                            set_connection_status(&tab.status_dot, &tab.status_text, true);
                        }
                        let _ = client_c.send(crate::protocol::Message::ProvidersList);
                    }
                    Err(e) => append_text(&transcript, &format!("Connect failed: {e}\n")),
                }
            });
        }

        {
            let key_entry = key_entry.clone();
            let connect_btn = connect_btn.clone();
            key_entry.connect_activate(move |_| connect_btn.emit_clicked());
        }

        let do_send = {
            let client = client_s;
            let prompt = prompt.clone();
            let transcript = transcript.clone();
            let session_id = session_id.clone();
            let tabs_c = tabs_c.clone();
            Rc::new(move || {
                let text = prompt.text().to_string();
                if text.trim().is_empty() {
                    return;
                }
                append_text(
                    &transcript,
                    &format!("\nYou: {text}\nꔮ ({}): ...", provider.as_str()),
                );
                prompt.set_text("");
                if let Some(tab) = tabs_c.borrow().iter().find(|t| t.provider == provider) {
                    tab.pending_ellipsis.set(true);
                    set_tab_busy(tab, true);
                }
                let _ = client.chat_start(&session_id, provider);
                if let Err(e) = client.chat_send(&session_id, &text) {
                    if let Some(tab) = tabs_c.borrow().iter().find(|t| t.provider == provider) {
                        if tab.pending_ellipsis.get() {
                            replace_trailing_ellipsis(&transcript, "");
                            tab.pending_ellipsis.set(false);
                        }
                        set_tab_busy(tab, false);
                    }
                    append_text(&transcript, &format!("\n[error] {e}\n"));
                }
            })
        };

        {
            let do_send = do_send.clone();
            send.connect_clicked(move |_| do_send());
        }
        {
            let do_send = do_send.clone();
            prompt.connect_activate(move |_| do_send());
        }
    }
}

fn append_text(view: &gtk::TextView, text: &str) {
    let buf = view.buffer();
    let mut end = buf.end_iter();
    buf.insert(&mut end, text);
    let mark = buf.create_mark(None, &buf.end_iter(), false);
    view.scroll_mark_onscreen(&mark);
}

/// Replace a trailing `...` placeholder with `text` (first chunk of the reply).
fn replace_trailing_ellipsis(view: &gtk::TextView, text: &str) {
    let buf = view.buffer();
    let end = buf.end_iter();
    let offset = end.offset();
    if offset >= 3 {
        let mut start = buf.iter_at_offset(offset - 3);
        let mut end = buf.end_iter();
        if buf.text(&start, &end, false).as_str() == "..." {
            buf.delete(&mut start, &mut end);
        }
    }
    if !text.is_empty() {
        append_text(view, text);
    } else {
        let mark = buf.create_mark(None, &buf.end_iter(), false);
        view.scroll_mark_onscreen(&mark);
    }
}

fn pretty_args(args: &Value) -> String {
    serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string())
}

pub fn files_drive_context(handler: CapabilityHandler) -> DriveContext {
    DriveContext {
        app_id: "org.neuronix.GtkFiles".into(),
        capabilities: capabilities::files_capabilities(),
        handler,
    }
}

pub fn term_drive_context(handler: CapabilityHandler) -> DriveContext {
    DriveContext {
        app_id: "org.neuronix.GtkTerm".into(),
        capabilities: capabilities::term_capabilities(),
        handler,
    }
}

pub fn image_drive_context(handler: CapabilityHandler) -> DriveContext {
    DriveContext {
        app_id: "org.neuronix.GtkImage".into(),
        capabilities: capabilities::image_capabilities(),
        handler,
    }
}

pub fn video_drive_context(handler: CapabilityHandler) -> DriveContext {
    DriveContext {
        app_id: "org.neuronix.GtkVideo".into(),
        capabilities: capabilities::video_capabilities(),
        handler,
    }
}

pub fn edit_drive_context(handler: CapabilityHandler) -> DriveContext {
    DriveContext {
        app_id: "org.neuronix.GtkEdit".into(),
        capabilities: capabilities::edit_capabilities(),
        handler,
    }
}
