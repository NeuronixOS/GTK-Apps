# gtk-calc

A GTK4 calculator written in Rust, implementing the core feature set of
[GNOME Calculator](../Source-Apps/gnome-calculator).

Created by Kevin Hinds — [github.com/khinds10-Neuronix/GTK-Apps](https://github.com/khinds10-Neuronix/GTK-Apps)

- Expression entry with Unicode operators (`× ÷ − √ ∧ ∨ ⊻ ≪ ≫`)
- **Basic**, **Advanced**, **Programming**, and **Keyboard** modes
- Trig / hyperbolic / inverse, logs, roots, factorial, `%`, `mod`
- Bitwise ops, hex/bin/oct literals (`0x`, `0b`, `0o`), `twos`, `bswap`
- Angle units (degrees / radians / gradians), number base, word size
- History tape, undo/redo, `ans`, constants `π` `e` `τ` `φ`
- Optional TOML config and CSS theming
- CLI solve: `gtk-calc --solve '2+3×4'`

Math uses a custom lexer/parser over `f64` (GNOME Calculator uses MPFR/MPC).

## Requirements

- A Rust toolchain (`rustc` / `cargo`) — install via [rustup](https://rustup.rs)
- GTK4 development libraries + `pkg-config`
- Runtime: `adwaita-icon-theme` (menu / toolbar / dialog icons)

### Install system dependencies

**Debian / Ubuntu:**

```bash
sudo apt install build-essential pkg-config libgtk-4-dev adwaita-icon-theme
```

**Fedora:**

```bash
sudo dnf install gcc pkgconf-pkg-config gtk4-devel adwaita-icon-theme
```

**Arch:**

```bash
sudo pacman -S base-devel gtk4 adwaita-icon-theme
```

## Build

From this directory (`GTK4-Apps/gtk-calc`):

```bash
cargo build --release
```

The binary is produced at `target/release/gtk-calc`.

## Run

```bash
# Debug build:
cargo run

# Optimized binary:
./target/release/gtk-calc

# Solve on the command line (no window):
./target/release/gtk-calc --solve 'sin(90)+√16'
```

### Install system-wide (optional)

```bash
cargo build --release
sudo install -Dm755 target/release/gtk-calc /usr/local/bin/gtk-calc
```

## Keyboard shortcuts

| Shortcut           | Action              |
|--------------------|---------------------|
| `Enter`            | Solve               |
| `Escape`           | Clear               |
| `Ctrl+Z`           | Undo                |
| `Ctrl+Shift+Z`     | Redo                |
| `Ctrl+Alt+B`       | Basic mode          |
| `Ctrl+Alt+A`       | Advanced mode       |
| `Ctrl+Alt+P`       | Programming mode    |
| `Ctrl+Alt+K`       | Keyboard mode       |
| `Ctrl+B/O/D/H`     | Base 2/8/10/16      |
| `Ctrl+Q`           | Quit                |

## Configuration

Copy [`examples/config.toml`](examples/config.toml) to
`~/.config/gtk-apps/gtk-calc/config.toml`. Optional CSS:
`~/.config/gtk-apps/gtk-calc/style.css`.

## Not yet ported

Relative to GNOME Calculator, these are deferred:

- Arbitrary-precision MPFR/MPC / complex numbers
- Financial mode dialogs and live currency conversion
- Dedicated unit conversion mode
- Superscript/subscript digit entry, custom functions, GtkSourceView editing

## License

GPL-3.0-or-later (same family as GNOME Calculator).
