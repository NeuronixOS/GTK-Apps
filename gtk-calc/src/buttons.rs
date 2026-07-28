//! Button pads for Basic / Advanced / Programming modes.
//!
//! Layouts follow GNOME Calculator's Blueprint grids, built in code for GTK4.

use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;

/// Kind of button action.
#[derive(Debug, Clone)]
pub enum BtnAction {
    Clear,
    Digit(u8),
    Point,
    Insert(&'static str),
    Function(&'static str),
    Brackets,
    Square,
    Solve,
}

pub struct ButtonPads {
    pub stack: gtk::Stack,
    pub inv_toggle: gtk::ToggleButton,
    inverse_pairs: Vec<(gtk::Button, gtk::Button)>,
}

impl ButtonPads {
    pub fn new() -> Self {
        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_hexpand(true);
        stack.set_vexpand(true);

        let basic = wrap_pad(&build_basic_grid());
        let (advanced, inv_toggle, inverse_pairs) = build_advanced_pad();
        let programming = wrap_pad(&build_programming_pad());

        let keyboard = gtk::Label::new(Some("Type an equation and press Enter"));
        keyboard.add_css_class("dim-label");
        keyboard.set_margin_top(24);
        keyboard.set_margin_bottom(24);
        keyboard.set_halign(gtk::Align::Center);

        stack.add_titled(&basic, Some("basic"), "Basic");
        stack.add_titled(&advanced, Some("advanced"), "Advanced");
        stack.add_titled(&programming, Some("programming"), "Programming");
        stack.add_titled(&keyboard, Some("keyboard"), "Keyboard");

        Self {
            stack,
            inv_toggle,
            inverse_pairs,
        }
    }

    pub fn set_mode(&self, mode: &str) {
        self.stack.set_visible_child_name(mode);
    }

    pub fn set_inverse(&self, active: bool) {
        for (normal, inverse) in &self.inverse_pairs {
            normal.set_visible(!active);
            inverse.set_visible(active);
        }
    }

    /// Wire every pad button to `handler`.
    pub fn connect_all<F>(&self, handler: F)
    where
        F: Fn(BtnAction) + 'static,
    {
        let handler = Rc::new(handler);
        wire_tree(self.stack.upcast_ref(), handler);
    }
}

fn wrap_pad(child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let wrap = gtk::Box::new(gtk::Orientation::Vertical, 0);
    wrap.add_css_class("math-buttons");
    wrap.set_margin_top(4);
    wrap.set_margin_bottom(4);
    wrap.set_margin_start(4);
    wrap.set_margin_end(4);
    wrap.append(child);
    wrap
}

fn style_grid(grid: &gtk::Grid) {
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);
    grid.set_row_spacing(3);
    grid.set_column_spacing(3);
    grid.set_hexpand(true);
    grid.set_vexpand(true);
    grid.add_css_class("buttons");
}

fn make_btn(label: &str, css: &str, tag: &'static str) -> gtk::Button {
    let btn = gtk::Button::with_label(label);
    btn.set_focus_on_click(false);
    btn.set_hexpand(true);
    btn.set_vexpand(true);
    btn.set_widget_name(tag);
    for cls in css.split_whitespace() {
        if !cls.is_empty() {
            btn.add_css_class(cls);
        }
    }
    btn
}

fn build_basic_grid() -> gtk::Grid {
    // GNOME Calculator–style compact 5×5 pad:
    //   C  (  )  mod  π
    //   7  8  9   ÷   √
    //   4  5  6   ×  x²
    //   1  2  3   −   =
    //   0  .  %   +   (=)
    let grid = gtk::Grid::new();
    style_grid(&grid);

    let specs: &[(&str, &'static str, i32, i32, i32, i32, &str)] = &[
        ("C", "clear", 0, 0, 1, 1, "clear-button"),
        ("(", "paren-l", 1, 0, 1, 1, "parenthesis-button"),
        (")", "paren-r", 2, 0, 1, 1, "parenthesis-button"),
        ("mod", "op-mod", 3, 0, 1, 1, "function-button"),
        ("π", "const-pi", 4, 0, 1, 1, "function-button"),
        ("7", "d7", 0, 1, 1, 1, "number-button"),
        ("8", "d8", 1, 1, 1, 1, "number-button"),
        ("9", "d9", 2, 1, 1, 1, "number-button"),
        ("÷", "op-div", 3, 1, 1, 1, "operator-button"),
        ("√", "op-sqrt", 4, 1, 1, 1, "function-button"),
        ("4", "d4", 0, 2, 1, 1, "number-button"),
        ("5", "d5", 1, 2, 1, 1, "number-button"),
        ("6", "d6", 2, 2, 1, 1, "number-button"),
        ("×", "op-mul", 3, 2, 1, 1, "operator-button"),
        ("x²", "square", 4, 2, 1, 1, "function-button"),
        ("1", "d1", 0, 3, 1, 1, "number-button"),
        ("2", "d2", 1, 3, 1, 1, "number-button"),
        ("3", "d3", 2, 3, 1, 1, "number-button"),
        ("−", "op-sub", 3, 3, 1, 1, "operator-button"),
        ("=", "solve", 4, 3, 1, 2, "suggested-action"),
        ("0", "d0", 0, 4, 1, 1, "number-button"),
        (".", "point", 1, 4, 1, 1, "numeric-point-button"),
        ("%", "op-pct", 2, 4, 1, 1, "percent-button"),
        ("+", "op-add", 3, 4, 1, 1, "operator-button"),
    ];

    for &(label, tag, col, row, w, h, css) in specs {
        grid.attach(&make_btn(label, css, tag), col, row, w, h);
    }

    grid
}

fn build_advanced_pad() -> (gtk::Box, gtk::ToggleButton, Vec<(gtk::Button, gtk::Button)>) {
    let outer = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    outer.set_hexpand(true);
    outer.set_vexpand(true);
    outer.add_css_class("math-buttons");
    outer.set_margin_top(4);
    outer.set_margin_bottom(4);
    outer.set_margin_start(4);
    outer.set_margin_end(4);

    let adv = gtk::Grid::new();
    style_grid(&adv);

    let inv_toggle = gtk::ToggleButton::with_label("⇧⁻¹");
    inv_toggle.set_focus_on_click(false);
    inv_toggle.add_css_class("accent");
    inv_toggle.set_tooltip_text(Some("Inverse functions"));
    inv_toggle.set_widget_name("inverse");
    adv.attach(&inv_toggle, 0, 0, 1, 1);

    let mut pairs = Vec::new();

    let pair_specs: &[(&str, &'static str, &str, &'static str, i32, i32)] = &[
        ("x²", "square", "√", "op-sqrt", 1, 0),
        ("xʸ", "op-pow", "ʸ√", "fn-root", 2, 0),
        ("sin", "fn-sin", "sin⁻¹", "fn-asin", 0, 1),
        ("sinh", "fn-sinh", "sinh⁻¹", "fn-asinh", 1, 1),
        ("cos", "fn-cos", "cos⁻¹", "fn-acos", 0, 2),
        ("cosh", "fn-cosh", "cosh⁻¹", "fn-acosh", 1, 2),
        ("tan", "fn-tan", "tan⁻¹", "fn-atan", 0, 3),
        ("tanh", "fn-tanh", "tanh⁻¹", "fn-atanh", 1, 3),
        ("log", "fn-log", "10ˣ", "op-10pow", 2, 1),
        ("ln", "fn-ln", "eˣ", "fn-exp", 2, 2),
        ("x⁻¹", "fn-inv", "|x|", "fn-abs", 2, 3),
        ("n!", "op-fact", "⌊x⌋", "fn-floor", 3, 0),
    ];

    for &(n_label, n_tag, i_label, i_tag, col, row) in pair_specs {
        let n = make_btn(n_label, "function-button", n_tag);
        let i = make_btn(i_label, "function-button accent", i_tag);
        i.set_visible(false);

        let cell = gtk::Overlay::new();
        cell.set_hexpand(true);
        cell.set_vexpand(true);
        cell.set_child(Some(&n));
        cell.add_overlay(&i);
        i.set_halign(gtk::Align::Fill);
        i.set_valign(gtk::Align::Fill);

        pairs.push((n, i));
        adv.attach(&cell, col, row, 1, 1);
    }

    let singles: &[(&str, &'static str, i32, i32, &str)] = &[
        ("e", "const-e", 3, 1, "function-button"),
        ("π", "const-pi", 3, 2, "function-button"),
        ("%", "op-pct", 3, 3, "percent-button"),
        ("abs", "fn-abs2", 0, 4, "function-button"),
        ("√", "op-sqrt2", 1, 4, "function-button"),
        ("^", "op-pow2", 2, 4, "function-button"),
        ("( )", "brackets", 3, 4, "parenthesis-button"),
    ];
    for &(label, tag, col, row, css) in singles {
        adv.attach(&make_btn(label, css, tag), col, row, 1, 1);
    }

    outer.append(&adv);
    outer.append(&build_basic_grid());

    (outer, inv_toggle, pairs)
}

fn build_programming_pad() -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    outer.set_hexpand(true);
    outer.set_vexpand(true);

    let prog = gtk::Grid::new();
    style_grid(&prog);

    let specs: &[(&str, &'static str, i32, i32, &str)] = &[
        ("∧", "bit-and", 0, 0, "function-button"),
        ("∨", "bit-or", 1, 0, "function-button"),
        ("⊻", "bit-xor", 2, 0, "function-button"),
        ("¬", "bit-not", 3, 0, "function-button"),
        ("≪", "bit-lsh", 0, 1, "function-button"),
        ("≫", "bit-rsh", 1, 1, "function-button"),
        ("⋙", "bit-ursh", 2, 1, "function-button"),
        ("mod", "op-mod", 3, 1, "function-button"),
        ("A", "hex-a", 0, 2, "number-button"),
        ("B", "hex-b", 1, 2, "number-button"),
        ("C", "hex-c", 2, 2, "number-button"),
        ("D", "hex-d", 3, 2, "number-button"),
        ("E", "hex-e", 0, 3, "number-button"),
        ("F", "hex-f", 1, 3, "number-button"),
        ("0x", "pfx-hex", 2, 3, "function-button"),
        ("0b", "pfx-bin", 3, 3, "function-button"),
        ("twos", "fn-twos", 0, 4, "function-button"),
        ("bswap", "fn-bswap", 1, 4, "function-button"),
        ("NAND", "bit-nand", 2, 4, "function-button"),
        ("NOR", "bit-nor", 3, 4, "function-button"),
    ];

    for &(label, tag, col, row, css) in specs {
        prog.attach(&make_btn(label, css, tag), col, row, 1, 1);
    }

    outer.append(&prog);
    outer.append(&build_basic_grid());
    outer
}

fn wire_tree(widget: &gtk::Widget, handler: Rc<dyn Fn(BtnAction)>) {
    if let Ok(btn) = widget.clone().downcast::<gtk::Button>() {
        if btn.type_().name() != "GtkToggleButton" {
            let name = btn.widget_name();
            if let Some(action) = action_from_tag(name.as_str()) {
                let handler = Rc::clone(&handler);
                btn.connect_clicked(move |_| {
                    handler(action.clone());
                });
            }
        }
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        wire_tree(&c, Rc::clone(&handler));
        child = c.next_sibling();
    }
}

fn action_from_tag(tag: &str) -> Option<BtnAction> {
    Some(match tag {
        "clear" => BtnAction::Clear,
        "point" => BtnAction::Point,
        "solve" => BtnAction::Solve,
        "brackets" => BtnAction::Brackets,
        "paren-l" => BtnAction::Insert("("),
        "paren-r" => BtnAction::Insert(")"),
        "square" => BtnAction::Square,
        "d0" => BtnAction::Digit(0),
        "d1" => BtnAction::Digit(1),
        "d2" => BtnAction::Digit(2),
        "d3" => BtnAction::Digit(3),
        "d4" => BtnAction::Digit(4),
        "d5" => BtnAction::Digit(5),
        "d6" => BtnAction::Digit(6),
        "d7" => BtnAction::Digit(7),
        "d8" => BtnAction::Digit(8),
        "d9" => BtnAction::Digit(9),
        "op-div" => BtnAction::Insert("÷"),
        "op-mul" => BtnAction::Insert("×"),
        "op-sub" => BtnAction::Insert("−"),
        "op-add" => BtnAction::Insert("+"),
        "op-mod" => BtnAction::Insert(" mod "),
        "op-pct" => BtnAction::Insert("%"),
        "op-pow" | "op-pow2" => BtnAction::Insert("^"),
        "op-sqrt" | "op-sqrt2" => BtnAction::Insert("√"),
        "op-fact" => BtnAction::Insert("!"),
        "op-10pow" => BtnAction::Insert("10^"),
        "const-pi" => BtnAction::Insert("π"),
        "const-e" => BtnAction::Insert("e"),
        "const-ans" => BtnAction::Insert("ans"),
        "fn-sin" => BtnAction::Function("sin"),
        "fn-cos" => BtnAction::Function("cos"),
        "fn-tan" => BtnAction::Function("tan"),
        "fn-asin" => BtnAction::Function("asin"),
        "fn-acos" => BtnAction::Function("acos"),
        "fn-atan" => BtnAction::Function("atan"),
        "fn-sinh" => BtnAction::Function("sinh"),
        "fn-cosh" => BtnAction::Function("cosh"),
        "fn-tanh" => BtnAction::Function("tanh"),
        "fn-asinh" => BtnAction::Function("asinh"),
        "fn-acosh" => BtnAction::Function("acosh"),
        "fn-atanh" => BtnAction::Function("atanh"),
        "fn-log" => BtnAction::Function("log"),
        "fn-ln" => BtnAction::Function("ln"),
        "fn-exp" => BtnAction::Function("exp"),
        "fn-inv" => BtnAction::Function("inv"),
        "fn-abs" | "fn-abs2" => BtnAction::Function("abs"),
        "fn-floor" => BtnAction::Function("floor"),
        "fn-root" => BtnAction::Function("root"),
        "fn-twos" => BtnAction::Function("twos"),
        "fn-bswap" => BtnAction::Function("bswap"),
        "bit-and" => BtnAction::Insert(" ∧ "),
        "bit-or" => BtnAction::Insert(" ∨ "),
        "bit-xor" => BtnAction::Insert(" ⊻ "),
        "bit-not" => BtnAction::Insert("¬"),
        "bit-lsh" => BtnAction::Insert(" ≪ "),
        "bit-rsh" => BtnAction::Insert(" ≫ "),
        "bit-ursh" => BtnAction::Insert(" ⋙ "),
        "bit-nand" => BtnAction::Insert(" ⊼ "),
        "bit-nor" => BtnAction::Insert(" ⊽ "),
        "hex-a" => BtnAction::Insert("A"),
        "hex-b" => BtnAction::Insert("B"),
        "hex-c" => BtnAction::Insert("C"),
        "hex-d" => BtnAction::Insert("D"),
        "hex-e" => BtnAction::Insert("E"),
        "hex-f" => BtnAction::Insert("F"),
        "pfx-hex" => BtnAction::Insert("0x"),
        "pfx-bin" => BtnAction::Insert("0b"),
        _ => return None,
    })
}
