//! gtk-neuron capability API for gtk-files.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::{json, Value};

use crate::file_ops;
use crate::window::FilesWindow;

/// Build Self Driving UI (header toggle + right panel); caller packs / wraps.
pub fn make_driving_mode(
    fw_slot: &Rc<RefCell<Option<Rc<FilesWindow>>>>,
) -> gtk_neuron::DrivingMode {
    let slot = Rc::clone(fw_slot);
    let handler: gtk_neuron::CapabilityHandler = Rc::new(move |name, args| {
        let Some(fw) = slot.borrow().clone() else {
            return Err("file manager not ready".into());
        };
        dispatch(&fw, name, args)
    });
    gtk_neuron::create_driving_mode(gtk_neuron::files_drive_context(handler))
}

fn dispatch(fw: &Rc<FilesWindow>, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "/list-dir" => {
            let path = arg_path(args, "path")?;
            let mut entries = Vec::new();
            for ent in std::fs::read_dir(&path).map_err(|e| e.to_string())? {
                let ent = ent.map_err(|e| e.to_string())?;
                let meta = ent.metadata().ok();
                entries.push(json!({
                    "name": ent.file_name().to_string_lossy(),
                    "path": ent.path().to_string_lossy(),
                    "is_dir": meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                }));
            }
            Ok(json!({ "entries": entries }))
        }
        "/get-selection" => {
            let paths: Vec<String> = fw
                .current_tab()
                .selected_paths()
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            let cwd = fw
                .current_tab()
                .location_path()
                .map(|p| p.to_string_lossy().into_owned());
            Ok(json!({ "paths": paths, "cwd": cwd }))
        }
        "/open-path" => {
            let path = arg_path(args, "path")?;
            if path.is_dir() {
                fw.current_tab().navigate_path(&path, true);
            } else if let Some(parent) = path.parent() {
                fw.current_tab().navigate_path(parent, true);
                fw.current_tab().reveal_path(&path);
            } else {
                return Err("invalid path".into());
            }
            Ok(json!({ "ok": true, "path": path.to_string_lossy() }))
        }
        "/create-folder" => {
            let parent = arg_path(args, "parent")?;
            let name = arg_str(args, "name")?;
            let dest = file_ops::create_folder(&parent, name)?;
            fw.current_tab().refresh();
            Ok(json!({ "path": dest.to_string_lossy() }))
        }
        "/rename-files" => {
            let path = arg_path(args, "path")?;
            let new_name = arg_str(args, "new_name")?;
            let dest = file_ops::rename(&path, new_name)?;
            fw.current_tab().refresh();
            Ok(json!({ "path": dest.to_string_lossy() }))
        }
        "/move-files" => {
            let paths = arg_paths(args, "paths")?;
            let dest = arg_path(args, "dest")?;
            move_or_copy(&paths, &dest, true)?;
            fw.current_tab().refresh();
            Ok(json!({ "ok": true }))
        }
        "/copy-files" => {
            let paths = arg_paths(args, "paths")?;
            let dest = arg_path(args, "dest")?;
            move_or_copy(&paths, &dest, false)?;
            fw.current_tab().refresh();
            Ok(json!({ "ok": true }))
        }
        "/trash-files" => {
            let paths = arg_paths(args, "paths")?;
            let fw2 = Rc::clone(fw);
            file_ops::trash_paths(Some(&fw.window), &paths, false, move || {
                fw2.current_tab().refresh();
            });
            Ok(json!({ "ok": true }))
        }
        other => Err(format!("unknown capability {other}")),
    }
}

fn move_or_copy(paths: &[PathBuf], dest_dir: &Path, move_files: bool) -> Result<(), String> {
    if !dest_dir.is_dir() {
        return Err(format!(
            "destination is not a directory: {}",
            dest_dir.display()
        ));
    }
    for src in paths {
        let name = src
            .file_name()
            .ok_or_else(|| format!("bad path {}", src.display()))?;
        let dest = dest_dir.join(name);
        if move_files {
            std::fs::rename(src, &dest).or_else(|_| {
                copy_tree(src, &dest)?;
                if src.is_dir() {
                    std::fs::remove_dir_all(src).map_err(|e| e.to_string())
                } else {
                    std::fs::remove_file(src).map_err(|e| e.to_string())
                }
            })?;
        } else {
            copy_tree(src, &dest)?;
        }
    }
    Ok(())
}

fn copy_tree(src: &Path, dest: &Path) -> Result<(), String> {
    if src.is_dir() {
        std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        for ent in std::fs::read_dir(src).map_err(|e| e.to_string())? {
            let ent = ent.map_err(|e| e.to_string())?;
            copy_tree(&ent.path(), &dest.join(ent.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(src, dest).map_err(|e| e.to_string())?;
        Ok(())
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

fn arg_paths(args: &Value, key: &str) -> Result<Vec<PathBuf>, String> {
    let arr = args
        .get(key)
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("missing array arg `{key}`"))?;
    Ok(arr
        .iter()
        .filter_map(|v| v.as_str().map(PathBuf::from))
        .collect())
}
