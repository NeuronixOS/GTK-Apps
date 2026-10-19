//! `~/.config/gtk-apps/gtk-player/config.toml`. Missing files use defaults.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub window_width: i32,
    pub window_height: i32,
    /// Repeat the current item between the timeline handles.
    pub loop_track: bool,
    /// After the last playlist item, start again at the first.
    pub loop_playlist: bool,
    pub playlist_open: bool,
    /// 0.0–1.0
    pub volume: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window_width: 1100,
            window_height: 700,
            loop_track: false,
            loop_playlist: false,
            playlist_open: true,
            volume: 1.0,
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-apps")
        .join("gtk-player")
}

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
                    "gtk-player: {} is invalid ({err}); using defaults",
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
        if self.window_width < 640 {
            self.window_width = 1100;
        }
        if self.window_height < 400 {
            self.window_height = 700;
        }
        self.volume = self.volume.clamp(0.0, 1.0);
    }
}

pub fn save(cfg: &Config) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(text) = toml::to_string_pretty(cfg) {
        let _ = std::fs::write(dir.join("config.toml"), text);
    }
}
