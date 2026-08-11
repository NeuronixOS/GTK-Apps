//! gtk-neuron capability API for gtk-term.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use serde_json::{json, Value};
use vte4::prelude::*;

use crate::config::Config;

pub fn attach(
    header: &gtk::HeaderBar,
    window: &gtk::ApplicationWindow,
    main: &impl IsA<gtk::Widget>,
    notebook: &gtk::Notebook,
    config: &Rc<RefCell<Config>>,
) {
    let window_cap = window.clone();
    let notebook = notebook.clone();
    let config = Rc::clone(config);
    let handler: gtk_neuron::CapabilityHandler =
        Rc::new(move |name, args| dispatch(&window_cap, &notebook, &config, name, args));
    gtk_neuron::attach_driving_mode(
        header,
        window,
        main,
        gtk_neuron::term_drive_context(handler),
    );
}

fn dispatch(
    window: &gtk::ApplicationWindow,
    notebook: &gtk::Notebook,
    config: &Rc<RefCell<Config>>,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    match name {
        "/run-term-command" => {
            let cmd = arg_str(args, "command")?;
            let term = crate::current_terminal(notebook)
                .ok_or_else(|| "no active terminal".to_string())?;
            let line = if cmd.ends_with('\n') {
                cmd.to_string()
            } else {
                format!("{cmd}\n")
            };
            term.feed_child(line.as_bytes());
            Ok(json!({ "ok": true }))
        }
        "/write-terminal" => {
            let text = arg_str(args, "text")?;
            let term = crate::current_terminal(notebook)
                .ok_or_else(|| "no active terminal".to_string())?;
            term.feed_child(text.as_bytes());
            Ok(json!({ "ok": true }))
        }
        "/read-terminal-output" => {
            let term = crate::current_terminal(notebook)
                .ok_or_else(|| "no active terminal".to_string())?;
            let title = term
                .window_title()
                .map(|t| t.to_string())
                .unwrap_or_default();
            Ok(json!({
                "note": "VTE text capture is limited in v1; returning window title",
                "title": title,
            }))
        }
        "/list-tabs" => {
            let mut tabs = Vec::new();
            let n = notebook.n_pages();
            for i in 0..n {
                let title = notebook
                    .nth_page(Some(i))
                    .and_then(|child| {
                        child
                            .downcast::<gtk::ScrolledWindow>()
                            .ok()
                            .and_then(|s| s.child())
                            .and_then(|c| c.downcast::<vte4::Terminal>().ok())
                            .and_then(|t| t.window_title())
                    })
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| format!("Tab {}", i + 1));
                tabs.push(json!({ "index": i, "title": title }));
            }
            Ok(json!({ "tabs": tabs }))
        }
        "/new-tab" => {
            crate::create_tab(window, notebook, &config.borrow());
            Ok(json!({ "ok": true, "pages": notebook.n_pages() }))
        }
        other => Err(format!("unknown capability {other}")),
    }
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing string arg `{key}`"))
}
