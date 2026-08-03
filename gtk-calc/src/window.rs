//! Main application window — rusty counterpart to MathWindow.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::glib::prelude::*;
use gtk::prelude::*;

use crate::buttons::{BtnAction, ButtonPads};
use crate::config::{self, Config};
use crate::engine::{format_bits, AngleUnit, CalcMode};
use crate::equation::Equation;

pub struct CalcWindow {
    pub window: gtk::ApplicationWindow,
    equation: RefCell<Equation>,
    display: gtk::Entry,
    info_label: gtk::Label,
    history_list: gtk::ListBox,
    history_revealer: gtk::Revealer,
    pads: ButtonPads,
    mode: RefCell<CalcMode>,
    status_bar: gtk::Box,
    angle_label: gtk::Label,
    base_label: gtk::Label,
    bit_label: gtk::Label,
    mode_button: gtk::MenuButton,
}

impl CalcWindow {
    pub fn new(app: &gtk::Application, cfg: &Config) -> Rc<Self> {
        let equation = Equation::new(cfg.angle_unit, cfg.precision, cfg.base, cfg.word_size);

        let display = gtk::Entry::new();
        display.set_hexpand(true);
        gtk::prelude::EditableExt::set_alignment(&display, 1.0);
        display.add_css_class("calc-display");
        display.set_placeholder_text(Some("0"));
        display.set_input_purpose(gtk::InputPurpose::Number);

        let info_label = gtk::Label::new(None);
        info_label.add_css_class("dim-label");
        info_label.add_css_class("info-view");
        info_label.set_xalign(1.0);
        info_label.set_wrap(true);

        let history_list = gtk::ListBox::new();
        history_list.set_selection_mode(gtk::SelectionMode::None);
        history_list.add_css_class("history-view");

        let history_scroll = gtk::ScrolledWindow::builder()
            .child(&history_list)
            .max_content_height(120)
            .propagate_natural_height(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        history_scroll.add_css_class("gtk-content");

        let history_revealer = gtk::Revealer::new();
        history_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        history_revealer.set_child(Some(&history_scroll));
        history_revealer.set_reveal_child(false);

        let display_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        display_box.add_css_class("display-container");
        display_box.add_css_class("gtk-content");
        display_box.set_margin_top(4);
        display_box.set_margin_start(6);
        display_box.set_margin_end(6);
        display_box.set_vexpand(true);
        display_box.append(&history_revealer);
        display_box.append(&display);
        display_box.append(&info_label);

        let pads = ButtonPads::new();
        pads.stack.add_css_class("gtk-content");

        let angle_label = gtk::Label::new(Some(cfg.angle_unit.label()));
        angle_label.add_css_class("dim-label");
        let base_label = gtk::Label::new(Some(&format!("Base {}", cfg.base)));
        base_label.add_css_class("dim-label");
        base_label.set_visible(cfg.mode == CalcMode::Programming);
        let bit_label = gtk::Label::new(None);
        bit_label.add_css_class("dim-label");
        bit_label.set_hexpand(true);
        bit_label.set_xalign(0.0);

        let status = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        status.add_css_class("statusbar");
        status.set_margin_start(6);
        status.set_margin_end(6);
        status.set_margin_top(2);
        status.set_margin_bottom(2);
        status.append(&bit_label);
        status.append(&base_label);
        status.append(&angle_label);
        // Compact Basic mode hides the status strip (GNOME Calculator style).
        status.set_visible(matches!(
            cfg.mode,
            CalcMode::Advanced | CalcMode::Programming
        ));

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main_box.append(&display_box);
        main_box.append(&pads.stack);
        main_box.append(&status);

        let mode_button = gtk::MenuButton::new();
        mode_button.set_label(cfg.mode.label());
        mode_button.set_always_show_arrow(true);
        let (mode_model, mode_icons) = mode_menu();
        mode_button.set_menu_model(Some(&mode_model));
        mode_icons.bind_menu_button(&mode_button);

        // Compact header: Undo | mode (title) | history + menu
        let header = gtk::HeaderBar::new();
        gtk_theme::prepare_headerbar(&header);
        header.set_title_widget(Some(&mode_button));

        let undo_btn = gtk_theme::labeled_button("edit-undo-symbolic", "Undo");
        undo_btn.set_tooltip_text(Some("Undo (Ctrl+Z)"));
        undo_btn.set_action_name(Some("win.undo"));
        undo_btn.add_css_class("flat");
        header.pack_start(&undo_btn);

        let hist_btn = gtk::ToggleButton::new();
        hist_btn.set_icon_name("document-open-recent-symbolic");
        hist_btn.set_tooltip_text(Some("History"));
        hist_btn.add_css_class("flat");
        header.pack_end(&hist_btn);

        let menu_button = build_menu_button();
        header.pack_end(&menu_button);

        let (default_w, default_h) = match cfg.mode {
            CalcMode::Basic | CalcMode::Keyboard => {
                // Migrate away from the old 420×560 default toward a compact portrait.
                let w = if cfg.window_width >= 400 {
                    340
                } else {
                    cfg.window_width.max(300)
                };
                let h = if cfg.window_height >= 540 {
                    520
                } else {
                    cfg.window_height.max(480)
                };
                (w, h)
            }
            _ => (cfg.window_width.max(700), cfg.window_height.max(520)),
        };

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("GTK Calc")
            .default_width(default_w)
            .default_height(default_h)
            .build();
        window.set_titlebar(Some(&header));
        window.set_child(Some(&main_box));

        let cw = Rc::new(Self {
            window,
            equation: RefCell::new(equation),
            display,
            info_label,
            history_list,
            history_revealer,
            pads,
            mode: RefCell::new(cfg.mode),
            status_bar: status,
            angle_label,
            base_label,
            bit_label,
            mode_button,
        });

        cw.pads.set_mode(cfg.mode.as_str());
        install_actions(&cw);
        wire_pads(&cw);
        wire_display(&cw);
        wire_history_toggle(&cw, &hist_btn);
        wire_inverse(&cw);

        {
            let cw_weak = Rc::downgrade(&cw);
            cw.window.connect_close_request(move |win| {
                if !win.is_maximized() && !win.is_fullscreen() {
                    let w = win.width();
                    let h = win.height();
                    if w > 0 && h > 0 {
                        let mut cfg = config::load();
                        cfg.window_width = w;
                        cfg.window_height = h;
                        if let Some(cw) = cw_weak.upgrade() {
                            cfg.mode = *cw.mode.borrow();
                            cfg.angle_unit = cw.equation.borrow().angle;
                            cfg.base = cw.equation.borrow().base;
                            cfg.word_size = cw.equation.borrow().word_size;
                        }
                        config::save(&cfg);
                    }
                }
                glib::Propagation::Proceed
            });
        }

        cw
    }

    pub fn present(&self) {
        self.window.present();
        self.display.grab_focus();
    }

    fn sync_display(&self) {
        let eq = self.equation.borrow();
        // Avoid recursive notify loops by checking
        if self.display.text().as_str() != eq.text() {
            self.display.set_text(eq.text());
            self.display.set_position(eq.cursor() as i32);
        }
        if eq.status.is_empty() {
            self.info_label.set_text("");
        } else {
            self.info_label.set_text(&eq.status);
        }
        self.update_bit_status();
    }

    fn update_bit_status(&self) {
        let eq = self.equation.borrow();
        if *self.mode.borrow() != CalcMode::Programming {
            self.bit_label.set_text("");
            return;
        }
        if let Some(bits) = format_bits(eq.ans, eq.word_size) {
            self.bit_label
                .set_text(&format!("{bits:0width$b}  ({})", eq.word_size, width = eq.word_size as usize));
        } else {
            self.bit_label.set_text("");
        }
    }

    fn refresh_history(&self) {
        while let Some(row) = self.history_list.row_at_index(0) {
            self.history_list.remove(&row);
        }
        let eq = self.equation.borrow();
        for entry in eq.history.iter().rev().take(40) {
            let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row_box.add_css_class("history-entry");
            row_box.set_margin_start(8);
            row_box.set_margin_end(8);
            row_box.set_margin_top(2);
            row_box.set_margin_bottom(2);

            let eq_lbl = gtk::Label::new(Some(&entry.equation));
            eq_lbl.add_css_class("equation-label");
            eq_lbl.set_hexpand(true);
            eq_lbl.set_xalign(0.0);
            eq_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);

            let eq_sym = gtk::Label::new(Some("="));
            eq_sym.add_css_class("equation-symbol");

            let ans_lbl = gtk::Label::new(Some(&entry.answer));
            ans_lbl.add_css_class("answer-label");
            ans_lbl.set_xalign(1.0);

            row_box.append(&eq_lbl);
            row_box.append(&eq_sym);
            row_box.append(&ans_lbl);
            attach_history_context_menu(&row_box, &entry.equation, &entry.answer);
            self.history_list.append(&row_box);
        }
        if !eq.history.is_empty() {
            self.history_revealer.set_reveal_child(true);
        }
    }

    fn set_mode(&self, mode: CalcMode) {
        *self.mode.borrow_mut() = mode;
        self.pads.set_mode(mode.as_str());
        self.mode_button.set_label(mode.label());
        self.base_label
            .set_visible(mode == CalcMode::Programming);
        self.status_bar.set_visible(matches!(
            mode,
            CalcMode::Advanced | CalcMode::Programming
        ));
        let (width, height) = match mode {
            CalcMode::Basic | CalcMode::Keyboard => (340, 520),
            _ => (780, self.window.height().max(520)),
        };
        self.window.set_default_size(width, height);
        self.update_bit_status();
        self.display.grab_focus();
    }

    fn handle_action(&self, action: BtnAction) {
        {
            let mut eq = self.equation.borrow_mut();
            // Sync text from entry first (user may have typed)
            let current = self.display.text().to_string();
            if current != eq.text() {
                eq.set_text(current);
            }

            match action {
                BtnAction::Clear => eq.clear(),
                BtnAction::Digit(d) => eq.insert_digit(d),
                BtnAction::Point => eq.insert("."),
                BtnAction::Insert(s) => eq.insert(s),
                BtnAction::Function(name) => eq.insert_function(name),
                BtnAction::Brackets => eq.insert_brackets(),
                BtnAction::Square => eq.square(),
                BtnAction::Solve => match eq.solve() {
                    Ok(_) => {}
                    Err(e) => eq.set_error(e.to_string()),
                },
            }
        }
        self.sync_display();
        if matches!(action, BtnAction::Solve) {
            self.refresh_history();
        }
    }
}

fn wire_pads(cw: &Rc<CalcWindow>) {
    let weak = Rc::downgrade(cw);
    cw.pads.connect_all(move |action| {
        if let Some(cw) = weak.upgrade() {
            cw.handle_action(action);
        }
    });
}

fn wire_inverse(cw: &Rc<CalcWindow>) {
    let weak = Rc::downgrade(cw);
    cw.pads.inv_toggle.connect_toggled(move |tog| {
        if let Some(cw) = weak.upgrade() {
            cw.pads.set_inverse(tog.is_active());
        }
    });
}

fn wire_history_toggle(cw: &Rc<CalcWindow>, btn: &gtk::ToggleButton) {
    let revealer = cw.history_revealer.clone();
    btn.connect_toggled(move |tog| {
        revealer.set_reveal_child(tog.is_active());
    });
}

fn wire_display(cw: &Rc<CalcWindow>) {
    let weak = Rc::downgrade(cw);
    cw.display.connect_activate(move |_| {
        if let Some(cw) = weak.upgrade() {
            cw.handle_action(BtnAction::Solve);
        }
    });

    // Key controller for Escape / operators when focus is on display
    let key = gtk::EventControllerKey::new();
    let weak = Rc::downgrade(cw);
    key.connect_key_pressed(move |_, keyval, _keycode, _mods| {
        let Some(cw) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        if keyval == gdk::Key::Escape {
            cw.handle_action(BtnAction::Clear);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    cw.display.add_controller(key);

}

fn install_actions(cw: &Rc<CalcWindow>) {
    let win = &cw.window;

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
        // Follow ~/.config/gtk-apps/theme.toml when any suite app changes profile.
        gtk_theme::watch_theme_sync_action(win, "theme");
        gtk_theme::install_open_theme_editor_action(win);
    }

    add_win_action(cw, "undo", |cw| {
        cw.equation.borrow_mut().undo();
        cw.sync_display();
    });
    add_win_action(cw, "redo", |cw| {
        cw.equation.borrow_mut().redo();
        cw.sync_display();
    });
    add_win_action(cw, "clear", |cw| {
        cw.handle_action(BtnAction::Clear);
    });
    add_win_action(cw, "solve", |cw| {
        cw.handle_action(BtnAction::Solve);
    });

    // Mode actions
    for (name, mode) in [
        ("mode-basic", CalcMode::Basic),
        ("mode-advanced", CalcMode::Advanced),
        ("mode-programming", CalcMode::Programming),
        ("mode-keyboard", CalcMode::Keyboard),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(cw);
        action.connect_activate(move |_, _| {
            if let Some(cw) = weak.upgrade() {
                cw.set_mode(mode);
            }
        });
        win.add_action(&action);
    }

    // Angle unit
    for (name, unit) in [
        ("angle-degrees", AngleUnit::Degrees),
        ("angle-radians", AngleUnit::Radians),
        ("angle-gradians", AngleUnit::Gradians),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(cw);
        let label = cw.angle_label.clone();
        action.connect_activate(move |_, _| {
            if let Some(cw) = weak.upgrade() {
                cw.equation.borrow_mut().angle = unit;
                label.set_text(unit.label());
            }
        });
        win.add_action(&action);
    }

    // Base
    for (name, base) in [
        ("base-2", 2u32),
        ("base-8", 8u32),
        ("base-10", 10u32),
        ("base-16", 16u32),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(cw);
        let label = cw.base_label.clone();
        action.connect_activate(move |_, _| {
            if let Some(cw) = weak.upgrade() {
                cw.equation.borrow_mut().base = base;
                label.set_text(&format!("Base {base}"));
                cw.update_bit_status();
            }
        });
        win.add_action(&action);
    }

    // Word size
    for (name, bits) in [
        ("word-8", 8u32),
        ("word-16", 16u32),
        ("word-32", 32u32),
        ("word-64", 64u32),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(cw);
        action.connect_activate(move |_, _| {
            if let Some(cw) = weak.upgrade() {
                cw.equation.borrow_mut().word_size = bits;
                cw.update_bit_status();
            }
        });
        win.add_action(&action);
    }

    let _ = win;
}

fn add_win_action(cw: &Rc<CalcWindow>, name: &str, f: impl Fn(&CalcWindow) + 'static) {
    let action = gio::SimpleAction::new(name, None);
    let weak = Rc::downgrade(cw);
    action.connect_activate(move |_, _| {
        if let Some(cw) = weak.upgrade() {
            f(&cw);
        }
    });
    cw.window.add_action(&action);
}

fn mode_menu() -> (gio::Menu, gtk_theme::IconMenu) {
    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();
    icons.append(
        &menu,
        "Basic",
        "win.mode-basic",
        "accessories-calculator-symbolic",
    );
    icons.append(
        &menu,
        "Advanced",
        "win.mode-advanced",
        "preferences-system-symbolic",
    );
    icons.append(
        &menu,
        "Programming",
        "win.mode-programming",
        "utilities-terminal-symbolic",
    );
    icons.append(
        &menu,
        "Keyboard",
        "win.mode-keyboard",
        "input-keyboard-symbolic",
    );
    (menu, icons)
}

/// Right-click menu on a history row: copy answer, equation, or both.
fn attach_history_context_menu(row: &gtk::Box, equation: &str, answer: &str) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(3); // right mouse button
    let equation = equation.to_string();
    let answer = answer.to_string();
    gesture.connect_pressed(move |gesture, _n, x, y| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        let Some(widget) = gesture.widget() else {
            return;
        };
        show_history_copy_menu(&widget, x, y, &equation, &answer);
    });
    row.add_controller(gesture);
}

fn show_history_copy_menu(
    anchor: &gtk::Widget,
    x: f64,
    y: f64,
    equation: &str,
    answer: &str,
) {
    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();
    icons.append(
        &menu,
        "Copy Answer",
        "history.copy-answer",
        "edit-copy-symbolic",
    );
    icons.append(
        &menu,
        "Copy Equation",
        "history.copy-equation",
        "edit-copy-symbolic",
    );
    icons.append(
        &menu,
        "Copy Both",
        "history.copy-both",
        "edit-copy-symbolic",
    );

    let group = gio::SimpleActionGroup::new();
    let clipboard = anchor.clipboard();

    {
        let clipboard = clipboard.clone();
        let answer = answer.to_string();
        let act = gio::SimpleAction::new("copy-answer", None);
        act.connect_activate(move |_, _| {
            clipboard.set_text(&answer);
        });
        group.add_action(&act);
    }
    {
        let clipboard = clipboard.clone();
        let equation = equation.to_string();
        let act = gio::SimpleAction::new("copy-equation", None);
        act.connect_activate(move |_, _| {
            clipboard.set_text(&equation);
        });
        group.add_action(&act);
    }
    {
        let clipboard = clipboard.clone();
        let both = format!("{equation} = {answer}");
        let act = gio::SimpleAction::new("copy-both", None);
        act.connect_activate(move |_, _| {
            clipboard.set_text(&both);
        });
        group.add_action(&act);
    }

    anchor.insert_action_group("history", Some(&group));

    // Keep the popover alive; dropping it immediately destroys the menu.
    thread_local! {
        static ACTIVE: RefCell<Option<gtk::PopoverMenu>> = const { RefCell::new(None) };
    }

    ACTIVE.with(|slot| {
        if let Some(old) = slot.borrow_mut().take() {
            old.popdown();
            old.unparent();
        }

        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        icons.bind_popover(&popover);
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        popover.set_parent(anchor);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(
            x.round() as i32,
            y.round() as i32,
            1,
            1,
        )));
        popover.popup();
        *slot.borrow_mut() = Some(popover);
    });
}

fn build_menu_button() -> gtk::MenuButton {
    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();

    // Angle/base/word leaves stay plain — option groups (not IconMenu rows).
    let angle = gio::Menu::new();
    angle.append(Some("Degrees"), Some("win.angle-degrees"));
    angle.append(Some("Radians"), Some("win.angle-radians"));
    angle.append(Some("Gradians"), Some("win.angle-gradians"));
    icons.append_submenu(
        &menu,
        "Angle Unit",
        &angle,
        "preferences-system-symbolic",
    );

    let base = gio::Menu::new();
    base.append(Some("Binary"), Some("win.base-2"));
    base.append(Some("Octal"), Some("win.base-8"));
    base.append(Some("Decimal"), Some("win.base-10"));
    base.append(Some("Hexadecimal"), Some("win.base-16"));
    icons.append_submenu(
        &menu,
        "Number Base",
        &base,
        "format-justify-fill-symbolic",
    );

    let word = gio::Menu::new();
    word.append(Some("8-bit"), Some("win.word-8"));
    word.append(Some("16-bit"), Some("win.word-16"));
    word.append(Some("32-bit"), Some("win.word-32"));
    word.append(Some("64-bit"), Some("win.word-64"));
    icons.append_submenu(&menu, "Word Size", &word, "view-list-symbolic");

    gtk_theme::append_profile_menu(&menu, "win.theme");
    icons.append_action(&menu, "Keyboard Shortcuts", "app.shortcuts");
    icons.append_action(&menu, "About", "app.about");

    let btn = gtk::MenuButton::new();
    btn.set_icon_name("open-menu-symbolic");
    btn.set_menu_model(Some(&menu));
    icons.bind_menu_button(&btn);
    btn
}
