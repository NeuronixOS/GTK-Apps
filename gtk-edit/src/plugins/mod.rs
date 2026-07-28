mod changecase;
mod docinfo;
mod externaltools;
mod filebrowser;
mod filesearch;
mod markdown;
mod modelines;
mod pythonconsole;
mod quickopen;
mod snippets;
mod sort;
mod spell;
mod terminal;
mod time;
mod todolist;

use std::rc::Rc;

use crate::plugin::activatable::{Plugin, PluginFactory, PluginInfo};

pub fn builtin_plugins() -> Vec<(PluginInfo, PluginFactory)> {
    vec![
        changecase::factory(),
        docinfo::factory(),
        filebrowser::factory(),
        filesearch::factory(),
        markdown::factory(),
        modelines::factory(),
        sort::factory(),
        spell::factory(),
        time::factory(),
        todolist::factory(),
        externaltools::factory(),
        pythonconsole::factory(),
        quickopen::factory(),
        snippets::factory(),
        terminal::factory(),
    ]
}

fn info(
    module: &str,
    name: &str,
    description: &str,
) -> PluginInfo {
    PluginInfo {
        module: module.into(),
        name: name.into(),
        description: description.into(),
        authors: "gtk-edit".into(),
        copyright: "GPL-2.0-or-later".into(),
        website: String::new(),
        builtin: true,
    }
}

fn make_factory<F>(info: PluginInfo, f: F) -> (PluginInfo, PluginFactory)
where
    F: Fn() -> Box<dyn Plugin> + 'static,
{
    (info.clone(), Rc::new(f) as PluginFactory)
}
