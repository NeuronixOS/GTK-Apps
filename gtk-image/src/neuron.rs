//! gtk-neuron capability API for gtk-image.

use std::path::PathBuf;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use serde_json::{json, Value};

use crate::window::ImageWindow;

pub fn attach(
    header: &gtk::HeaderBar,
    window: &gtk::ApplicationWindow,
    main: &impl IsA<gtk::Widget>,
    iw: &Rc<ImageWindow>,
) {
    let iw = Rc::clone(iw);
    let handler: gtk_neuron::CapabilityHandler =
        Rc::new(move |name, args| dispatch(&iw, name, args));
    gtk_neuron::attach_driving_mode(
        header,
        window,
        main,
        gtk_neuron::image_drive_context(handler),
    );
}

fn dispatch(iw: &Rc<ImageWindow>, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "/open-image" => {
            let path = arg_path(args, "path")?;
            iw.open_path(&path);
            Ok(json!({ "ok": true }))
        }
        "/get-current-image" => {
            let path = iw.current_path();
            Ok(json!({ "path": path.map(|p| p.to_string_lossy().into_owned()) }))
        }
        "/rotate" => {
            let dir = arg_str(args, "direction")?;
            match dir {
                "cw" => iw.rotate_cw(),
                "ccw" => iw.rotate_ccw(),
                other => return Err(format!("unknown direction {other}")),
            }
            Ok(json!({ "ok": true }))
        }
        "/flip" => {
            let axis = arg_str(args, "axis")?;
            match axis {
                "horizontal" => iw.flip_horizontal(),
                "vertical" => iw.flip_vertical(),
                other => return Err(format!("unknown axis {other}")),
            }
            Ok(json!({ "ok": true }))
        }
        "/save-image" => {
            let path = arg_path(args, "path")?;
            iw.save_to(&path)?;
            Ok(json!({ "path": path.to_string_lossy() }))
        }
        "/edit-image" => {
            let op = arg_str(args, "op")?;
            match op {
                "rotate-cw" | "cw" => iw.rotate_cw(),
                "rotate-ccw" | "ccw" => iw.rotate_ccw(),
                "flip-h" | "flip-horizontal" => iw.flip_horizontal(),
                "flip-v" | "flip-vertical" => iw.flip_vertical(),
                other => return Err(format!("unknown edit op {other}")),
            }
            Ok(json!({ "ok": true }))
        }
        other => Err(format!("unknown capability {other}")),
    }
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing string arg `{key}`"))
}

fn arg_path(args: &Value, key: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(arg_str(args, key)?))
}
