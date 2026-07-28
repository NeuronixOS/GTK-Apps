use std::rc::Rc;

use regex::Regex;
use sourceview5::prelude::*;

use crate::plugin::activatable::{Plugin, PluginInfo, ViewActivatable};
use crate::plugins::{info, make_factory};

struct ModelinesPlugin {
    info: PluginInfo,
}

impl Plugin for ModelinesPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_view(&mut self) -> Option<&mut dyn ViewActivatable> {
        Some(self)
    }
}

impl ViewActivatable for ModelinesPlugin {
    fn activate(&mut self, view: &sourceview5::View) {
        apply_modelines(view);
        let view2 = view.clone();
        let binding = view.buffer();
        if let Some(buf) = binding.downcast_ref::<sourceview5::Buffer>() {
            buf.connect_modified_changed(move |b| {
                if !b.is_modified() {
                    apply_modelines(&view2);
                }
            });
        }
    }

    fn deactivate(&mut self) {}
}

fn apply_modelines(view: &sourceview5::View) {
    let binding = view.buffer();
    let Some(buffer) = binding.downcast_ref::<sourceview5::Buffer>() else {
        return;
    };
    let start = buffer.start_iter();
    let mut end = start;
    end.forward_lines(5);
    let head = buffer.text(&start, &end, false).to_string();
    let mut tail_start = buffer.end_iter();
    tail_start.backward_lines(5);
    let tail = buffer
        .text(&tail_start, &buffer.end_iter(), false)
        .to_string();
    let text = format!("{head}\n{tail}");

    if let Some(re) = Regex::new(r"(?i)(?:vi|vim|ex):\s*(.*)").ok() {
        if let Some(caps) = re.captures(&text) {
            parse_vim_options(view, caps.get(1).map(|m| m.as_str()).unwrap_or(""));
        }
    }
    if let Some(re) = Regex::new(r"(?i)-\*-\s*(.*?)\s*-\*-").ok() {
        if let Some(caps) = re.captures(&text) {
            parse_emacs_options(view, buffer, caps.get(1).map(|m| m.as_str()).unwrap_or(""));
        }
    }
    // kate: ...
    if let Some(re) = Regex::new(r"(?i)kate:\s*(.*)").ok() {
        if let Some(caps) = re.captures(&text) {
            parse_kate_options(view, caps.get(1).map(|m| m.as_str()).unwrap_or(""));
        }
    }
}

fn parse_vim_options(view: &sourceview5::View, opts: &str) {
    for part in opts.split(|c| c == ':' || c == ' ') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(rest) = part.strip_prefix("ts=") {
            if let Ok(n) = rest.parse::<u32>() {
                view.set_tab_width(n);
            }
        } else if let Some(rest) = part.strip_prefix("sw=") {
            if let Ok(n) = rest.parse::<u32>() {
                view.set_indent_width(n as i32);
            }
        } else if part == "et" || part == "expandtab" {
            view.set_insert_spaces_instead_of_tabs(true);
        } else if part == "noet" || part == "noexpandtab" {
            view.set_insert_spaces_instead_of_tabs(false);
        } else if part == "nu" || part == "number" {
            view.set_show_line_numbers(true);
        } else if part == "nonu" || part == "nonumber" {
            view.set_show_line_numbers(false);
        }
    }
}

fn parse_emacs_options(
    view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    opts: &str,
) {
    for part in opts.split(',') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once(':') {
            let k = k.trim().to_lowercase();
            let v = v.trim();
            match k.as_str() {
                "mode" => {
                    let lm = sourceview5::LanguageManager::default();
                    if let Some(lang) = lm.language(v) {
                        buffer.set_language(Some(&lang));
                    }
                }
                "tab-width" => {
                    if let Ok(n) = v.parse::<u32>() {
                        view.set_tab_width(n);
                    }
                }
                "indent-tabs-mode" => {
                    view.set_insert_spaces_instead_of_tabs(v != "t" && v != "true");
                }
                _ => {}
            }
        } else {
            // bare mode name
            let lm = sourceview5::LanguageManager::default();
            if let Some(lang) = lm.language(part) {
                buffer.set_language(Some(&lang));
            }
        }
    }
}

fn parse_kate_options(view: &sourceview5::View, opts: &str) {
    for part in opts.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once(' ') {
            match k.trim() {
                "tab-width" | "indent-width" => {
                    if let Ok(n) = v.trim().parse::<u32>() {
                        view.set_tab_width(n);
                    }
                }
                "space-indent" => {
                    view.set_insert_spaces_instead_of_tabs(v.trim() == "on" || v.trim() == "true");
                }
                _ => {}
            }
        }
    }
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info(
        "modelines",
        "Modelines",
        "Applies Emacs/Vim/Kate modelines found in documents.",
    );
    make_factory(i.clone(), move || {
        Box::new(ModelinesPlugin { info: i.clone() })
    })
}

#[allow(dead_code)]
fn _rc_use() -> Rc<()> {
    Rc::new(())
}
