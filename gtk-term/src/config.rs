//! Configuration loading and color parsing.
//!
//! Reads `~/.config/gtk-apps/gtk-term/config.toml`. Anything missing falls back to a
//! built-in Gruvbox Dark theme, so the app runs fine with no config at all.
//!
//! Runtime view state (window size, zoom, profile) is persisted separately in
//! `~/.config/gtk-apps/gtk-term/state.toml` and restored on next launch.

use std::path::PathBuf;

use gtk4::gdk::RGBA;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// Pango font string, e.g. "JetBrains Mono 12" or "monospace 11".
    pub font: String,
    /// When false, VTE uses the system monospace font.
    pub use_custom_font: bool,
    /// Initial / preferred terminal grid size.
    pub columns: i64,
    pub rows: i64,
    /// Cell spacing multipliers (1.0 = default).
    pub cell_width_scale: f64,
    pub cell_height_scale: f64,
    /// `never` | `focused` | `unfocused` | `always`
    pub text_blink: String,
    /// `block` | `ibeam` | `underline`
    pub cursor_shape: String,
    /// `system` | `on` | `off`  (legacy bool `cursor_blink` still accepted)
    #[serde(alias = "cursor_blink")]
    pub cursor_blink_mode: CursorBlinkSetting,
    pub audible_bell: bool,
    pub show_scrollbar: bool,
    pub scroll_on_output: bool,
    pub scroll_on_keystroke: bool,
    pub scroll_on_paste: bool,
    /// When false, scrollback is unlimited (`-1` in VTE).
    pub limit_scrollback: bool,
    /// Lines of scrollback history when `limit_scrollback` is true.
    pub scrollback_lines: i64,
    pub colors: Colors,
}

/// Accepts either a string (`"system"`/`"on"`/`"off"`) or a legacy bool.
#[derive(Debug, Clone)]
pub enum CursorBlinkSetting {
    System,
    On,
    Off,
}

impl Default for CursorBlinkSetting {
    fn default() -> Self {
        Self::System
    }
}

impl Serialize for CursorBlinkSetting {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Self::System => "system",
            Self::On => "on",
            Self::Off => "off",
        })
    }
}

impl<'de> Deserialize<'de> for CursorBlinkSetting {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{self, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = CursorBlinkSetting;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("string or bool for cursor blink")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(if v {
                    CursorBlinkSetting::On
                } else {
                    CursorBlinkSetting::Off
                })
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(match v.to_ascii_lowercase().as_str() {
                    "on" | "true" | "always" => CursorBlinkSetting::On,
                    "off" | "false" | "never" => CursorBlinkSetting::Off,
                    _ => CursorBlinkSetting::System,
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

impl CursorBlinkSetting {
    pub fn as_index(&self) -> u32 {
        match self {
            Self::System => 0,
            Self::On => 1,
            Self::Off => 2,
        }
    }

    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Self::On,
            2 => Self::Off,
            _ => Self::System,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Colors {
    pub foreground: String,
    pub background: String,
    /// The 16 ANSI colors (0-7 normal, 8-15 bright). Fewer entries are allowed;
    /// VTE fills the remainder with its defaults.
    pub palette: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font: "monospace 12".to_string(),
            use_custom_font: true,
            columns: 80,
            rows: 24,
            cell_width_scale: 1.0,
            cell_height_scale: 1.0,
            text_blink: "always".to_string(),
            cursor_shape: "block".to_string(),
            cursor_blink_mode: CursorBlinkSetting::System,
            audible_bell: true,
            show_scrollbar: true,
            scroll_on_output: false,
            scroll_on_keystroke: true,
            scroll_on_paste: true,
            limit_scrollback: true,
            scrollback_lines: 10_000,
            colors: Colors::default(),
        }
    }
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            foreground: "#ebdbb2".to_string(),
            background: "#282828".to_string(),
            palette: default_palette(),
        }
    }
}

fn default_palette() -> Vec<String> {
    [
        "#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#a89984",
        "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#ebdbb2",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// `~/.config/gtk-apps/gtk-term` (or `./gtk-apps/gtk-term` if no config dir is available).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-apps")
        .join("gtk-term")
}

/// Load config from disk, falling back to defaults on any error.
pub fn load() -> Config {
    let path = config_dir().join("config.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(cfg) => cfg,
            Err(err) => {
                eprintln!(
                    "gtk-term: {} is invalid ({err}); using defaults",
                    path.display()
                );
                Config::default()
            }
        },
        Err(_) => Config::default(),
    }
}

pub fn save(cfg: &Config) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("config.toml");
    match toml::to_string_pretty(cfg) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                eprintln!("gtk-term: failed to save config: {e}");
            }
        }
        Err(e) => eprintln!("gtk-term: failed to serialize config: {e}"),
    }
}

fn parse_color(value: &str, fallback: &str) -> RGBA {
    value
        .parse::<RGBA>()
        .or_else(|_| fallback.parse::<RGBA>())
        .unwrap_or(RGBA::BLACK)
}

impl Config {
    pub fn foreground_rgba(&self) -> RGBA {
        parse_color(&self.colors.foreground, "#ebdbb2")
    }

    pub fn background_rgba(&self) -> RGBA {
        parse_color(&self.colors.background, "#282828")
    }

    pub fn palette_rgba(&self) -> Vec<RGBA> {
        self.colors
            .palette
            .iter()
            .map(|c| parse_color(c, "#000000"))
            .collect()
    }

    pub fn text_blink_index(&self) -> u32 {
        match self.text_blink.to_ascii_lowercase().as_str() {
            "never" => 0,
            "focused" => 1,
            "unfocused" => 2,
            _ => 3, // always
        }
    }

    pub fn text_blink_from_index(i: u32) -> String {
        match i {
            0 => "never",
            1 => "focused",
            2 => "unfocused",
            _ => "always",
        }
        .into()
    }

    pub fn cursor_shape_index(&self) -> u32 {
        match self.cursor_shape.to_ascii_lowercase().as_str() {
            "ibeam" | "i-beam" => 1,
            "underline" => 2,
            _ => 0,
        }
    }

    pub fn cursor_shape_from_index(i: u32) -> String {
        match i {
            1 => "ibeam",
            2 => "underline",
            _ => "block",
        }
        .into()
    }
}

// ---------------------------------------------------------------------------
// Runtime view state — persisted between sessions.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub window_width: i32,
    pub window_height: i32,
    pub zoom: f64,
    pub profile: String,
    pub maximized: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            window_width: 900,
            window_height: 560,
            zoom: 1.0,
            profile: "gruvbox-dark".to_string(),
            maximized: false,
        }
    }
}

pub fn load_state() -> State {
    let path = config_dir().join("state.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str::<State>(&text).unwrap_or_default(),
        Err(_) => State::default(),
    }
}

pub fn save_state(state: &State) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("state.toml");
    if let Ok(text) = toml::to_string_pretty(state) {
        if let Err(e) = std::fs::write(&path, text) {
            eprintln!("gtk-term: failed to save state: {e}");
        }
    }
}

/// Seed the shared suite theme from gtk-term's previous per-app state once.
pub fn migrate_theme_from_state(state: &State) {
    let shared = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-apps")
        .join("theme.toml");
    if !shared.exists() && gtk_theme::profile_by_id(&state.profile).is_some() {
        gtk_theme::save_theme_id(&state.profile);
    }
}
