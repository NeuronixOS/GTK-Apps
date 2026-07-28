//! Configuration loading for gtk-image.
//!
//! Reads `~/.config/gtk-apps/gtk-image/config.toml`. Missing or invalid files fall
//! back to sensible defaults so the app runs with no config at all.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// Start in best-fit zoom mode.
    pub best_fit: bool,
    /// Multiplicative zoom step for in/out and scroll-wheel zoom.
    pub zoom_step: f64,
    pub zoom_min: f64,
    pub zoom_max: f64,
    pub window_width: i32,
    pub window_height: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            best_fit: true,
            zoom_step: 1.25,
            zoom_min: 0.05,
            zoom_max: 20.0,
            window_width: 960,
            window_height: 640,
        }
    }
}

/// `~/.config/gtk-apps/gtk-image` (or `./gtk-apps/gtk-image` if no config dir is available).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-apps")
        .join("gtk-image")
}

/// Load config from disk, falling back to defaults on any error.
pub fn load() -> Config {
    let path = config_dir().join("config.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(cfg) => cfg,
            Err(err) => {
                eprintln!(
                    "gtk-image: {} is invalid ({err}); using defaults",
                    path.display()
                );
                Config::default()
            }
        },
        Err(_) => Config::default(),
    }
}

/// Persist window size (best-effort; ignores errors).
pub fn save_window_size(width: i32, height: i32) {
    let mut cfg = load();
    if width > 0 {
        cfg.window_width = width;
    }
    if height > 0 {
        cfg.window_height = height;
    }
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("config.toml");
    if let Ok(text) = toml::to_string_pretty(&cfg) {
        let _ = std::fs::write(path, text);
    }
}
