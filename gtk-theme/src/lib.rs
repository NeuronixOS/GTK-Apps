//! Shared color profiles for the GTK4 apps suite.
//!
//! Profiles (Gruvbox Dark by default, plus Tokyo Night, Dracula, Nord, …)
//! are selected from each app's options/menu and persisted in
//! `~/.config/gtk-apps/theme.toml` so the suite stays visually consistent.
//!
//! Also provides Adwaita icon helpers ([`icons`]) for labeled buttons and
//! menu rows. Runtime icons come from the system `adwaita-icon-theme` package.

mod icons;

pub use icons::{
    app_icons, ensure_adwaita_icons, icon_button, icon_for_action, icon_for_label, icon_label_box,
    labeled_button, strip_mnemonic, symbolic_image, IconMenu, MenuIconEntry,
    RUNTIME_ICON_THEME_PACKAGE, SYMBOLIC_PIXEL_SIZE,
};

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::prelude::*;
use serde::{Deserialize, Serialize};

/// A named, built-in color scheme.
#[derive(Debug, Clone, Copy)]
pub struct Profile {
    pub id: &'static str,
    pub name: &'static str,
    pub foreground: &'static str,
    pub background: &'static str,
    pub palette: [&'static str; 16],
}

impl Profile {
    pub fn foreground_rgba(&self) -> gdk::RGBA {
        parse_color(self.foreground, "#ebdbb2")
    }

    pub fn background_rgba(&self) -> gdk::RGBA {
        parse_color(self.background, "#282828")
    }

    pub fn palette_rgba(&self) -> Vec<gdk::RGBA> {
        self.palette
            .iter()
            .map(|c| parse_color(c, "#000000"))
            .collect()
    }

    /// True when the background is dark (drives chrome contrast).
    pub fn is_dark(&self) -> bool {
        relative_luminance(self.background) < 0.45
    }

    /// Elevated surface for header bars / toolbars / side panels.
    pub fn surface_hex(&self) -> String {
        // Blend a little foreground into the background so bars are distinct
        // from the window, without relying on ANSI slot 0 (often == bg).
        mix_hex(self.background, self.foreground, 0.10)
    }

    /// Stronger elevation for selected tabs / hover panels.
    pub fn surface_alt_hex(&self) -> String {
        mix_hex(self.background, self.foreground, 0.18)
    }

    /// Suite accent — ANSI blue slot (palette[4]). Drives Adwaita
    /// `--accent-blue` / `blue_3`, file selection, and text highlights.
    pub fn accent(&self) -> &'static str {
        self.palette[4]
    }
}

/// Owned, editable form of a [`Profile`] — used by the theme editor and for
/// user "custom" profiles persisted to `~/.config/gtk-apps/custom-profiles.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileData {
    pub id: String,
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub palette: Vec<String>,
}

impl ProfileData {
    /// Snapshot of a static [`Profile`] as owned, editable data.
    pub fn from_profile(p: &Profile) -> Self {
        Self {
            id: p.id.to_string(),
            name: p.name.to_string(),
            foreground: p.foreground.to_string(),
            background: p.background.to_string(),
            palette: p.palette.iter().map(|c| c.to_string()).collect(),
        }
    }

    /// Suite accent — palette slot 4 (Blue / `--accent-blue`).
    pub fn accent(&self) -> &str {
        self.palette
            .get(4)
            .map(|s| s.as_str())
            .unwrap_or("#458588")
    }

    /// Ensure exactly 16 palette entries (pad/truncate) for a valid Profile.
    pub fn normalized_palette(&self) -> [String; 16] {
        let mut arr: [String; 16] = Default::default();
        for (i, slot) in arr.iter_mut().enumerate() {
            *slot = self
                .palette
                .get(i)
                .cloned()
                .unwrap_or_else(|| "#000000".to_string());
        }
        arr
    }
}

/// Built-in profiles shown in every app's theme selector.
pub fn builtin_profiles() -> &'static [Profile] {
    &PROFILES
}

/// Whether an id belongs to a user-saved custom profile (not a built-in).
pub fn is_custom_profile(id: &str) -> bool {
    !PROFILES.iter().any(|p| p.id == id) && custom_profile_data(id).is_some()
}

/// Built-in and custom profiles, in menu order (built-ins first, then custom).
pub fn all_profiles() -> Vec<&'static Profile> {
    refresh_custom_registry();
    let mut out: Vec<&'static Profile> = PROFILES.iter().collect();
    CUSTOM_REGISTRY.with(|reg| {
        let reg = reg.borrow();
        let mut customs: Vec<&'static Profile> = reg.values().map(|(_, p)| *p).collect();
        customs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        out.extend(customs);
    });
    out
}

pub fn profile_by_id(id: &str) -> Option<&'static Profile> {
    if let Some(p) = PROFILES.iter().find(|p| p.id == id) {
        return Some(p);
    }
    refresh_custom_registry();
    CUSTOM_REGISTRY.with(|reg| reg.borrow().get(id).map(|(_, p)| *p))
}

pub fn default_profile_id() -> &'static str {
    "gruvbox-dark"
}

pub fn default_profile() -> &'static Profile {
    profile_by_id(default_profile_id()).unwrap_or(&PROFILES[0])
}

/// Current shared theme id (`~/.config/gtk-apps/theme.toml`).
pub fn load_theme_id() -> String {
    let path = theme_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<ThemeFile>(&text) {
            Ok(t) if profile_by_id(&t.profile).is_some() => t.profile,
            _ => default_profile_id().to_string(),
        },
        Err(_) => default_profile_id().to_string(),
    }
}

pub fn save_theme_id(id: &str) {
    if profile_by_id(id).is_none() {
        return;
    }
    let dir = theme_dir();
    let _ = std::fs::create_dir_all(&dir);
    let file = ThemeFile {
        profile: id.to_string(),
    };
    if let Ok(text) = toml::to_string_pretty(&file) {
        let _ = std::fs::write(theme_path(), text);
    }
}

pub fn load_profile() -> &'static Profile {
    profile_by_id(&load_theme_id()).unwrap_or_else(default_profile)
}

// ---------------------------------------------------------------------------
// custom (user) profiles — ~/.config/gtk-apps/custom-profiles.json
// ---------------------------------------------------------------------------

/// Path to the user's custom profile store (JSON array of [`ProfileData`]).
pub fn custom_profiles_path() -> PathBuf {
    theme_dir().join("custom-profiles.json")
}

/// All user-saved custom profiles (empty if the file is missing/invalid).
pub fn load_custom_profiles() -> Vec<ProfileData> {
    match std::fs::read_to_string(custom_profiles_path()) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn write_custom_profiles(list: &[ProfileData]) {
    let dir = theme_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(text) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(custom_profiles_path(), text);
    }
}

/// Owned data for a custom profile id (`None` for built-ins / unknown ids).
pub fn custom_profile_data(id: &str) -> Option<ProfileData> {
    load_custom_profiles().into_iter().find(|p| p.id == id)
}

/// Owned data for any known profile id (built-in or custom).
pub fn profile_data_by_id(id: &str) -> Option<ProfileData> {
    if let Some(p) = PROFILES.iter().find(|p| p.id == id) {
        return Some(ProfileData::from_profile(p));
    }
    custom_profile_data(id)
}

/// Turn a display name into a stable, unique, filesystem-safe custom id.
///
/// Collisions with built-in ids and other custom ids (unless it is the
/// `keep` id being re-saved) get a numeric suffix.
pub fn custom_id_for_name(name: &str, keep: Option<&str>) -> String {
    let base: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let base: String = base
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if base.is_empty() {
        "custom".to_string()
    } else {
        format!("custom-{base}")
    };

    let existing = load_custom_profiles();
    let taken = |candidate: &str| -> bool {
        if Some(candidate) == keep {
            return false;
        }
        PROFILES.iter().any(|p| p.id == candidate)
            || existing.iter().any(|p| p.id == candidate)
    };

    if !taken(&base) {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Insert or replace a custom profile (matched by `data.id`), then refresh the
/// in-process registry so [`profile_by_id`] / [`all_profiles`] see it at once.
pub fn save_custom_profile(data: &ProfileData) {
    let mut list = load_custom_profiles();
    if let Some(slot) = list.iter_mut().find(|p| p.id == data.id) {
        *slot = data.clone();
    } else {
        list.push(data.clone());
    }
    write_custom_profiles(&list);
    refresh_custom_registry();
}

/// Remove a custom profile by id (no-op for built-ins / unknown ids).
pub fn delete_custom_profile(id: &str) {
    let mut list = load_custom_profiles();
    let before = list.len();
    list.retain(|p| p.id != id);
    if list.len() != before {
        write_custom_profiles(&list);
        refresh_custom_registry();
    }
}

/// GTK chrome CSS derived from a profile (window / header / toolbars / menus).
///
/// Intentionally broad so symbolic toolbar buttons, menubars, and panels pick up
/// readable fg/bg instead of leaving Adwaita defaults that fight the profile.
pub fn chrome_css(profile: &Profile) -> String {
    chrome_css_raw(
        profile.foreground,
        profile.background,
        profile.accent(),
        profile.palette[1],
    )
}

/// Like [`chrome_css`] but for in-progress (unsaved) [`ProfileData`] — used by
/// the theme editor for live preview without persisting anything.
pub fn chrome_css_data(data: &ProfileData) -> String {
    let palette = data.normalized_palette();
    chrome_css_raw(
        &data.foreground,
        &data.background,
        data.accent(),
        palette.get(1).map(|s| s.as_str()).unwrap_or("#cc241d"),
    )
}

/// Chrome CSS from raw colors so both static [`Profile`]s and editable
/// [`ProfileData`] share one (large) template.
fn chrome_css_raw(fg: &str, bg: &str, accent: &str, accent_red: &str) -> String {
    let surface = mix_hex(bg, fg, 0.10);
    let surface_alt = mix_hex(bg, fg, 0.18);
    let is_dark = relative_luminance(bg) < 0.45;
    let border = format!("alpha({fg}, 0.18)");
    let hover = format!("alpha({fg}, 0.10)");
    let active = format!("alpha({fg}, 0.16)");
    // Suggested-action label: light on dark accents, dark on light accents.
    let on_accent = if relative_luminance(accent) < 0.55 {
        "#fbf1c7"
    } else {
        "#1d2021"
    };
    // Window controls (min/max/close): muted icon, brighter on hover — no bg wash.
    let (wc_icon, wc_icon_hover) = if is_dark {
        (
            format!("alpha({fg}, 0.55)"),
            mix_hex(fg, "#ffffff", 0.45),
        )
    } else {
        (
            format!("alpha({fg}, 0.50)"),
            mix_hex(fg, "#000000", 0.35),
        )
    };
    // Editor body text: on dark profiles, soften near-white fg to a soft grey
    // so gtk-edit isn't harsh bright white on black.
    let editor_fg = if is_dark {
        mix_hex(fg, bg, 0.20)
    } else {
        fg.to_string()
    };

    format!(
        r#"
/* ---- suite accent → Adwaita --accent-blue / blue_3 ----
 * Palette slot 4 is the configurable accent used for selection, tabs,
 * suggested buttons, and text highlights across all suite apps.
 */
@define-color accent_bg_color {accent};
@define-color accent_fg_color {on_accent};
@define-color accent_color {accent};
@define-color theme_selected_bg_color {accent};
@define-color theme_selected_fg_color {on_accent};
@define-color theme_unfocused_selected_bg_color {accent};
@define-color theme_unfocused_selected_fg_color {on_accent};
@define-color blue_3 {accent};

:root {{
  --accent-bg-color: {accent};
  --accent-fg-color: {on_accent};
  --accent-color: {accent};
  --accent-blue: {accent};
  --blue-3: {accent};
}}

/* ---- window chrome ----
 * Clear Adwaita background-image layers — otherwise modal gtk::Windows keep a
 * light cream shell while our buttons go dark (unreadable labels).
 */
window, window.csd, window.solid-csd {{
  background: {bg};
  background-color: {bg};
  background-image: none;
  color: {fg};
}}
window label, window .title {{
  color: {fg};
}}

/* ---- header / menu / toolbar bars ----
 * Adwaita-dark paints headerbars with `background` + gradient / image layers.
 * Setting only background-color leaves those dark layers visible under a light
 * suite profile — clear background-image and use the shorthand.
 */
headerbar,
headerbar.default-decoration,
headerbar:backdrop,
.titlebar,
.titlebar:backdrop,
menubar,
popovermenubar,
.menubar,
.toolbar,
toolbar,
actionbar,
.meld-actionbar {{
  background: {surface};
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-bottom-color: {border};
  box-shadow: none;
}}
headerbar {{
  border-bottom: 1px solid {border};
}}
.toolbar, toolbar {{
  border-bottom: 1px solid {border};
  padding: 2px 4px;
}}
menubar, popovermenubar, .menubar {{
  border-bottom: 1px solid {border};
}}
/* gtk-meld path bars — Adwaita ActionBar picks up accent tint (purple). */
actionbar > revealer > box,
.meld-actionbar > revealer > box {{
  background: {surface};
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-color: {border};
  box-shadow: none;
}}
headerbar *,
menubar *,
popovermenubar *,
.toolbar *,
toolbar *,
actionbar *,
.meld-actionbar * {{
  color: {fg};
}}
headerbar button,
menubar button,
popovermenubar button,
.toolbar button,
toolbar button,
button.flat,
button.image-button {{
  color: {fg};
  background-color: transparent;
  background-image: none;
  border: none;
  box-shadow: none;
}}
headerbar button:hover,
.toolbar button:hover,
toolbar button:hover,
button.flat:hover,
button.image-button:hover {{
  background-color: {hover};
  background-image: none;
  color: {fg};
}}
headerbar button:active,
headerbar button:checked,
.toolbar button:active,
.toolbar button:checked,
toolbar button:active,
toolbar button:checked {{
  background-color: {active};
  background-image: none;
  color: {fg};
}}

/* Window chrome controls (min / max / close): no background wash; hover
 * only brightens the symbolic icon (lighter grey on dark themes, deeper
 * grey/black on light themes).
 */
windowcontrols button,
windowcontrols button.minimize,
windowcontrols button.maximize,
windowcontrols button.close,
windowcontrols button:backdrop,
headerbar windowcontrols button,
.titlebar windowcontrols button {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  border: none;
  box-shadow: none;
  outline: none;
  color: {wc_icon};
}}
windowcontrols button image {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  color: {wc_icon};
  -gtk-icon-filter: none;
  opacity: 1;
}}
windowcontrols button:hover,
windowcontrols button:active,
windowcontrols button:checked,
windowcontrols button.minimize:hover,
windowcontrols button.minimize:active,
windowcontrols button.maximize:hover,
windowcontrols button.maximize:active,
windowcontrols button.close:hover,
windowcontrols button.close:active,
headerbar windowcontrols button:hover,
headerbar windowcontrols button:active,
.titlebar windowcontrols button:hover,
.titlebar windowcontrols button:active {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  border: none;
  box-shadow: none;
  outline: none;
  color: {wc_icon_hover};
}}
windowcontrols button:hover image,
windowcontrols button:active image {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  color: {wc_icon_hover};
  -gtk-icon-filter: none;
  opacity: 1;
}}

/* ---- generic buttons / entries ----
 * Always set label color explicitly — Adwaita often keeps dark label text on
 * our recolored button backgrounds (unreadable Cancel / Create dialogs).
 */
button {{
  color: {fg};
  background: {surface_alt};
  background-color: {surface_alt};
  background-image: none;
  border: 1px solid {border};
  box-shadow: none;
}}
button label {{
  color: {fg};
}}
button:hover {{
  background: {hover};
  background-color: {hover};
  background-image: none;
  color: {fg};
}}
button:hover label {{
  color: {fg};
}}
button.suggested-action,
button.suggested-action:hover,
button.suggested-action:active {{
  background: {accent};
  background-color: {accent};
  background-image: none;
  color: {on_accent};
  border-color: {accent};
}}
button.suggested-action label,
button.suggested-action:hover label {{
  color: {on_accent};
}}
button.destructive-action,
button.destructive-action:hover {{
  background: {accent_red};
  background-color: {accent_red};
  background-image: none;
  color: #ffffff;
  border-color: {accent_red};
}}
button.destructive-action label {{
  color: #ffffff;
}}
entry, searchentry, spinbutton, textview {{
  background-color: {bg};
  color: {fg};
  caret-color: {fg};
  border-color: {border};
}}
entry:focus, searchentry:focus {{
  border-color: {accent};
}}
entry selection {{
  background-color: alpha({accent}, 0.45);
  color: {fg};
}}

/* Editor / TextView selection — match suite accent (not stock Adwaita blue). */
textview selection,
textview text selection,
.textview selection,
.sourceview selection,
.editor-view selection,
.editor-view text selection,
textview.gtk-edit-view selection,
textview.gtk-edit-view text selection {{
  background-color: alpha({accent}, 0.45);
  color: {fg};
}}

/* ---- side panels / notebooks ----
 * Avoid styling bare listbox/listview (breaks gtk-edit File Browser).
 * gtk-files opts in via .files-view / .file-list / .file-grid.
 */
.sidebar,
.navigation-sidebar,
.navigation-sidebar > row {{
  background-color: {surface};
  background-image: none;
  color: {fg};
}}
.navigation-sidebar > row:hover {{
  background-color: {hover};
  background-image: none;
}}
.navigation-sidebar > row:selected {{
  background-color: alpha({accent}, 0.35);
  background-image: none;
  color: {fg};
}}

/* Opt-in content surfaces (apps add these classes) — kills leftover Adwaita grey */
.gtk-content,
.side-panel,
.editor-view,
.files-view {{
  background-color: {bg};
  background-image: none;
  color: {fg};
}}

/* Side panels / plugin lists (gtk-edit File Browser, etc.) */
.side-panel,
listbox.side-panel,
scrolledwindow.side-panel,
.side-panel listbox,
.side-panel scrolledwindow {{
  background-color: {bg};
  background-image: none;
  color: {fg};
}}
listbox.side-panel row,
.side-panel listbox row {{
  color: {fg};
  background-image: none;
}}
listbox.side-panel row label,
.side-panel listbox row label,
.side-panel label {{
  color: {fg};
}}
listbox.side-panel row:hover,
.side-panel listbox row:hover {{
  background-color: {hover};
  background-image: none;
}}
listbox.side-panel row:selected,
.side-panel listbox row:selected {{
  background-color: alpha({accent}, 0.35);
  background-image: none;
  color: {fg};
}}

/* SourceView / TextView — profile bg + softened editor fg on dark themes */
.editor-view,
textview.editor-view,
textview.gtk-edit-view,
textview.gtk-diff-view,
.sourceview.editor-view {{
  background-color: {bg};
  background-image: none;
  color: {editor_fg};
}}
.editor-view text,
textview.editor-view text,
textview.gtk-edit-view text,
textview.gtk-diff-view text,
.sourceview.editor-view text {{
  background-color: {bg};
  background-image: none;
  color: {editor_fg};
}}
.editor-view border,
textview.editor-view border,
textview.gtk-edit-view border,
textview.gtk-diff-view border {{
  background-color: {surface};
  background-image: none;
  color: alpha({editor_fg}, 0.7);
}}
scrolledwindow.gtk-content,
scrolledwindow.editor-view {{
  background-color: {bg};
  background-image: none;
  color: {editor_fg};
}}

/* gtk-files main listing (ColumnView / GridView) */
.files-view,
.file-list,
.file-grid,
columnview.file-list,
columnview.file-list listview,
gridview.file-grid {{
  background-color: {bg};
  background-image: none;
  color: {fg};
}}
columnview.file-list > header,
columnview.file-list header {{
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-bottom: 1px solid {border};
}}
columnview.file-list header button,
columnview.file-list header label {{
  color: {fg};
  background-color: transparent;
  background-image: none;
}}
.file-list row:hover,
.file-grid child:hover,
columnview.file-list listview row:hover,
gridview.file-grid child:hover {{
  background-color: {hover};
  background-image: none;
}}
.file-list row:selected,
.file-grid child:selected,
columnview.file-list listview row:selected,
gridview.file-grid child:selected {{
  background-color: alpha({accent}, 0.35);
  background-image: none;
  color: {fg};
}}
.file-row-content label,
.file-list label,
.file-grid label {{
  color: {fg};
}}
.file-list label.dim-label,
.file-grid label.dim-label {{
  color: alpha({fg}, 0.65);
}}

/* calc / image content shells */
.calc-display,
.display-container,
.history-view,
.math-buttons,
.image-scroller,
.image-view,
stack.gtk-content {{
  background-color: {bg};
  background-image: none;
  color: {fg};
}}
.calc-display {{
  color: {fg};
  caret-color: {fg};
}}

listview row:hover,
listbox row:hover,
gridview child:hover {{
  background-color: {hover};
  background-image: none;
}}
listview row:selected,
listbox row:selected,
gridview child:selected {{
  background-color: alpha({accent}, 0.35);
  background-image: none;
  color: {fg};
}}
listbox row label,
listview row label {{
  color: inherit;
}}
notebook {{
  background-color: {bg};
  background-image: none;
  color: {fg};
}}
notebook > header {{
  background: {surface};
  background-color: {surface};
  background-image: none;
  border-color: {border};
}}
notebook > header > tabs > tab {{
  background-color: transparent;
  background-image: none;
  color: {fg};
}}
notebook > header > tabs > tab:checked {{
  background-color: {surface_alt};
  background-image: none;
  color: {fg};
}}
notebook > stack {{
  background-color: {bg};
  background-image: none;
}}

/* ---- popovers / menus / dialogs ----
 * Paint only `contents` (rounded). Filling bare `popover` draws a sharp
 * rectangle behind the menu — the “bleed” past the border.
 * GtkDropDown / ListView popups also nest scrolledwindow+listview; those must
 * stay transparent or Adwaita’s grey plate shows through and past the radius.
 */
popover,
popover.background,
popover.menu {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  border: none;
  box-shadow: none;
}}
popover contents,
popover.menu contents,
.menu {{
  background: {surface};
  background-color: {surface};
  background-image: none;
  color: {fg};
  border: 1px solid {border};
  border-radius: 12px;
  box-shadow: none;
}}
popover > arrow {{
  background: {surface};
  background-color: {surface};
  background-image: none;
  border: none;
}}
popover contents > *,
popover contents scrolledwindow,
popover contents scrolledwindow > viewport,
popover contents viewport,
popover contents listview,
popover contents .view {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  border: none;
  box-shadow: none;
  color: {fg};
}}
popover contents modelbutton,
popover contents label,
popover contents listview row label {{
  color: {fg};
}}
/* Roomier menu rows (stock model buttons + IconMenu custom rows). */
popover.menu contents modelbutton,
popover contents modelbutton {{
  padding: 8px 12px;
  min-height: 32px;
  margin: 2px 4px;
  border-radius: 6px;
}}
popover.menu contents button.gtk-theme-menu-row,
popover contents button.gtk-theme-menu-row {{
  padding: 8px 12px;
  min-height: 32px;
  margin: 2px 4px;
  border-radius: 6px;
  color: {fg};
  background: transparent;
  background-color: transparent;
  background-image: none;
  box-shadow: none;
}}
/* Icon + label flush left (GtkButton otherwise centers its child). */
popover.menu contents button.gtk-theme-menu-row > box,
popover contents button.gtk-theme-menu-row > box {{
  margin: 0;
  min-width: 12em;
}}
popover.menu contents button.gtk-theme-menu-row label,
popover contents button.gtk-theme-menu-row label {{
  color: {fg};
  text-align: left;
}}
popover contents modelbutton:hover,
popover contents listview > row:hover,
popover.menu contents button.gtk-theme-menu-row:hover,
popover contents button.gtk-theme-menu-row:hover {{
  background-color: {hover};
  background-image: none;
}}
popover contents listview > row:selected {{
  background-color: alpha({accent}, 0.35);
  background-image: none;
  color: {fg};
}}

/* Closed GtkDropDown toggle (and font dialog button chrome) */
dropdown > button,
dropdown > togglebutton,
button.combo,
fontbutton,
fontdialogbutton {{
  background: {surface_alt};
  background-color: {surface_alt};
  background-image: none;
  color: {fg};
  border-color: {border};
}}
dropdown > button:hover,
dropdown > togglebutton:hover,
button.combo:hover {{
  background: {hover};
  background-color: {hover};
  background-image: none;
}}

/* Modal dialogs are often plain gtk::Window (not GtkDialog) — theme both */
dialog,
messagedialog,
window.gtk-dialog,
window.gtk-dialog.csd {{
  background: {bg};
  background-color: {bg};
  background-image: none;
  color: {fg};
}}
dialog headerbar,
messagedialog headerbar,
window.gtk-dialog headerbar,
window.gtk-dialog .titlebar {{
  background: {surface};
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-bottom: 1px solid {border};
}}
dialog label,
messagedialog label,
window.gtk-dialog label {{
  color: {fg};
}}
dialog entry,
messagedialog entry,
window.gtk-dialog entry {{
  background-color: {bg};
  color: {fg};
  caret-color: {fg};
  border-color: {border};
}}
dialog button,
messagedialog button,
window.gtk-dialog button {{
  background: {surface_alt};
  background-color: {surface_alt};
  background-image: none;
  color: {fg};
  border: 1px solid {border};
}}
dialog button label,
messagedialog button label,
window.gtk-dialog button label {{
  color: {fg};
}}
dialog button.suggested-action,
messagedialog button.suggested-action,
window.gtk-dialog button.suggested-action,
dialog button.suggested-action:hover,
messagedialog button.suggested-action:hover,
window.gtk-dialog button.suggested-action:hover {{
  background: {accent};
  background-color: {accent};
  background-image: none;
  color: {on_accent};
  border-color: {accent};
}}
dialog button.suggested-action label,
messagedialog button.suggested-action label,
window.gtk-dialog button.suggested-action label {{
  color: {on_accent};
}}
/* Flatten window-control chrome in dialog titlebars */
window.gtk-dialog headerbar windowcontrols button,
window.gtk-dialog .titlebar windowcontrols button {{
  background: transparent;
  background-color: transparent;
  background-image: none;
  border: none;
  box-shadow: none;
  color: {fg};
}}
/* FileChooserDialog puts Cancel / Open / Save in the headerbar — keep them
 * solid. suggested-action must win over the generic dialog button fg wash. */
window.gtk-dialog headerbar button.text-button,
window.gtk-dialog .titlebar button.text-button {{
  background: {surface_alt};
  background-color: {surface_alt};
  background-image: none;
  border: 1px solid {border};
  box-shadow: none;
  color: {fg};
}}
window.gtk-dialog headerbar button.text-button label,
window.gtk-dialog .titlebar button.text-button label {{
  color: {fg};
}}
window.gtk-dialog headerbar button.suggested-action,
window.gtk-dialog headerbar button.suggested-action:hover,
window.gtk-dialog headerbar button.suggested-action:active,
window.gtk-dialog .titlebar button.suggested-action,
window.gtk-dialog .titlebar button.suggested-action:hover,
window.gtk-dialog .titlebar button.suggested-action:active,
dialog button.suggested-action:active,
window.gtk-dialog button.suggested-action:active {{
  background: {accent};
  background-color: {accent};
  background-image: none;
  border: 1px solid {accent};
  box-shadow: none;
  color: {on_accent};
}}
window.gtk-dialog headerbar button.suggested-action label,
window.gtk-dialog headerbar button.suggested-action:hover label,
window.gtk-dialog .titlebar button.suggested-action label,
window.gtk-dialog .titlebar button.suggested-action:hover label {{
  color: {on_accent};
}}
/* Preferences notebook / plugins list — kill Adwaita grey .view wash */
window.gtk-dialog notebook,
window.gtk-dialog notebook > stack,
window.gtk-dialog scrolledwindow,
window.gtk-dialog viewport,
window.gtk-dialog listbox,
window.gtk-dialog listbox row,
window.gtk-dialog .view {{
  background: {bg};
  background-color: {bg};
  background-image: none;
  color: {fg};
}}
window.gtk-dialog listbox row:hover {{
  background-color: {hover};
  background-image: none;
}}
window.gtk-dialog checkbutton label,
window.gtk-dialog spinbutton,
window.gtk-dialog dropdown {{
  color: {fg};
}}

/* About dialog credits / license pages — Adwaita paints .view + .frame grey */
window.aboutdialog,
.aboutdialog {{
  background: {bg};
  background-color: {bg};
  background-image: none;
  color: {fg};
}}
window.aboutdialog headerbar,
window.aboutdialog .titlebar {{
  background: {surface};
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-bottom: 1px solid {border};
}}
window.aboutdialog label {{
  color: {fg};
}}
window.aboutdialog .view,
window.aboutdialog viewport,
window.aboutdialog viewport.view,
window.aboutdialog scrolledwindow,
window.aboutdialog scrolledwindow.frame,
window.aboutdialog textview,
window.aboutdialog textview text,
window.aboutdialog textview.view {{
  background: {bg};
  background-color: {bg};
  background-image: none;
  color: {fg};
  border-color: {border};
  box-shadow: none;
}}
window.aboutdialog stackswitcher button,
window.aboutdialog stackswitcher togglebutton {{
  background: {surface_alt};
  background-color: {surface_alt};
  background-image: none;
  color: {fg};
  border-color: {border};
}}
window.aboutdialog stackswitcher button:checked,
window.aboutdialog stackswitcher togglebutton:checked {{
  background: alpha({accent}, 0.35);
  background-color: alpha({accent}, 0.35);
  background-image: none;
  color: {fg};
}}

/* In-app file chooser (FileChooserDialog / FileChooserWidget) — portal
   * FileDialog cannot be themed; these widgets live in-process and follow us.
   */
filechooser,
filechooserwidget,
.window.gtk-dialog filechooser,
.window.gtk-dialog filechooserwidget {{
  background: {bg};
  background-color: {bg};
  color: {fg};
}}
filechooser *,
filechooserwidget * {{
  color: {fg};
}}
filechooser listview,
filechooser treeview,
filechooser columnview,
filechooserwidget listview,
filechooserwidget treeview,
filechooserwidget columnview,
filechooser .view,
filechooserwidget .view {{
  background-color: {bg};
  color: {fg};
}}
filechooser row:selected,
filechooserwidget row:selected,
filechooser listview > row:selected,
filechooserwidget listview > row:selected {{
  background-color: alpha({accent}, 0.35);
  color: {fg};
}}
filechooser entry,
filechooser searchentry,
filechooserwidget entry,
filechooserwidget searchentry {{
  background-color: {surface_alt};
  color: {fg};
  caret-color: {fg};
  border-color: {border};
}}
filechooser placessidebar,
filechooserwidget placessidebar,
placessidebar {{
  background-color: {surface};
  color: {fg};
}}
filechooser placessidebar row:selected,
placessidebar row:selected {{
  background-color: alpha({accent}, 0.35);
  color: {fg};
}}
pathbar button,
.pathbar button {{
  background: transparent;
  background-color: transparent;
  color: {fg};
  border: none;
  box-shadow: none;
}}
pathbar button:hover,
.pathbar button:hover {{
  background-color: {hover};
  color: {fg};
}}

/* ---- panes / separators / status ----
 * Adwaita-dark paned separators use background-image: image(#1b1b1b) and
 * .wide fills with #353535 — override both layers.
 */
separator {{
  background-color: {border};
  background-image: none;
}}
paned > separator,
paned > separator.wide,
paned > separator:hover,
paned > separator.wide:hover {{
  background: {border};
  background-color: {border};
  background-image: none;
  min-width: 4px;
  min-height: 4px;
}}
statusbar, .statusbar {{
  background: {surface};
  background-color: {surface};
  background-image: none;
  color: {fg};
  border-top: 1px solid {border};
}}

/* ---- gtk-files terminal sidebar (right) ---- */
.terminal-panel {{
  background-color: {bg};
  background-image: none;
  color: {fg};
  border-left: 1px solid {border};
}}
.terminal-panel-header {{
  background-color: {surface};
  background-image: none;
  color: {fg};
}}

/* ---- scrollbars ---- */
scrollbar, scrollbar trough {{
  background-color: {bg};
}}
scrollbar slider {{
  background-color: alpha({fg}, 0.35);
}}
scrollbar slider:hover {{
  background-color: alpha({fg}, 0.5);
}}
"#
    )
}

/// Tag a modal `gtk::Window` so dialog chrome (header, body, buttons) follows
/// the suite profile even when Adwaita paints transient windows lightly.
pub fn style_dialog(window: &impl IsA<gtk::Widget>) {
    window.add_css_class("gtk-dialog");
}

/// Present an in-app file chooser that follows the suite theme.
///
/// GTK’s portal-backed [`gtk::FileDialog`] is drawn by the desktop and ignores
/// app CSS — use this instead whenever the picker should match rusty chrome.
#[allow(deprecated)]
pub fn present_file_chooser(
    parent: Option<&impl IsA<gtk::Window>>,
    title: &str,
    action: gtk::FileChooserAction,
    accept_label: &str,
    filter: Option<&gtk::FileFilter>,
    initial_name: Option<&str>,
    callback: impl FnOnce(Option<gio::File>) + 'static,
) {
    present_file_chooser_at(parent, title, action, accept_label, filter, initial_name, None, callback);
}

/// Like [`present_file_chooser`], optionally starting in `current_folder`.
#[allow(deprecated)]
pub fn present_file_chooser_at(
    parent: Option<&impl IsA<gtk::Window>>,
    title: &str,
    action: gtk::FileChooserAction,
    accept_label: &str,
    filter: Option<&gtk::FileFilter>,
    initial_name: Option<&str>,
    current_folder: Option<&gio::File>,
    callback: impl FnOnce(Option<gio::File>) + 'static,
) {
    let dialog = gtk::FileChooserDialog::new(
        Some(title),
        parent,
        action,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            (accept_label, gtk::ResponseType::Accept),
        ],
    );
    style_dialog(&dialog);
    dialog.set_modal(true);
    dialog.set_default_width(720);
    dialog.set_default_height(480);

    if let Some(filter) = filter {
        dialog.add_filter(filter);
        dialog.set_filter(filter);
    }
    if let Some(name) = initial_name {
        dialog.set_current_name(name);
    }
    if let Some(folder) = current_folder {
        let _ = dialog.set_current_folder(Some(folder));
    }

    // Make Accept the default action button.
    if let Some(btn) = dialog.widget_for_response(gtk::ResponseType::Accept) {
        btn.add_css_class("suggested-action");
        dialog.set_default_widget(Some(&btn));
    }

    let callback = RefCell::new(Some(callback));
    dialog.connect_response(move |dlg, response| {
        // Nested Cancel/DeleteEvent from close() must not consume the callback.
        if matches!(response, gtk::ResponseType::DeleteEvent) {
            return;
        }
        // Take before close() — close() can re-enter with Cancel and would
        // otherwise call the callback with None, dropping the chosen file.
        let Some(cb) = callback.borrow_mut().take() else {
            return;
        };
        let file = match response {
            gtk::ResponseType::Accept | gtk::ResponseType::Ok => dlg.file().or_else(|| {
                let list = dlg.files();
                (0..list.n_items()).find_map(|i| {
                    list.item(i)
                        .and_then(|o| o.downcast::<gio::File>().ok())
                })
            }),
            _ => None,
        };
        dlg.close();
        cb(file);
    });
    dialog.present();
}

/// Load / update the shared chrome CSS provider for the default display.
pub fn apply_chrome(profile: &Profile) {
    apply_chrome_css(profile.is_dark(), &chrome_css(profile));
}

/// Live-preview chrome for in-progress (unsaved) [`ProfileData`].
///
/// Recolors every window in this process immediately without touching
/// `theme.toml` — the theme editor calls this on every tweak.
pub fn apply_chrome_data(data: &ProfileData) {
    let is_dark = relative_luminance(&data.background) < 0.45;
    apply_chrome_css(is_dark, &chrome_css_data(data));
}

fn apply_chrome_css(is_dark: bool, css: &str) {
    // Align GTK's light/dark preference with the suite profile so Adwaita does
    // not keep loading Default-dark assets under a light profile. Apps still
    // rely on chrome CSS overrides when the desktop theme is forced to *-dark.
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(is_dark);
    }

    // Adwaita symbolics for menus/toolbars even when the session uses Faenza/etc.
    ensure_adwaita_icons();

    if let Some(display) = gdk::Display::default() {
        CHROME_PROVIDER.with(|slot| {
            let mut slot = slot.borrow_mut();
            let provider = slot.get_or_insert_with(|| {
                let provider = gtk::CssProvider::new();
                // Sit above per-app APPLICATION CSS so toolbars/menus actually recolor.
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    // Above app CSS; just under USER so ~/.config/*/style.css still wins.
                    gtk::STYLE_PROVIDER_PRIORITY_USER.saturating_sub(10),
                );
                provider
            });
            provider.load_from_data(css);
        });
    }
}

/// Preferred GtkSourceView scheme ids for a profile (first available wins).
pub fn sourceview_scheme_candidates(profile_id: &str) -> &'static [&'static str] {
    match profile_id {
        "gruvbox-dark" => &["gruvbox-dark", "oblivion", "Adwaita-dark", "classic"],
        "gruvbox-light" => &["gruvbox-light", "solarized-light", "Adwaita", "classic"],
        "tokyo-night" | "tokyo-night-storm" => &["tokyo-night", "Adwaita-dark", "classic"],
        "dracula" => &["dracula", "Adwaita-dark", "classic"],
        "nord" => &["nord", "Adwaita-dark", "classic"],
        "catppuccin-mocha" | "catppuccin-frappe" => &["catppuccin-mocha", "Adwaita-dark", "classic"],
        "catppuccin-latte" => &["catppuccin-latte", "Adwaita", "classic"],
        "rose-pine" | "rose-pine-moon" => &["rose-pine", "Adwaita-dark", "classic"],
        "rose-pine-dawn" => &["rose-pine-dawn", "Adwaita", "classic"],
        "one-dark" => &["one-dark", "oblivion", "Adwaita-dark", "classic"],
        "monokai" => &["monokai", "oblivion", "Adwaita-dark", "classic"],
        "kanagawa" => &["kanagawa", "Adwaita-dark", "classic"],
        "everforest-dark" => &["everforest-dark", "Adwaita-dark", "classic"],
        "ayu-dark" | "ayu-mirage" => &["ayu-dark", "Adwaita-dark", "classic"],
        "night-owl" => &["night-owl", "Adwaita-dark", "classic"],
        "palenight" => &["palenight", "Adwaita-dark", "classic"],
        "material-darker" => &["material-darker", "Adwaita-dark", "classic"],
        "cobalt2" => &["cobalt", "Adwaita-dark", "classic"],
        "zenburn" => &["zenburn", "oblivion", "Adwaita-dark", "classic"],
        "tomorrow-night" => &["tomorrow-night", "Adwaita-dark", "classic"],
        "oceanic-next" => &["oceanic-next", "Adwaita-dark", "classic"],
        "github-dark" => &["github-dark", "Adwaita-dark", "classic"],
        "github-light" => &["github-light", "Adwaita", "classic"],
        "solarized-dark" => &["solarized-dark", "classic"],
        "solarized-light" => &["solarized-light", "classic"],
        "synthwave-84" => &["synthwave-84", "dracula", "Adwaita-dark", "classic"],
        id if id.contains("light") || id.contains("latte") || id.contains("dawn") => {
            &["Adwaita", "classic"]
        }
        _ => &["Adwaita-dark", "classic"],
    }
}

/// Resolve a SourceView scheme id using an availability predicate from the host app.
pub fn resolve_sourceview_scheme(
    profile_id: &str,
    is_available: impl Fn(&str) -> bool,
) -> &'static str {
    for id in sourceview_scheme_candidates(profile_id) {
        if is_available(id) {
            return id;
        }
    }
    "classic"
}

/// Window-scoped action name that launches [`gtk-theme-editor`].
///
/// Menu models built by [`append_profile_menu`] activate `win.open-theme-editor`;
/// call [`install_open_theme_editor_action`] once per window so the item works.
pub const OPEN_THEME_EDITOR_ACTION: &str = "open-theme-editor";

/// Full action path used in Gio.Menu models (`win.` + [`OPEN_THEME_EDITOR_ACTION`]).
pub const OPEN_THEME_EDITOR_MENU_ACTION: &str = "win.open-theme-editor";

/// Sentinel id appended by [`build_profile_dropdown`] for the "Custom…" row.
pub const CUSTOM_EDITOR_SENTINEL: &str = "__custom_editor__";

/// Resolve the `gtk-theme-editor` binary (release → debug → PATH).
pub fn theme_editor_path() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("GTK_THEME_EDITOR") {
        let p = PathBuf::from(custom);
        if p.is_file() {
            return Some(p);
        }
    }

    // Suite layout: GTK-Apps/gtk-<app>/target/{release,debug}/<bin>
    // → sibling gtk-theme-editor/target/...
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors().take(5) {
            let base = ancestor.join("gtk-theme-editor").join("target");
            for name in ["release/gtk-theme-editor", "debug/gtk-theme-editor"] {
                let p = base.join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }

    // Relative to this crate when building from the monorepo.
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(suite) = crate_dir.parent() {
        let base = suite.join("gtk-theme-editor").join("target");
        for name in ["release/gtk-theme-editor", "debug/gtk-theme-editor"] {
            let p = base.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    // Fall back to PATH (e.g. if installed system-wide).
    std::env::var_os("PATH").and_then(|paths| {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join("gtk-theme-editor");
            if p.is_file() {
                return Some(p);
            }
        }
        None
    })
}

/// Spawn the suite theme editor so the user can create/edit a custom profile.
pub fn launch_theme_editor() {
    let Some(bin) = theme_editor_path() else {
        eprintln!("gtk-theme: gtk-theme-editor binary not found");
        return;
    };
    if let Err(e) = std::process::Command::new(&bin).spawn() {
        eprintln!("gtk-theme: failed to launch {}: {e}", bin.display());
    }
}

/// Install `win.open-theme-editor` on a window (idempotent).
pub fn install_open_theme_editor_action(map: &impl IsA<gio::ActionMap>) {
    let map = map.as_ref();
    if map.lookup_action(OPEN_THEME_EDITOR_ACTION).is_some() {
        return;
    }
    let act = gio::SimpleAction::new(OPEN_THEME_EDITOR_ACTION, None);
    act.connect_activate(|_, _| launch_theme_editor());
    map.add_action(&act);
}

/// Build a radio-style Profile submenu (matches the gtk-term Profile UI).
///
/// `action_name` should be a stateful string action, e.g. `"win.theme"`.
/// Radio rows stay text-only so GTK can show the check indicator; the
/// submenu parent uses a themed color icon when the menu renderer shows it.
///
/// Always ends with a **Custom…** item that activates
/// [`OPEN_THEME_EDITOR_MENU_ACTION`] — install it with
/// [`install_open_theme_editor_action`].
pub fn append_profile_menu(parent: &gio::Menu, action_name: &str) {
    let profiles_menu = gio::Menu::new();

    let builtin_section = gio::Menu::new();
    for profile in builtin_profiles() {
        let item = gio::MenuItem::new(Some(profile.name), None);
        item.set_action_and_target_value(Some(action_name), Some(&profile.id.to_variant()));
        builtin_section.append_item(&item);
    }
    profiles_menu.append_section(None, &builtin_section);

    // User-saved custom profiles (from the theme editor) get their own section.
    let customs: Vec<&'static Profile> = all_profiles()
        .into_iter()
        .filter(|p| !PROFILES.iter().any(|b| b.id == p.id))
        .collect();
    if !customs.is_empty() {
        let custom_section = gio::Menu::new();
        for profile in customs {
            let item = gio::MenuItem::new(Some(profile.name), None);
            item.set_action_and_target_value(Some(action_name), Some(&profile.id.to_variant()));
            custom_section.append_item(&item);
        }
        profiles_menu.append_section(Some("Saved"), &custom_section);
    }

    // Opens gtk-theme-editor to create / tweak a custom profile.
    let editor_section = gio::Menu::new();
    let custom_item =
        gio::MenuItem::new(Some("Custom…"), Some(OPEN_THEME_EDITOR_MENU_ACTION));
    editor_section.append_item(&custom_item);
    profiles_menu.append_section(None, &editor_section);

    let item = gio::MenuItem::new(Some("Profile"), None);
    item.set_submenu(Some(&profiles_menu));
    parent.append_item(&item);
}

/// Drop-down of profile display names, plus a trailing **Custom…** row that
/// launches the theme editor (selection snaps back to the previous profile).
///
/// Returns `(dropdown, profile ids in order)` — ids do **not** include the
/// Custom sentinel; use [`profile_id_at`] when reading the selection.
pub fn build_profile_dropdown(current_id: &str) -> (gtk::DropDown, Vec<&'static str>) {
    let profiles = all_profiles();
    let ids: Vec<&'static str> = profiles.iter().map(|p| p.id).collect();
    let mut labels: Vec<&str> = profiles.iter().map(|p| p.name).collect();
    labels.push("Custom…");
    let model = gtk::StringList::new(&labels);
    let drop = gtk::DropDown::new(Some(model), None::<gtk::Expression>);
    let idx = ids.iter().position(|id| *id == current_id).unwrap_or(0) as u32;
    drop.set_selected(idx);

    // Selecting "Custom…" opens the editor and restores the prior profile row.
    let last = std::cell::Cell::new(idx);
    let custom_row = ids.len() as u32;
    drop.connect_selected_notify(move |dd| {
        let sel = dd.selected();
        if sel == custom_row {
            dd.set_selected(last.get());
            launch_theme_editor();
            return;
        }
        last.set(sel);
    });

    (drop, ids)
}

/// Profile id for a dropdown selection, or `None` if out of range / Custom row.
pub fn profile_id_at(ids: &[&'static str], selected: u32) -> Option<&'static str> {
    ids.get(selected as usize).copied()
}

/// Persist theme id, apply chrome CSS, then invoke `on_profile`.
///
/// Also notifies [`watch_theme`] listeners so other suite apps (and VTE panels
/// in this process) stay in sync.
pub fn select_theme(id: &str, on_profile: impl FnOnce(&Profile)) {
    let Some(profile) = profile_by_id(id) else {
        return;
    };
    save_theme_id(id);
    apply_chrome(profile);
    on_profile(profile);
    broadcast_theme(profile, false);
}

/// Watch the shared theme file and keep a stateful window action in sync.
///
/// Chrome CSS is reapplied by [`watch_theme`]. Use this in apps that only need
/// the View/Theme menu radio to follow suite-wide profile changes (e.g. calc,
/// image). Apps with extra surfaces (VTE, SourceView) should call [`watch_theme`]
/// themselves and update those surfaces in the callback.
pub fn watch_theme_sync_action(window: &gtk::ApplicationWindow, action_name: &str) {
    let window = window.clone();
    let action_name = action_name.to_string();
    watch_theme(move |profile| {
        if let Some(action) = window.lookup_action(&action_name) {
            action
                .downcast_ref::<gio::SimpleAction>()
                .map(|a| a.set_state(&profile.id.to_variant()));
        }
    });
}

/// Watch `~/.config/gtk-apps/theme.toml` for profile changes from other apps.
///
/// Chrome CSS is reapplied automatically. Register once per process interest
/// (window / VTE / action state); callbacks run for local [`select_theme`] too.
pub fn watch_theme(on_change: impl Fn(&Profile) + 'static) {
    THEME_WATCHERS.with(|state| {
        let mut state = state.borrow_mut();
        if state.last_id.is_empty() {
            state.last_id = load_theme_id();
        }
        state.callbacks.push(Rc::new(on_change));
        if state.monitor.is_some() {
            return;
        }
        // Ensure the file exists so the monitor has a stable target.
        if !theme_path().exists() {
            save_theme_id(&load_theme_id());
        }
        let file = gio::File::for_path(theme_path());
        let Ok(monitor) =
            file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        else {
            return;
        };
        monitor.connect_changed(move |_mon, _file, _other, event| {
            use gio::FileMonitorEvent::*;
            if !matches!(event, ChangesDoneHint | Changed | Created) {
                return;
            }
            let id = load_theme_id();
            let should = THEME_WATCHERS.with(|state| state.borrow().last_id != id);
            if !should {
                return;
            }
            let Some(profile) = profile_by_id(&id) else {
                return;
            };
            apply_chrome(profile);
            broadcast_theme(profile, true);
        });
        state.monitor = Some(monitor);
    });
}

/// `from_file_monitor`: skip when id already matches (local [`select_theme`] set it).
fn broadcast_theme(profile: &Profile, from_file_monitor: bool) {
    let callbacks = THEME_WATCHERS.with(|state| {
        let mut state = state.borrow_mut();
        if from_file_monitor && state.last_id == profile.id {
            return Vec::new();
        }
        state.last_id = profile.id.to_string();
        state.callbacks.clone()
    });
    for cb in callbacks {
        cb(profile);
    }
}

// ---------------------------------------------------------------------------
// internals
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ThemeFile {
    profile: String,
}

fn theme_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-apps")
}

fn theme_path() -> PathBuf {
    theme_dir().join("theme.toml")
}

fn parse_color(value: &str, fallback: &str) -> gdk::RGBA {
    value
        .parse::<gdk::RGBA>()
        .or_else(|_| fallback.parse::<gdk::RGBA>())
        .unwrap_or(gdk::RGBA::BLACK)
}

fn parse_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some((r, g, b))
}

fn relative_luminance(hex: &str) -> f32 {
    let (r, g, b) = parse_rgb(hex).unwrap_or((0, 0, 0));
    // sRGB relative luminance (approximate, good enough for dark/light splits).
    (0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)) / 255.0
}

fn mix_hex(a: &str, b: &str, t: f32) -> String {
    let (ar, ag, ab) = parse_rgb(a).unwrap_or((0, 0, 0));
    let (br, bg, bb) = parse_rgb(b).unwrap_or((255, 255, 255));
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 {
        ((f32::from(x) * (1.0 - t)) + (f32::from(y) * t)).round() as u8
    };
    format!("#{:02x}{:02x}{:02x}", mix(ar, br), mix(ag, bg), mix(ab, bb))
}

thread_local! {
    static CHROME_PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    static THEME_WATCHERS: RefCell<ThemeWatchState> = const { RefCell::new(ThemeWatchState::new()) };
    static CUSTOM_REGISTRY: RefCell<HashMap<String, (ProfileData, &'static Profile)>> =
        RefCell::new(HashMap::new());
}

/// Leak owned profile data into a `&'static Profile` so it can be returned
/// alongside the built-in static profiles. Only reached when a custom profile
/// is added or changed on disk, so the leak is bounded by user edits.
fn leak_profile(data: &ProfileData) -> &'static Profile {
    fn leak_str(s: &str) -> &'static str {
        Box::leak(s.to_string().into_boxed_str())
    }
    let palette = data.normalized_palette();
    let mut arr: [&'static str; 16] = ["#000000"; 16];
    for (slot, hex) in arr.iter_mut().zip(palette.iter()) {
        *slot = leak_str(hex);
    }
    let profile = Profile {
        id: leak_str(&data.id),
        name: leak_str(&data.name),
        foreground: leak_str(&data.foreground),
        background: leak_str(&data.background),
        palette: arr,
    };
    Box::leak(Box::new(profile))
}

/// Sync the in-process custom-profile registry with the on-disk store,
/// re-leaking only entries whose data changed and dropping deleted ones.
fn refresh_custom_registry() {
    let list = load_custom_profiles();
    CUSTOM_REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        let live: std::collections::HashSet<String> =
            list.iter().map(|d| d.id.clone()).collect();
        reg.retain(|id, _| live.contains(id));
        for data in list {
            let unchanged = reg
                .get(&data.id)
                .map(|(existing, _)| existing == &data)
                .unwrap_or(false);
            if !unchanged {
                let leaked = leak_profile(&data);
                reg.insert(data.id.clone(), (data, leaked));
            }
        }
    });
}

struct ThemeWatchState {
    monitor: Option<gio::FileMonitor>,
    last_id: String,
    callbacks: Vec<Rc<dyn Fn(&Profile)>>,
}

impl ThemeWatchState {
    const fn new() -> Self {
        Self {
            monitor: None,
            last_id: String::new(),
            callbacks: Vec::new(),
        }
    }
}

/// Built-in suite profiles. Default (`gruvbox-dark`) is first.
///
/// Palettes are adapted from the published color schemes of each theme
/// (Gruvbox, Tokyo Night, Catppuccin, Dracula, Nord, Rosé Pine, One Dark,
/// Monokai, Kanagawa, Everforest, Ayu, Night Owl, Solarized, etc.) — not
/// invented arbitrarily. Exact hex values can differ slightly from upstream
/// repos; chrome CSS uses fg/bg for contrast rather than ANSI slot 0.
static PROFILES: [Profile; 30] = [
    // --- default ---
    Profile {
        id: "gruvbox-dark",
        name: "Gruvbox Dark",
        foreground: "#ebdbb2",
        background: "#282828",
        palette: [
            "#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a",
            "#a89984", "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b",
            "#8ec07c", "#ebdbb2",
        ],
    },
    Profile {
        id: "gruvbox-light",
        name: "Gruvbox Light",
        foreground: "#3c3836",
        background: "#fbf1c7",
        palette: [
            "#fbf1c7", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a",
            "#7c6f64", "#928374", "#9d0006", "#79740e", "#b57614", "#076678", "#8f3f71",
            "#427b58", "#3c3836",
        ],
    },
    // --- popular dark ---
    Profile {
        id: "tokyo-night",
        name: "Tokyo Night",
        foreground: "#c0caf5",
        background: "#1a1b26",
        palette: [
            "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff",
            "#a9b1d6", "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7",
            "#7dcfff", "#c0caf5",
        ],
    },
    Profile {
        id: "tokyo-night-storm",
        name: "Tokyo Night Storm",
        foreground: "#c0caf5",
        background: "#24283b",
        palette: [
            "#1d202f", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff",
            "#a9b1d6", "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7",
            "#7dcfff", "#c0caf5",
        ],
    },
    Profile {
        id: "dracula",
        name: "Dracula",
        foreground: "#f8f8f2",
        background: "#282a36",
        palette: [
            "#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd",
            "#f8f8f2", "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df",
            "#a4ffff", "#ffffff",
        ],
    },
    Profile {
        id: "nord",
        name: "Nord",
        foreground: "#d8dee9",
        background: "#2e3440",
        palette: [
            "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0",
            "#e5e9f0", "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead",
            "#8fbcbb", "#eceff4",
        ],
    },
    Profile {
        id: "catppuccin-mocha",
        name: "Catppuccin Mocha",
        foreground: "#cdd6f4",
        background: "#1e1e2e",
        palette: [
            "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5",
            "#bac2de", "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7",
            "#94e2d5", "#cdd6f4",
        ],
    },
    Profile {
        id: "catppuccin-frappe",
        name: "Catppuccin Frappé",
        foreground: "#c6d0f5",
        background: "#303446",
        palette: [
            "#51576d", "#e78284", "#a6d189", "#e5c890", "#8caaee", "#f4b8e4", "#81c8be",
            "#b5bfe2", "#626880", "#e78284", "#a6d189", "#e5c890", "#8caaee", "#f4b8e4",
            "#81c8be", "#c6d0f5",
        ],
    },
    Profile {
        id: "rose-pine",
        name: "Rosé Pine",
        foreground: "#e0def4",
        background: "#191724",
        palette: [
            "#26233a", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba",
            "#e0def4", "#6e6a86", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7",
            "#ebbcba", "#e0def4",
        ],
    },
    Profile {
        id: "rose-pine-moon",
        name: "Rosé Pine Moon",
        foreground: "#e0def4",
        background: "#232136",
        palette: [
            "#2a273f", "#eb6f92", "#3e8fb0", "#f6c177", "#9ccfd8", "#c4a7e7", "#ea9a97",
            "#e0def4", "#6e6a86", "#eb6f92", "#3e8fb0", "#f6c177", "#9ccfd8", "#c4a7e7",
            "#ea9a97", "#e0def4",
        ],
    },
    Profile {
        id: "one-dark",
        name: "One Dark",
        foreground: "#abb2bf",
        background: "#282c34",
        palette: [
            "#282c34", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2",
            "#abb2bf", "#5c6370", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd",
            "#56b6c2", "#ffffff",
        ],
    },
    Profile {
        id: "monokai",
        name: "Monokai",
        foreground: "#f8f8f2",
        background: "#272822",
        palette: [
            "#272822", "#f92672", "#a6e22e", "#f4bf75", "#66d9ef", "#ae81ff", "#a1efe4",
            "#f8f8f2", "#75715e", "#f92672", "#a6e22e", "#f4bf75", "#66d9ef", "#ae81ff",
            "#a1efe4", "#f9f8f5",
        ],
    },
    Profile {
        id: "kanagawa",
        name: "Kanagawa",
        foreground: "#dcd7ba",
        background: "#1f1f28",
        palette: [
            "#090618", "#c34043", "#76946a", "#c0a36e", "#7e9cd8", "#957fb8", "#6a9589",
            "#c8c093", "#727169", "#e82424", "#98bb6c", "#e6c384", "#7fb4ca", "#938aa9",
            "#7aa89f", "#dcd7ba",
        ],
    },
    Profile {
        id: "everforest-dark",
        name: "Everforest Dark",
        foreground: "#d3c6aa",
        background: "#2d353b",
        palette: [
            "#475258", "#e67e80", "#a7c080", "#dbbc7f", "#7fbbb3", "#d699b6", "#83c092",
            "#d3c6aa", "#7a8478", "#e67e80", "#a7c080", "#dbbc7f", "#7fbbb3", "#d699b6",
            "#83c092", "#d3c6aa",
        ],
    },
    Profile {
        id: "ayu-dark",
        name: "Ayu Dark",
        foreground: "#b3b1ad",
        background: "#0a0e14",
        palette: [
            "#01060e", "#ea6c73", "#91b362", "#f9af4f", "#53bdfa", "#fae994", "#90e1c6",
            "#c7c7c7", "#686868", "#f07178", "#c2d94c", "#ffb454", "#59c2ff", "#ffee99",
            "#95e6cb", "#ffffff",
        ],
    },
    Profile {
        id: "ayu-mirage",
        name: "Ayu Mirage",
        foreground: "#cccac2",
        background: "#1f2430",
        palette: [
            "#191e2a", "#ed8274", "#87d96c", "#fad07b", "#6dcbfa", "#fbb0ce", "#90e1c6",
            "#c7c7c7", "#686868", "#f28779", "#d5ff80", "#ffcc66", "#73d0ff", "#f287bc",
            "#95e6cb", "#ffffff",
        ],
    },
    Profile {
        id: "night-owl",
        name: "Night Owl",
        foreground: "#d6deeb",
        background: "#011627",
        palette: [
            "#011627", "#ef5350", "#22da6e", "#addb67", "#82aaff", "#c792ea", "#21c7a8",
            "#ffffff", "#575656", "#ef5350", "#22da6e", "#ffeb95", "#82aaff", "#c792ea",
            "#7fdbca", "#ffffff",
        ],
    },
    Profile {
        id: "palenight",
        name: "Palenight",
        foreground: "#a6accd",
        background: "#292d3e",
        palette: [
            "#292d3e", "#f07178", "#c3e88d", "#ffcb6b", "#82aaff", "#c792ea", "#89ddff",
            "#d0d0d0", "#434758", "#ff8b92", "#ddffa7", "#ffe585", "#9cc4ff", "#e1acff",
            "#a3f7ff", "#ffffff",
        ],
    },
    Profile {
        id: "material-darker",
        name: "Material Darker",
        foreground: "#eeffff",
        background: "#212121",
        palette: [
            "#000000", "#ff5370", "#c3e88d", "#ffcb6b", "#82aaff", "#c792ea", "#89ddff",
            "#ffffff", "#545454", "#ff5370", "#c3e88d", "#ffcb6b", "#82aaff", "#c792ea",
            "#89ddff", "#ffffff",
        ],
    },
    Profile {
        id: "cobalt2",
        name: "Cobalt2",
        foreground: "#ffffff",
        background: "#193549",
        palette: [
            "#000000", "#ff0000", "#38de21", "#ffe50a", "#1460d2", "#ff005d", "#00bbbb",
            "#bbbbbb", "#555555", "#f40e17", "#3bd01d", "#edc809", "#5555ff", "#ff55ff",
            "#6ae3fa", "#ffffff",
        ],
    },
    Profile {
        id: "zenburn",
        name: "Zenburn",
        foreground: "#dcdccc",
        background: "#3f3f3f",
        palette: [
            "#4f4f4f", "#705050", "#60b48a", "#f0dfaf", "#506070", "#dc8cc3", "#8cd0d3",
            "#dcdccc", "#709080", "#dca3a3", "#c3bf9f", "#e0cf9f", "#94bff3", "#ec93d3",
            "#93e0e3", "#ffffff",
        ],
    },
    Profile {
        id: "tomorrow-night",
        name: "Tomorrow Night",
        foreground: "#c5c8c6",
        background: "#1d1f21",
        palette: [
            "#1d1f21", "#cc6666", "#b5bd68", "#f0c674", "#81a2be", "#b294bb", "#8abeb7",
            "#c5c8c6", "#969896", "#cc6666", "#b5bd68", "#f0c674", "#81a2be", "#b294bb",
            "#8abeb7", "#ffffff",
        ],
    },
    Profile {
        id: "oceanic-next",
        name: "Oceanic Next",
        foreground: "#d8dee9",
        background: "#1b2b34",
        palette: [
            "#29414f", "#ec5f67", "#99c794", "#fac863", "#6699cc", "#c594c5", "#5fb3b3",
            "#d8dee9", "#405860", "#ec5f67", "#99c794", "#fac863", "#6699cc", "#c594c5",
            "#5fb3b3", "#ffffff",
        ],
    },
    Profile {
        id: "github-dark",
        name: "GitHub Dark",
        foreground: "#e6edf3",
        background: "#0d1117",
        palette: [
            "#484f58", "#ff7b72", "#3fb950", "#d29922", "#58a6ff", "#bc8cff", "#39c5cf",
            "#b1bac4", "#6e7681", "#ffa198", "#56d364", "#e3b341", "#79c0ff", "#d2a8ff",
            "#56d4dd", "#ffffff",
        ],
    },
    Profile {
        id: "synthwave-84",
        name: "Synthwave '84",
        foreground: "#f92aad",
        background: "#2b213a",
        palette: [
            "#241b2f", "#f97e72", "#72f1b8", "#fede5d", "#36f9f6", "#ff7edb", "#f97e72",
            "#ffffff", "#848bbd", "#f88414", "#72f1b8", "#fff951", "#36f9f6", "#ff7edb",
            "#f97e72", "#ffffff",
        ],
    },
    // --- light ---
    Profile {
        id: "solarized-dark",
        name: "Solarized Dark",
        foreground: "#839496",
        background: "#002b36",
        palette: [
            "#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198",
            "#eee8d5", "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4",
            "#93a1a1", "#fdf6e3",
        ],
    },
    Profile {
        id: "solarized-light",
        name: "Solarized Light",
        foreground: "#657b83",
        background: "#fdf6e3",
        palette: [
            "#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198",
            "#eee8d5", "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4",
            "#93a1a1", "#fdf6e3",
        ],
    },
    Profile {
        id: "catppuccin-latte",
        name: "Catppuccin Latte",
        foreground: "#4c4f69",
        background: "#eff1f5",
        palette: [
            "#5c5f77", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299",
            "#acb0be", "#6c6f85", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb",
            "#179299", "#4c4f69",
        ],
    },
    Profile {
        id: "rose-pine-dawn",
        name: "Rosé Pine Dawn",
        foreground: "#575279",
        background: "#faf4ed",
        palette: [
            "#f2e9e1", "#b4637a", "#286983", "#ea9d34", "#56949f", "#907aa9", "#d7827e",
            "#575279", "#9893a5", "#b4637a", "#286983", "#ea9d34", "#56949f", "#907aa9",
            "#d7827e", "#575279",
        ],
    },
    Profile {
        id: "github-light",
        name: "GitHub Light",
        foreground: "#1f2328",
        background: "#ffffff",
        palette: [
            "#24292f", "#cf222e", "#116329", "#4d2d00", "#0969da", "#8250df", "#1b7c83",
            "#6e7781", "#57606a", "#a40e26", "#1a7f37", "#633c01", "#218bff", "#a475f9",
            "#3192aa", "#1f2328",
        ],
    },
];
