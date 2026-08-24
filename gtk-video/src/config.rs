//! Configuration loading for gtk-video.
//!
//! Reads `~/.config/gtk-apps/gtk-video/config.toml`. Missing or invalid files fall
//! back to sensible defaults so the app runs with no config at all.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// Re-encode on export so in/out points are frame-accurate.
    /// `false` uses stream copy (faster, keyframe-aligned).
    pub accurate_export: bool,
    /// When playing, loop inside the selected range instead of stopping at the end.
    pub loop_selection: bool,
    pub window_width: i32,
    pub window_height: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            accurate_export: true,
            loop_selection: false,
            window_width: 960,
            window_height: 640,
        }
    }
}

/// `~/.config/gtk-apps/gtk-video` (or `./gtk-apps/gtk-video` if no config dir is available).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-apps")
        .join("gtk-video")
}

/// Load config from disk, falling back to defaults on any error.
pub fn load() -> Config {
    let path = config_dir().join("config.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(mut cfg) => {
                cfg.sanitize();
                cfg
            }
            Err(err) => {
                eprintln!(
                    "gtk-video: {} is invalid ({err}); using defaults",
                    path.display()
                );
                Config::default()
            }
        },
        Err(_) => Config::default(),
    }
}

impl Config {
    fn sanitize(&mut self) {
        if self.window_width < 480 {
            self.window_width = 960;
        }
        if self.window_height < 320 {
            self.window_height = 640;
        }
    }
}

/// Persist window size and export preferences (best-effort).
pub fn save(cfg: &Config) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("config.toml");
    if let Ok(text) = toml::to_string_pretty(cfg) {
        let _ = std::fs::write(path, text);
    }
}
