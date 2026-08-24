//! gtk-neuron capability API for gtk-video.

use std::path::PathBuf;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use serde_json::{json, Value};

use crate::window::VideoWindow;

pub fn attach(
    header: &gtk::HeaderBar,
    window: &gtk::ApplicationWindow,
    main: &impl IsA<gtk::Widget>,
    vw: &Rc<VideoWindow>,
) {
    let vw = Rc::clone(vw);
    let handler: gtk_neuron::CapabilityHandler =
        Rc::new(move |name, args| dispatch(&vw, name, args));
    gtk_neuron::attach_driving_mode(
        header,
        window,
        main,
        gtk_neuron::video_drive_context(handler),
    );
}

fn dispatch(vw: &Rc<VideoWindow>, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "/open-video" => {
            let path = arg_path(args, "path")?;
            vw.open_path(&path);
            Ok(json!({ "ok": true }))
        }
        "/get-current-video" => Ok(vw.current_state()),
        "/play-pause" => {
            if let Some(playing) = args.get("playing").and_then(|v| v.as_bool()) {
                vw.set_playing(playing);
            } else {
                vw.toggle_play();
            }
            Ok(json!({ "ok": true }))
        }
        "/seek" => {
            vw.seek_us(arg_time_us(args)?);
            Ok(json!({ "ok": true }))
        }
        "/set-in" => {
            vw.set_in_us(opt_time_us(args));
            Ok(json!({ "ok": true }))
        }
        "/set-out" => {
            vw.set_out_us(opt_time_us(args));
            Ok(json!({ "ok": true }))
        }
        "/set-range" => {
            let start = arg_named_time_us(args, "start_us", "start")?;
            let end = arg_named_time_us(args, "end_us", "end")?;
            vw.set_range_us(start, end);
            Ok(json!({ "ok": true }))
        }
        "/rotate" => {
            let dir = arg_str(args, "direction")?;
            match dir {
                "cw" => vw.rotate_cw(),
                "ccw" => vw.rotate_ccw(),
                other => return Err(format!("unknown direction {other}")),
            }
            Ok(json!({ "ok": true }))
        }
        "/flip" => {
            let axis = arg_str(args, "axis")?;
            match axis {
                "horizontal" => vw.flip_horizontal(),
                "vertical" => vw.flip_vertical(),
                other => return Err(format!("unknown axis {other}")),
            }
            Ok(json!({ "ok": true }))
        }
        "/crop" => {
            if let Some(preset) = args.get("preset").and_then(|v| v.as_str()) {
                if preset == "reset" || preset == "full" || preset == "none" {
                    vw.reset_crop();
                } else {
                    vw.apply_crop_preset(preset)?;
                }
            } else if args.get("x").is_some() {
                vw.set_crop_rect(
                    arg_i32(args, "x")?,
                    arg_i32(args, "y")?,
                    arg_i32(args, "w")?,
                    arg_i32(args, "h")?,
                )?;
            } else {
                return Err("crop needs `preset` or `x`,`y`,`w`,`h`".into());
            }
            Ok(json!({ "ok": true }))
        }
        "/export" => {
            let path = arg_path(args, "path")?;
            vw.export_to(path.clone())?;
            Ok(json!({ "path": path.to_string_lossy() }))
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

fn arg_i32(args: &Value, key: &str) -> Result<i32, String> {
    args.get(key)
        .and_then(|v| {
            v.as_i64()
                .map(|n| n as i32)
                .or_else(|| v.as_f64().map(|n| n.round() as i32))
        })
        .ok_or_else(|| format!("missing integer arg `{key}`"))
}

fn json_to_us(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_u64().map(|n| n as i64))
        .or_else(|| v.as_f64().map(|n| n.round() as i64))
}

fn seconds_to_us(v: &Value) -> Option<i64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|n| n as f64))
        .map(|s| (s * 1_000_000.0).round() as i64)
}

fn arg_time_us(args: &Value) -> Result<i64, String> {
    if let Some(v) = args.get("us") {
        return json_to_us(v).ok_or_else(|| "invalid `us`".into());
    }
    if let Some(v) = args.get("seconds") {
        return seconds_to_us(v).ok_or_else(|| "invalid `seconds`".into());
    }
    Err("missing `us` or `seconds`".into())
}

fn opt_time_us(args: &Value) -> Option<i64> {
    arg_time_us(args).ok()
}

fn arg_named_time_us(args: &Value, us_key: &str, sec_key: &str) -> Result<i64, String> {
    if let Some(v) = args.get(us_key) {
        return json_to_us(v).ok_or_else(|| format!("invalid `{us_key}`"));
    }
    if let Some(v) = args.get(sec_key) {
        return seconds_to_us(v).ok_or_else(|| format!("invalid `{sec_key}`"));
    }
    Err(format!("missing `{us_key}` or `{sec_key}`"))
}
