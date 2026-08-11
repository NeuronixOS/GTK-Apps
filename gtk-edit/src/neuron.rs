//! gtk-neuron capability API for gtk-edit.

use std::path::PathBuf;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use serde_json::{json, Value};

use crate::window::EditorWindow;

pub fn attach(
    header: &gtk::HeaderBar,
    window: &gtk::ApplicationWindow,
    main: &impl IsA<gtk::Widget>,
    ew: &Rc<EditorWindow>,
) {
    let ew = Rc::clone(ew);
    let handler: gtk_neuron::CapabilityHandler =
        Rc::new(move |name, args| dispatch(&ew, name, args));
    gtk_neuron::attach_driving_mode(
        header,
        window,
        main,
        gtk_neuron::edit_drive_context(handler),
    );
}

fn dispatch(ew: &Rc<EditorWindow>, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "/open-file" => {
            let path = arg_path(args, "path")?;
            ew.open_path(&path);
            Ok(json!({ "ok": true }))
        }
        "/list-tabs" => {
            let mut tabs = Vec::new();
            for group in ew.groups.borrow().iter() {
                for tab in group.tabs.borrow().iter() {
                    tabs.push(json!({
                        "title": tab.document.title(),
                        "path": tab.document.path().map(|p| p.to_string_lossy().into_owned()),
                        "modified": tab.document.is_modified(),
                    }));
                }
            }
            Ok(json!({ "tabs": tabs }))
        }
        "/read-buffer" => {
            let tab = resolve_tab(ew, args)?;
            Ok(json!({ "text": tab.document.text() }))
        }
        "/write-buffer" => {
            let text = arg_str(args, "text")?;
            let tab = ew
                .current_tab()
                .ok_or_else(|| "no current tab".to_string())?;
            tab.document.set_text(text);
            tab.refresh_title();
            ew.update_statusbar();
            Ok(json!({ "ok": true }))
        }
        "/replace-selection" => {
            let text = arg_str(args, "text")?;
            let tab = ew
                .current_tab()
                .ok_or_else(|| "no current tab".to_string())?;
            let buf = &tab.document.buffer;
            if let Some((mut start, mut end)) = buf.selection_bounds() {
                buf.begin_user_action();
                buf.delete(&mut start, &mut end);
                buf.insert(&mut start, text);
                buf.end_user_action();
            } else {
                let insert = buf
                    .mark("insert")
                    .ok_or_else(|| "no insert mark".to_string())?;
                let mut iter = buf.iter_at_mark(&insert);
                buf.begin_user_action();
                buf.insert(&mut iter, text);
                buf.end_user_action();
            }
            Ok(json!({ "ok": true }))
        }
        "/save-file" => {
            let tab = ew
                .current_tab()
                .ok_or_else(|| "no current tab".to_string())?;
            let ok = ew.save_tab(&tab, false);
            Ok(json!({ "ok": ok }))
        }
        "/find" => {
            let query = arg_str(args, "query")?;
            ew.search.entry.set_text(query);
            ew.search.show_find_with(Some(query));
            let tab = ew
                .current_tab()
                .ok_or_else(|| "no current tab".to_string())?;
            let hl = ew.config.borrow().editor.search_highlighting;
            let found = ew.search.find(&tab.document.buffer, &tab.view, true, hl);
            Ok(json!({ "found": found }))
        }
        other => Err(format!("unknown capability {other}")),
    }
}

fn resolve_tab(
    ew: &EditorWindow,
    args: &Value,
) -> Result<Rc<crate::tab::EditorTab>, String> {
    if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
        let path = PathBuf::from(path);
        for group in ew.groups.borrow().iter() {
            for tab in group.tabs.borrow().iter() {
                if tab.document.path().as_deref() == Some(path.as_path()) {
                    return Ok(Rc::clone(tab));
                }
            }
        }
        return Err(format!("no tab for {}", path.display()));
    }
    ew.current_tab()
        .ok_or_else(|| "no current tab".to_string())
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing string arg `{key}`"))
}

fn arg_path(args: &Value, key: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(arg_str(args, key)?))
}
