//! Length-prefixed JSON frames on Unix stream sockets.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use crate::protocol::Message;

pub fn socket_path() -> PathBuf {
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

pub fn write_message(stream: &mut UnixStream, msg: &Message) -> Result<(), String> {
    let bytes = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    if bytes.len() > u32::MAX as usize {
        return Err("message too large".into());
    }
    let len = (bytes.len() as u32).to_be_bytes();
    stream.write_all(&len).map_err(|e| e.to_string())?;
    stream.write_all(&bytes).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())
}

pub fn read_message(stream: &mut UnixStream) -> Result<Message, String> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 16 * 1024 * 1024 {
        return Err("frame too large".into());
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).map_err(|e| e.to_string())?;
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

/// Try connect; on failure spawn daemon and retry briefly.
pub fn connect_or_spawn() -> Result<UnixStream, String> {
    let path = socket_path();
    if let Ok(stream) = UnixStream::connect(&path) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        return Ok(stream);
    }

    spawn_daemon()?;

    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(50));
        if let Ok(stream) = UnixStream::connect(&path) {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
            return Ok(stream);
        }
    }
    Err(format!(
        "could not connect to gtk-neurond at {}",
        path.display()
    ))
}

fn spawn_daemon() -> Result<(), String> {
    // Shared suite autostart (also invoked from gtk_theme::apply_chrome).
    gtk_theme::ensure_neuron_daemon();
    if UnixStream::connect(socket_path()).is_ok() {
        return Ok(());
    }
    Err("gtk-neurond did not become ready after autostart".into())
}
