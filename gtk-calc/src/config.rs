//! Configuration loading for gtk-calc.
//!
//! Reads `~/.config/gtk-apps/gtk-calc/config.toml`. Missing or invalid files fall
//! back to sensible defaults so the app runs with no config at all.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::engine::{AngleUnit, CalcMode};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// Angle unit for trig functions.
    pub angle_unit: AngleUnit,
    /// Significant digits for automatic number formatting.
    pub precision: u32,
    /// Starting calculator mode.
    pub mode: CalcMode,
    /// Number base for programming mode (2, 8, 10, 16).
    pub base: u32,
    /// Word size for bitwise operations (8, 16, 32, 64).
    pub word_size: u32,
    pub window_width: i32,
    pub window_height: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            angle_unit: AngleUnit::Degrees,
            precision: 12,
            mode: CalcMode::Basic,
            base: 10,
            word_size: 64,
            // Compact portrait, close to GNOME Calculator basic mode.
            window_width: 340,
            window_height: 520,
        }
    }
}

/// `~/.config/gtk-apps/gtk-calc` (or `./gtk-apps/gtk-calc` if no config dir is available).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-apps")
        .join("gtk-calc")
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
                    "gtk-calc: {} is invalid ({err}); using defaults",
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
        if self.precision == 0 || self.precision > 17 {
            self.precision = 12;
        }
        if !matches!(self.base, 2 | 8 | 10 | 16) {
            self.base = 10;
        }
        if !matches!(self.word_size, 8 | 16 | 32 | 64) {
            self.word_size = 64;
        }
        if self.window_width < 280 {
            self.window_width = 340;
        }
        if self.window_height < 420 {
            self.window_height = 520;
        }
    }
}

/// Persist window size and mode preferences (best-effort).
pub fn save(cfg: &Config) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("config.toml");
    if let Ok(text) = toml::to_string_pretty(cfg) {
        let _ = std::fs::write(path, text);
    }
}
