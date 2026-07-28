//! Spell checking via `aspell list` / `enchant-2 -l` (one-shot, non-interactive).

use std::io::Write;
use std::process::{Command, Stdio};

use gtk4 as gtk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use sourceview5::prelude::*;

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

struct SpellPlugin {
    info: PluginInfo,
    actions: Vec<gio::SimpleAction>,
    language: String,
}

impl Plugin for SpellPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for SpellPlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        let win = ctx.window.clone();
        let lang = self.language.clone();
        let check = gio::SimpleAction::new("spell-check", None);
        {
            let win2 = win.clone();
            let lang2 = lang.clone();
            check.connect_activate(move |_, _| run_spell_check(&win2, &lang2));
        }
        win.add_action(&check);
        ctx.menu_icons.borrow_mut().append(
            &ctx.tools_menu,
            "Check Spelling…",
            "win.spell-check",
            "tools-check-spelling-symbolic",
        );
        self.actions.push(check);

        let lang_action = gio::SimpleAction::new("spell-language", None);
        {
            let win2 = win.clone();
            lang_action.connect_activate(move |_, _| choose_language(&win2));
        }
        win.add_action(&lang_action);
        ctx.menu_icons.borrow_mut().append(
            &ctx.tools_menu,
            "Set Language…",
            "win.spell-language",
            "preferences-desktop-locale-symbolic",
        );
        self.actions.push(lang_action);

        ctx.status_label
            .set_text(&format!("Spell: {}", self.language));
    }

    fn deactivate(&mut self) {
        self.actions.clear();
    }
}

fn run_spell_check(win: &gtk::ApplicationWindow, language: &str) {
    let Some(tab) = crate::window::current_tab_from_window(win) else {
        return;
    };
    let text = tab.document.text();
    let result = check_document(&text, language);
    present_spell_results(win, &tab.document.buffer, result);
}

enum SpellResult {
    Ok(Vec<String>),
    Unavailable,
    Failed(String),
}

fn check_document(text: &str, language: &str) -> SpellResult {
    // Prefer aspell: one process, stdin list mode (never interactive `-a`).
    // The previous implementation called `enchant-2 -a` with inherited stdin,
    // which blocked the UI forever waiting for input.
    if let Some(words) = try_aspell(text, language) {
        return SpellResult::Ok(words);
    }
    if let Some(words) = try_enchant(text, language) {
        return SpellResult::Ok(words);
    }
    if bin_on_path("aspell") || bin_on_path("enchant-2") || bin_on_path("enchant") {
        SpellResult::Failed(format!(
            "Could not run the spell checker for language “{language}”. \
             Install a matching dictionary (e.g. aspell-en)."
        ))
    } else {
        SpellResult::Unavailable
    }
}

fn try_aspell(text: &str, language: &str) -> Option<Vec<String>> {
    let mut child = Command::new("aspell")
        .args(["list", &format!("--lang={language}")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() && out.stdout.is_empty() {
        return None;
    }
    Some(unique_words(&out.stdout))
}

fn try_enchant(text: &str, language: &str) -> Option<Vec<String>> {
    let bin = if bin_on_path("enchant-2") {
        "enchant-2"
    } else if bin_on_path("enchant") {
        "enchant"
    } else {
        return None;
    };
    // `-l` lists misspellings from stdin (non-interactive). Never use `-a` here.
    let mut child = Command::new(bin)
        .args(["-l", "-d", language])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() && out.stdout.is_empty() {
        return None;
    }
    Some(unique_words(&out.stdout))
}

fn unique_words(stdout: &[u8]) -> Vec<String> {
    let mut words: Vec<String> = String::from_utf8_lossy(stdout)
        .lines()
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect();
    words.sort();
    words.dedup();
    words
}

fn bin_on_path(bin: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(bin).is_file())
}

fn present_spell_results(
    win: &gtk::ApplicationWindow,
    buffer: &sourceview5::Buffer,
    result: SpellResult,
) {
    let detail = match &result {
        SpellResult::Ok(words) if words.is_empty() => "No misspellings found.".to_string(),
        SpellResult::Ok(words) => format!(
            "Possible misspellings ({}):\n{}",
            words.len(),
            words
                .iter()
                .take(40)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ),
        SpellResult::Unavailable => {
            "No spell checker found. Install aspell (or enchant-2) and a dictionary \
             such as aspell-en."
                .to_string()
        }
        SpellResult::Failed(msg) => msg.clone(),
    };

    if let SpellResult::Ok(words) = &result {
        if let Some(first) = words.first() {
            let settings = sourceview5::SearchSettings::new();
            settings.set_search_text(Some(first));
            settings.set_case_sensitive(false);
            settings.set_regex_enabled(false);
            let ctx = sourceview5::SearchContext::new(buffer, Some(&settings));
            ctx.set_highlight(true);
            // Keep the context alive briefly so the highlight paints.
            glib::timeout_add_local_once(std::time::Duration::from_secs(8), move || {
                drop(ctx);
            });
        }
    }

    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message("Spell Check")
        .detail(&detail)
        .buttons(["Close"])
        .build();
    dialog.show(Some(win));
}

fn choose_language(win: &gtk::ApplicationWindow) {
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message("Spell Language")
        .detail(
            "Default dictionary language is English (en). Install aspell/enchant \
             dictionaries for more languages.",
        )
        .buttons(["OK"])
        .build();
    dialog.show(Some(win));
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info(
        "spell",
        "Spell Checker",
        "Checks the spelling of the current document using aspell/enchant.",
    );
    make_factory(i.clone(), move || {
        Box::new(SpellPlugin {
            info: i.clone(),
            actions: Vec::new(),
            language: "en".into(),
        })
    })
}
