//! Best-effort autostart of `gtk-neurond` for the suite.

use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Ensure the AI driving daemon is listening. No-op if already up.
///
/// Called from [`crate::apply_chrome`] so every suite app that applies chrome
/// brings the daemon up without each app depending on gtk-neuron.
pub fn ensure_neuron_daemon() {
    if neuron_socket_connectable() {
        return;
    }
    // Stale socket file left behind by a crashed daemon.
    let sock = neuron_socket_path();
    let _ = std::fs::remove_file(&sock);

    if let Err(e) = spawn_neuron_daemon() {
        eprintln!("gtk-theme: gtk-neurond autostart skipped: {e}");
        return;
    }
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(50));
        if neuron_socket_connectable() {
            return;
        }
    }
}

fn neuron_socket_path() -> PathBuf {
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        if !runtime.is_empty() {
            return PathBuf::from(runtime).join("gtk-neuron.sock");
        }
    }
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-apps")
        .join("gtk-neuron.sock")
}

fn neuron_socket_connectable() -> bool {
    UnixStream::connect(neuron_socket_path()).is_ok()
}

fn spawn_neuron_daemon() -> Result<(), String> {
    let sock = neuron_socket_path();
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let exe = find_neurond_binary()?;
    // Detach into its own process group so closing the GTK app does not
    // take the daemon down with it.
    let mut cmd = Command::new(&exe);
    cmd.arg("--daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    cmd.spawn()
        .map_err(|e| format!("spawn {}: {e}", exe.display()))?;
    Ok(())
}

fn find_neurond_binary() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("GTK_NEUROND") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Ok(pb);
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();

    // Suite tree relative to this crate (gtk-theme → ../gtk-neuron/...).
    let theme_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(suite) = theme_root.parent() {
        candidates.push(suite.join("gtk-neuron/target/release/gtk-neurond"));
        candidates.push(suite.join("gtk-neuron/target/debug/gtk-neurond"));
    }

    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            candidates.push(dir.join("gtk-neurond"));
            candidates.push(dir.join("../../../gtk-neuron/target/release/gtk-neurond"));
            candidates.push(dir.join("../../../gtk-neuron/target/debug/gtk-neurond"));
            candidates.push(dir.join("../../gtk-neuron/target/release/gtk-neurond"));
            candidates.push(dir.join("../../gtk-neuron/target/debug/gtk-neurond"));
        }
    }

    candidates.push(PathBuf::from(
        "/usr/local/lib/neuronix/gtk-apps/gtk-neuron/gtk-neurond",
    ));
    candidates.push(PathBuf::from("/usr/local/bin/gtk-neurond"));

    if let Ok(path) = std::env::var("PATH") {
        for entry in path.split(':') {
            candidates.push(PathBuf::from(entry).join("gtk-neurond"));
        }
    }

    for c in &candidates {
        if c.is_file() {
            return Ok(c.canonicalize().unwrap_or_else(|_| c.clone()));
        }
    }

    Err(format!(
        "gtk-neurond not found (set GTK_NEUROND); tried under {}",
        theme_root
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("gtk-neuron")
            .display()
    ))
}
