//! Document state wrapping a GtkSource Buffer.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use sourceview5::prelude::*;

use crate::config::EditorConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewlineType {
    Lf,
    Cr,
    CrLf,
}

impl NewlineType {
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Cr => "\r",
            Self::CrLf => "\r\n",
        }
    }
}

pub struct Document {
    pub buffer: sourceview5::Buffer,
    pub path: RefCell<Option<PathBuf>>,
    pub encoding: RefCell<String>,
    pub newline: RefCell<NewlineType>,
    pub mtime: RefCell<Option<std::time::SystemTime>>,
    pub readonly: RefCell<bool>,
    cursor_line: RefCell<i32>,
    cursor_column: RefCell<i32>,
}

impl Document {
    pub fn new(cfg: &EditorConfig) -> Rc<Self> {
        let buffer = sourceview5::Buffer::new(None);
        apply_style_scheme(&buffer, &cfg.scheme);
        buffer.set_highlight_matching_brackets(cfg.bracket_matching);
        buffer.set_max_undo_levels(cfg.max_undo_actions.max(0) as u32);

        Rc::new(Self {
            buffer,
            path: RefCell::new(None),
            encoding: RefCell::new("UTF-8".into()),
            newline: RefCell::new(NewlineType::Lf),
            mtime: RefCell::new(None),
            readonly: RefCell::new(false),
            cursor_line: RefCell::new(0),
            cursor_column: RefCell::new(0),
        })
    }

    pub fn title(&self) -> String {
        match self.path.borrow().as_ref() {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".into()),
            None => "Untitled".into(),
        }
    }

    pub fn is_modified(&self) -> bool {
        self.buffer.is_modified()
    }

    pub fn set_modified(&self, modified: bool) {
        self.buffer.set_modified(modified);
    }

    pub fn text(&self) -> String {
        let start = self.buffer.start_iter();
        let end = self.buffer.end_iter();
        self.buffer.text(&start, &end, true).to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.buffer.begin_irreversible_action();
        self.buffer.set_text(text);
        self.buffer.end_irreversible_action();
        self.buffer.set_modified(false);
    }

    pub fn set_path(&self, path: Option<PathBuf>) {
        *self.path.borrow_mut() = path;
        self.guess_language();
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.path.borrow().clone()
    }

    pub fn guess_language(&self) {
        let Some(path) = self.path.borrow().clone() else {
            return;
        };
        let lm = sourceview5::LanguageManager::default();
        if let Some(lang) = lm.guess_language(Some(path.to_string_lossy().as_ref()), None) {
            self.buffer.set_language(Some(&lang));
        }
    }

    pub fn set_language_id(&self, id: Option<&str>) {
        match id {
            None | Some("plain") | Some("None") => self.buffer.set_language(None::<&sourceview5::Language>),
            Some(id) => {
                let lm = sourceview5::LanguageManager::default();
                self.buffer.set_language(lm.language(id).as_ref());
            }
        }
    }

    pub fn language_id(&self) -> Option<String> {
        self.buffer.language().map(|l| l.id().to_string())
    }

    #[allow(dead_code)]
    pub fn save_cursor(&self) {
        let mark = self.buffer.get_insert();
        let iter = self.buffer.iter_at_mark(&mark);
        *self.cursor_line.borrow_mut() = iter.line();
        *self.cursor_column.borrow_mut() = iter.line_offset();
    }

    pub fn restore_cursor(&self) {
        let line = *self.cursor_line.borrow();
        let col = *self.cursor_column.borrow();
        let mut iter = self.buffer.iter_at_line(line).unwrap_or_else(|| self.buffer.start_iter());
        let end = iter.ends_line();
        if !end {
            let chars = iter.chars_in_line();
            iter.set_line_offset(col.min(chars.saturating_sub(1).max(0)));
        }
        self.buffer.place_cursor(&iter);
    }

    pub fn ensure_trailing_newline(&self) {
        let text = self.text();
        if !text.is_empty() && !text.ends_with('\n') && !text.ends_with('\r') {
            let mut end = self.buffer.end_iter();
            self.buffer.insert(&mut end, "\n");
        }
    }

    pub fn apply_editor_config(&self, cfg: &EditorConfig) {
        apply_style_scheme(&self.buffer, &cfg.scheme);
        self.buffer.set_highlight_matching_brackets(cfg.bracket_matching);
        self.buffer.set_highlight_syntax(cfg.syntax_highlighting);
        self.buffer
            .set_max_undo_levels(cfg.max_undo_actions.max(0) as u32);
    }
}

pub fn apply_style_scheme(buffer: &sourceview5::Buffer, scheme_id: &str) {
    let sm = sourceview5::StyleSchemeManager::default();
    if let Some(scheme) = sm.scheme(scheme_id) {
        buffer.set_style_scheme(Some(&scheme));
    }
}

pub fn wrap_mode_from_str(s: &str) -> gtk::WrapMode {
    match s {
        "none" => gtk::WrapMode::None,
        "char" => gtk::WrapMode::Char,
        "word-char" => gtk::WrapMode::WordChar,
        _ => gtk::WrapMode::Word,
    }
}

pub fn short_title(path: Option<&Path>, modified: bool) -> String {
    let name = path
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".into());
    if modified {
        format!("• {name}")
    } else {
        name
    }
}
