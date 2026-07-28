//! Configuration mirroring classic gedit GSettings keys (TOML on disk).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-apps")
        .join("gtk-edit")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn plugins_user_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-edit")
        .join("plugins")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub editor: EditorConfig,
    pub ui: UiConfig,
    pub print: PrintConfig,
    pub encodings: EncodingsConfig,
    pub plugins: PluginsConfig,
    pub state: StateConfig,
    pub session: SessionConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            editor: EditorConfig::default(),
            ui: UiConfig::default(),
            print: PrintConfig::default(),
            encodings: EncodingsConfig::default(),
            plugins: PluginsConfig::default(),
            state: StateConfig::default(),
            session: SessionConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorConfig {
    pub use_default_font: bool,
    pub editor_font: String,
    pub scheme: String,
    pub create_backup_copy: bool,
    pub auto_save: bool,
    pub auto_save_interval: u32,
    pub max_undo_actions: i32,
    pub wrap_mode: String,
    pub tabs_size: u32,
    pub insert_spaces: bool,
    pub auto_indent: bool,
    pub display_line_numbers: bool,
    pub highlight_current_line: bool,
    pub bracket_matching: bool,
    pub display_right_margin: bool,
    pub right_margin_position: u32,
    pub smart_home_end: String,
    pub restore_cursor_position: bool,
    pub syntax_highlighting: bool,
    pub search_highlighting: bool,
    pub ensure_trailing_newline: bool,
    pub backup_copy_extension: String,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            use_default_font: true,
            editor_font: "Monospace 12".into(),
            scheme: "Adwaita-dark".into(),
            create_backup_copy: false,
            auto_save: false,
            auto_save_interval: 10,
            max_undo_actions: 2000,
            wrap_mode: "word".into(),
            tabs_size: 8,
            insert_spaces: false,
            auto_indent: false,
            display_line_numbers: false,
            highlight_current_line: false,
            bracket_matching: false,
            display_right_margin: false,
            right_margin_position: 80,
            smart_home_end: "after".into(),
            restore_cursor_position: true,
            syntax_highlighting: true,
            search_highlighting: true,
            ensure_trailing_newline: true,
            backup_copy_extension: "~".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub toolbar_visible: bool,
    pub notebook_show_tabs_mode: String,
    pub statusbar_visible: bool,
    pub side_panel_visible: bool,
    pub bottom_panel_visible: bool,
    pub max_recents: u32,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            toolbar_visible: false,
            notebook_show_tabs_mode: "always".into(),
            statusbar_visible: true,
            side_panel_visible: false,
            bottom_panel_visible: true,
            max_recents: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PrintConfig {
    pub print_syntax_highlighting: bool,
    pub print_header: bool,
    pub print_wrap_mode: String,
    pub print_line_numbers: u32,
    pub print_font_body: String,
    pub print_font_header: String,
    pub print_font_numbers: String,
}

impl Default for PrintConfig {
    fn default() -> Self {
        Self {
            print_syntax_highlighting: true,
            print_header: true,
            print_wrap_mode: "word".into(),
            print_line_numbers: 0,
            print_font_body: "Monospace 9".into(),
            print_font_header: "Sans 11".into(),
            print_font_numbers: "Sans 8".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EncodingsConfig {
    pub auto_detected: Vec<String>,
    pub shown_in_menu: Vec<String>,
}

impl Default for EncodingsConfig {
    fn default() -> Self {
        Self {
            auto_detected: vec![
                "UTF-8".into(),
                "CURRENT".into(),
                "ISO-8859-15".into(),
                "UTF-16".into(),
            ],
            shown_in_menu: vec!["ISO-8859-15".into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginsConfig {
    /// Plugin module names that are active (gedit-style).
    pub active_plugins: Vec<String>,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            active_plugins: vec![
                "docinfo".into(),
                "modelines".into(),
                "filebrowser".into(),
                "filesearch".into(),
                "markdown".into(),
                "spell".into(),
                "time".into(),
                "todolist".into(),
                "terminal".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StateConfig {
    pub window_width: i32,
    pub window_height: i32,
    pub side_panel_size: i32,
    /// Height of the bottom tools panel (terminal, file search, etc.) in pixels.
    /// Older configs that stored a right-panel width (≥ 280) are remapped on load.
    pub bottom_panel_size: i32,
    /// Last selected side-panel tab: `"documents"` or `"filebrowser"`.
    pub side_panel_page: String,
    pub search_history: Vec<String>,
    pub replace_history: Vec<String>,
    pub recent_files: Vec<String>,
}

impl Default for StateConfig {
    fn default() -> Self {
        Self {
            window_width: 650,
            window_height: 500,
            side_panel_size: 200,
            bottom_panel_size: 200,
            side_panel_page: "documents".into(),
            search_history: Vec::new(),
            replace_history: Vec::new(),
            recent_files: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    pub restore_on_startup: bool,
    pub open_files: Vec<String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            restore_on_startup: true,
            open_files: Vec::new(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(s) => match toml::from_str::<Self>(&s) {
                    Ok(mut c) => {
                        // Right-panel widths were typically ≥ 280px; clamp those
                        // back to a sensible bottom-panel height.
                        if c.state.bottom_panel_size >= 280 {
                            c.state.bottom_panel_size = 200;
                        }
                        c.state.bottom_panel_size = c.state.bottom_panel_size.clamp(120, 480);
                        return c;
                    }
                    Err(e) => eprintln!("gtk-edit: failed to parse {}: {e}", path.display()),
                },
                Err(e) => eprintln!("gtk-edit: failed to read {}: {e}", path.display()),
            }
        }
        let cfg = Self::default();
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = config_dir();
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let s = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(config_path(), s).map_err(|e| e.to_string())
    }

    pub fn add_recent(&mut self, path: &Path) {
        let s = path.to_string_lossy().to_string();
        self.state.recent_files.retain(|p| p != &s);
        self.state.recent_files.insert(0, s);
        let max = self.ui.max_recents as usize;
        if self.state.recent_files.len() > max {
            self.state.recent_files.truncate(max);
        }
    }

    pub fn push_search_history(&mut self, term: &str) {
        if term.is_empty() {
            return;
        }
        self.state.search_history.retain(|t| t != term);
        self.state.search_history.insert(0, term.to_string());
        if self.state.search_history.len() > 10 {
            self.state.search_history.truncate(10);
        }
    }

    pub fn push_replace_history(&mut self, term: &str) {
        if term.is_empty() {
            return;
        }
        self.state.replace_history.retain(|t| t != term);
        self.state.replace_history.insert(0, term.to_string());
        if self.state.replace_history.len() > 10 {
            self.state.replace_history.truncate(10);
        }
    }
}
