//! Build a themed VTE terminal widget and spawn the user's shell in it.
//!
//! Also sets up clickable URL matching (ported from gnome-terminal's
//! terminal-regex.hh patterns).

use gtk4 as gtk;
use gtk::{gio, glib};
use vte4::prelude::*;

use crate::config::{Config, CursorBlinkSetting};

/// URL regex patterns ported from gnome-terminal.
/// These detect http(s), ftp, file, voip, and bare www/ftp hostnames,
/// as well as email addresses.
const URL_REGEX_PATTERNS: &[&str] = &[
    // scheme://… (http, https, ftp, ftps, sftp, …)
    r#"(?i)(https?|ftps?|sftp|webcal|telnet|nntp|news)://[^\s<>"'(){}\[\]]+[^\s<>"'(){}\[\].,;:!?)]"#,
    // file:///…
    r#"(?i)file:///[^\s<>"']+"#,
    // www.host… (bare, no scheme)
    r#"(?i)\bwww\.[a-z0-9][-a-z0-9]*(\.[a-z0-9][-a-z0-9]*)+(/[^\s<>"']*)?"#,
    // ftp.host… (bare, no scheme)
    r#"(?i)\bftp\.[a-z0-9][-a-z0-9]*(\.[a-z0-9][-a-z0-9]*)+(/[^\s<>"']*)?"#,
    // email addresses
    r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b",
];

/// Create a fully configured `vte4::Terminal` with the user's theme applied,
/// URL matching registered, and the login shell already spawned.
pub fn build_terminal(config: &Config) -> vte4::Terminal {
    let terminal = vte4::Terminal::new();
    apply_settings(&terminal, config);

    let palette = config.palette_rgba();
    let palette_refs: Vec<&gtk::gdk::RGBA> = palette.iter().collect();
    terminal.set_colors(
        Some(&config.foreground_rgba()),
        Some(&config.background_rgba()),
        &palette_refs,
    );

    register_url_patterns(&terminal);
    spawn_shell(&terminal);
    terminal
}

/// Apply preference settings (font, scrollback, cursor, …) to an existing terminal.
pub fn apply_settings(terminal: &vte4::Terminal, config: &Config) {
    if config.use_custom_font && !config.font.trim().is_empty() {
        let font = gtk::pango::FontDescription::from_string(&config.font);
        terminal.set_font(Some(&font));
    } else {
        terminal.set_font(None);
    }

    terminal.set_cell_width_scale(config.cell_width_scale.clamp(0.5, 4.0));
    terminal.set_cell_height_scale(config.cell_height_scale.clamp(0.5, 4.0));

    let scrollback = if config.limit_scrollback {
        config.scrollback_lines.max(0)
    } else {
        -1
    };
    terminal.set_scrollback_lines(scrollback);

    terminal.set_mouse_autohide(true);
    terminal.set_scroll_on_output(config.scroll_on_output);
    terminal.set_scroll_on_keystroke(config.scroll_on_keystroke);
    terminal.set_scroll_on_insert(config.scroll_on_paste);
    terminal.set_allow_hyperlink(true);
    terminal.set_audible_bell(config.audible_bell);

    terminal.set_cursor_blink_mode(match config.cursor_blink_mode {
        CursorBlinkSetting::System => vte4::CursorBlinkMode::System,
        CursorBlinkSetting::On => vte4::CursorBlinkMode::On,
        CursorBlinkSetting::Off => vte4::CursorBlinkMode::Off,
    });

    terminal.set_cursor_shape(match config.cursor_shape.to_ascii_lowercase().as_str() {
        "ibeam" | "i-beam" => vte4::CursorShape::Ibeam,
        "underline" => vte4::CursorShape::Underline,
        _ => vte4::CursorShape::Block,
    });

    terminal.set_text_blink_mode(match config.text_blink.to_ascii_lowercase().as_str() {
        "never" => vte4::TextBlinkMode::Never,
        "focused" => vte4::TextBlinkMode::Focused,
        "unfocused" => vte4::TextBlinkMode::Unfocused,
        _ => vte4::TextBlinkMode::Always,
    });
}

/// Recolor an existing terminal to match a built-in profile (leaves font as-is).
pub fn apply_profile(terminal: &vte4::Terminal, profile: &gtk_theme::Profile) {
    let palette = profile.palette_rgba();
    let palette_refs: Vec<&gtk::gdk::RGBA> = palette.iter().collect();
    terminal.set_colors(
        Some(&profile.foreground_rgba()),
        Some(&profile.background_rgba()),
        &palette_refs,
    );
}

/// Register URL regex patterns so VTE highlights them and
/// `check_match_at()` can find them for Ctrl+click.
fn register_url_patterns(terminal: &vte4::Terminal) {
    // PCRE2_MULTILINE is required by VTE for match regexes
    const PCRE2_MULTILINE: u32 = 0x0000_0400;

    for pattern in URL_REGEX_PATTERNS {
        match vte4::Regex::for_match(pattern, PCRE2_MULTILINE) {
            Ok(re) => {
                let tag = terminal.match_add_regex(&re, 0);
                terminal.match_set_cursor_name(tag, "pointer");
            }
            Err(e) => {
                eprintln!("gtk-term: bad URL regex: {e}");
            }
        }
    }
}

fn spawn_shell(terminal: &vte4::Terminal) {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());

    let argv = [shell.as_str()];
    let envv: &[&str] = &[];

    terminal.spawn_async(
        vte4::PtyFlags::DEFAULT,
        Some(&home),
        &argv,
        envv,
        glib::SpawnFlags::DEFAULT,
        || {},
        -1,
        gio::Cancellable::NONE,
        move |result| {
            if let Err(err) = result {
                eprintln!("gtk-term: failed to spawn shell: {err}");
            }
        },
    );
}
