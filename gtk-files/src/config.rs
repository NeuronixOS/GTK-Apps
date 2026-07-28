//! Configuration under `~/.config/gtk-apps/gtk-files/config.toml` (suite standard).

use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub window: WindowConfig,
    pub view: ViewConfig,
    pub behavior: BehaviorConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub width: i32,
    pub height: i32,
    pub sidebar_visible: bool,
    pub sidebar_width: i32,
    /// Height of the bottom tools panel (terminal / find in files) in pixels.
    /// Accepts legacy `terminal_width` from older right-sidebar configs.
    #[serde(alias = "terminal_width")]
    pub terminal_height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewConfig {
    /// "list" or "grid"
    pub mode: String,
    pub show_hidden: bool,
    /// name | size | type | modified
    pub sort_by: String,
    pub sort_folders_first: bool,
    pub sort_reversed: bool,
    /// Pixel size used for grid / thumbnail view.
    pub icon_size: i32,
    /// small | medium | large | larger | largest — kept in sync with icon_size.
    pub thumbnail_size: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviorConfig {
    pub confirm_trash: bool,
    pub confirm_delete: bool,
    pub single_click: bool,
    pub open_folders_in_new_tab: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            view: ViewConfig::default(),
            behavior: BehaviorConfig::default(),
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 980,
            height: 640,
            sidebar_visible: true,
            sidebar_width: 200,
            terminal_height: 200,
        }
    }
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            mode: "list".into(),
            show_hidden: false,
            sort_by: "name".into(),
            sort_folders_first: true,
            sort_reversed: false,
            icon_size: 64,
            thumbnail_size: "medium".into(),
        }
    }
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            confirm_trash: true,
            confirm_delete: true,
            single_click: false,
            open_folders_in_new_tab: false,
        }
    }
}

/// Thumbnail size presets for grid view.
pub fn thumbnail_pixels(name: &str) -> i32 {
    match name.to_ascii_lowercase().as_str() {
        "small" => 48,
        "large" => 96,
        "larger" | "xlarge" | "x-large" => 128,
        "largest" | "xxlarge" | "xx-large" => 192,
        _ => 64, // medium / regular
    }
}

pub fn thumbnail_name_for_pixels(px: i32) -> &'static str {
    match px {
        ..=56 => "small",
        57..=80 => "medium",
        81..=112 => "large",
        113..=160 => "larger",
        _ => "largest",
    }
}

/// `~/.config/gtk-apps/gtk-files` (suite standard; same pattern as gtk-edit / gtk-term).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-apps")
        .join("gtk-files")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// Optional user CSS: `~/.config/gtk-apps/gtk-files/style.css`.
pub fn style_path() -> PathBuf {
    config_dir().join("style.css")
}

/// One-shot migration from old cwd / rusty-files / flat `~/.config/gtk-files` locations.
fn migrate_legacy_config_if_needed() {
    let dest = config_path();
    if dest.exists() {
        return;
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("config.toml"));
        // build-launch.sh often uses GTK4-Apps/ as cwd
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join("config.toml"));
        }
    }
    if let Some(home_cfg) = dirs::config_dir() {
        candidates.push(home_cfg.join("rusty-files").join("config.toml"));
        candidates.push(home_cfg.join("gtk-files").join("config.toml"));
    }

    for src in candidates {
        if !src.is_file() || src == dest {
            continue;
        }
        if let Err(e) = fs::create_dir_all(config_dir()) {
            eprintln!("gtk-files: could not create {}: {e}", config_dir().display());
            return;
        }
        match fs::copy(&src, &dest) {
            Ok(_) => {
                eprintln!(
                    "gtk-files: migrated settings from {} → {}",
                    src.display(),
                    dest.display()
                );
                return;
            }
            Err(e) => {
                eprintln!(
                    "gtk-files: failed to migrate {} → {}: {e}",
                    src.display(),
                    dest.display()
                );
            }
        }
    }
}

impl Config {
    pub fn load() -> Self {
        migrate_legacy_config_if_needed();
        let path = config_path();
        let mut cfg: Self = match fs::read_to_string(&path) {
            Ok(text) => {
                let mut loaded: Self = toml::from_str(&text).unwrap_or_default();
                // Old configs stored a right-sidebar width as `terminal_width`
                // (typically ≥ 280). Remap to a usable bottom-panel height.
                if text.contains("terminal_width") && !text.contains("terminal_height") {
                    loaded.window.terminal_height = 200;
                } else if loaded.window.terminal_height >= 280 {
                    loaded.window.terminal_height = 200;
                }
                loaded
            }
            Err(_) => Self::default(),
        };
        // Keep icon_size and thumbnail_size consistent; migrate old XX names.
        cfg.view.thumbnail_size = normalize_thumbnail_name(&cfg.view.thumbnail_size);
        cfg.view.icon_size = thumbnail_pixels(&cfg.view.thumbnail_size);
        cfg.window.terminal_height = cfg.window.terminal_height.clamp(120, 480);
        cfg
    }

    pub fn save(&self) {
        let dir = config_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!("gtk-files: failed to create {}: {e}", dir.display());
            return;
        }
        match toml::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = fs::write(config_path(), text) {
                    eprintln!(
                        "gtk-files: failed to save {}: {e}",
                        config_path().display()
                    );
                }
            }
            Err(e) => eprintln!("gtk-files: failed to serialize config: {e}"),
        }
    }

    pub fn is_grid(&self) -> bool {
        self.view.mode.eq_ignore_ascii_case("grid")
            || self.view.mode.eq_ignore_ascii_case("icon")
            || self.view.mode.eq_ignore_ascii_case("icons")
    }

    pub fn set_thumbnail_size(&mut self, name: &str) {
        self.view.thumbnail_size = normalize_thumbnail_name(name);
        self.view.icon_size = thumbnail_pixels(&self.view.thumbnail_size);
    }
}

fn normalize_thumbnail_name(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "small" => "small".into(),
        "medium" | "regular" => "medium".into(),
        "large" => "large".into(),
        "larger" | "xlarge" | "x-large" => "larger".into(),
        "largest" | "xxlarge" | "xx-large" => "largest".into(),
        other => thumbnail_name_for_pixels(thumbnail_pixels(other)).into(),
    }
}
