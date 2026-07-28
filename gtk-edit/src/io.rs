//! Document load/save with encoding, backups, and external modification checks.

use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use encoding_rs::Encoding;
use gtk4 as gtk;
use gtk::prelude::*;

use crate::config::EditorConfig;
use crate::document::{Document, NewlineType};

pub fn detect_newline(text: &str) -> NewlineType {
    if text.contains("\r\n") {
        NewlineType::CrLf
    } else if text.contains('\r') {
        NewlineType::Cr
    } else {
        NewlineType::Lf
    }
}

fn decode_bytes(bytes: &[u8], preferred: &[&str]) -> (String, String) {
    for name in preferred {
        if *name == "CURRENT" {
            let (cow, _, had_errors) = encoding_rs::UTF_8.decode(bytes);
            if !had_errors {
                return (cow.into_owned(), "UTF-8".into());
            }
            continue;
        }
        if let Some(enc) = Encoding::for_label(name.as_bytes()) {
            let (cow, _, had_errors) = enc.decode(bytes);
            if !had_errors {
                return (cow.into_owned(), enc.name().to_string());
            }
        }
    }
    let (cow, enc, _) = encoding_rs::UTF_8.decode(bytes);
    (cow.into_owned(), enc.name().to_string())
}

/// True when content looks like binary / non-text (not suitable for the editor).
pub fn is_binary_content(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    // NUL bytes are a strong binary signal.
    if bytes.contains(&0) {
        return true;
    }
    // Sample up to 8 KiB for control characters outside common whitespace.
    let sample = &bytes[..bytes.len().min(8192)];
    let mut suspicious = 0usize;
    for &b in sample {
        let is_textish = b == b'\t'
            || b == b'\n'
            || b == b'\r'
            || (0x20..=0x7E).contains(&b)
            || b >= 0x80; // allow UTF-8 / high bytes
        if !is_textish {
            suspicious += 1;
        }
    }
    // More than ~10% odd control bytes → treat as binary.
    suspicious * 10 > sample.len()
}

pub fn non_text_message(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    format!(
        "“{name}” does not appear to be a text file.\n\n\
         GTK Edit can only open plain text documents. \
         Binary files (images, archives, executables, and similar) cannot be edited here."
    )
}

pub fn load_path(
    doc: &Document,
    path: &Path,
    cfg: &EditorConfig,
    auto_encodings: &[String],
) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if is_binary_content(&bytes) {
        return Err(non_text_message(path));
    }
    let prefs: Vec<&str> = auto_encodings.iter().map(|s| s.as_str()).collect();
    let (text, encoding) = decode_bytes(&bytes, &prefs);
    // Reject content that still isn't valid enough as editable text (replacement chars
    // from forced UTF-8 decode of binary can pass the byte heuristic in rare cases).
    let char_count = text.chars().count();
    let replacement_count = text.chars().filter(|c| *c == '\u{FFFD}').count();
    if char_count > 0 && replacement_count > char_count.saturating_div(20).max(8) {
        return Err(non_text_message(path));
    }
    let nl = detect_newline(&text);
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    doc.set_text(&normalized);
    *doc.encoding.borrow_mut() = encoding;
    *doc.newline.borrow_mut() = nl;
    doc.set_path(Some(path.to_path_buf()));
    if let Ok(meta) = fs::metadata(path) {
        *doc.mtime.borrow_mut() = meta.modified().ok();
        *doc.readonly.borrow_mut() = meta.permissions().readonly();
    }
    doc.apply_editor_config(cfg);
    Ok(())
}

pub fn save_path(doc: &Document, path: &Path, cfg: &EditorConfig) -> Result<(), String> {
    if cfg.ensure_trailing_newline {
        doc.ensure_trailing_newline();
    }
    let mut text = doc.text();
    match *doc.newline.borrow() {
        NewlineType::Lf => {}
        NewlineType::Cr => text = text.replace('\n', "\r"),
        NewlineType::CrLf => text = text.replace('\n', "\r\n"),
    }

    let enc_name = doc.encoding.borrow().clone();
    let bytes = if let Some(enc) = Encoding::for_label(enc_name.as_bytes()) {
        let (encoded, _, _) = enc.encode(&text);
        encoded.into_owned()
    } else {
        text.into_bytes()
    };

    if cfg.create_backup_copy && path.exists() {
        let backup = PathBuf::from(format!(
            "{}{}",
            path.display(),
            cfg.backup_copy_extension
        ));
        let _ = fs::copy(path, &backup);
    }

    fs::write(path, &bytes).map_err(|e| e.to_string())?;
    doc.set_path(Some(path.to_path_buf()));
    doc.set_modified(false);
    if let Ok(meta) = fs::metadata(path) {
        *doc.mtime.borrow_mut() = meta.modified().ok();
    }
    Ok(())
}

pub fn externally_modified(doc: &Document) -> bool {
    let Some(path) = doc.path() else {
        return false;
    };
    let Some(old) = *doc.mtime.borrow() else {
        return false;
    };
    match fs::metadata(&path).and_then(|m| m.modified()) {
        Ok(new) => new > old,
        Err(_) => false,
    }
}

pub fn show_io_error(parent: &impl IsA<gtk::Window>, title: &str, message: &str) {
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(title)
        .detail(message)
        .buttons(["OK"])
        .build();
    dialog.show(Some(parent));
}

#[allow(dead_code)]
pub fn confirm_save(
    _parent: &impl IsA<gtk::Window>,
    title: &str,
) -> gtk::AlertDialog {
    gtk::AlertDialog::builder()
        .modal(true)
        .message(format!("Save changes to document \"{title}\" before closing?"))
        .detail("If you don't save, changes will be permanently lost.")
        .buttons(["Close without Saving", "Cancel", "Save"])
        .cancel_button(1)
        .default_button(2)
        .build()
}

/// Autosave helper — returns true if a save was performed.
pub fn maybe_autosave(doc: &Rc<Document>, cfg: &EditorConfig) -> bool {
    if !cfg.auto_save || !doc.is_modified() {
        return false;
    }
    let Some(path) = doc.path() else {
        return false;
    };
    save_path(doc, &path, cfg).is_ok()
}

#[allow(dead_code)]
pub fn file_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}
