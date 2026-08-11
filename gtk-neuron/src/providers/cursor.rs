//! Cursor provider via python/cursor_worker.py (Cursor SDK).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use crate::capabilities::tools_prompt;
use crate::credentials::CredentialsFile;
use crate::protocol::CapabilitySpec;

pub fn chat_cursor(
    creds: &CredentialsFile,
    system: &str,
    user: &str,
    history: &[(String, String)],
    caps: &[CapabilitySpec],
) -> Result<String, String> {
    if creds.cursor.api_key.is_empty() {
        return Err("Cursor API key not configured".into());
    }

    let (python, worker) = find_python_and_worker()?;
    let mut child = Command::new(&python)
        .arg(&worker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start cursor_worker ({python}): {e}"))?;

    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut stderr = child.stderr.take();

    let hist: Vec<Value> = history
        .iter()
        .map(|(r, t)| json!({"role": r, "text": t}))
        .collect();

    let req = json!({
        "api_key": creds.cursor.api_key,
        "system": format!("{}\n\n{}", system, tools_prompt(caps)),
        "user": user,
        "history": hist,
        "cwd": std::env::var("HOME").unwrap_or_else(|_| ".".into())
    });

    writeln!(stdin, "{}", req).map_err(|e| e.to_string())?;
    drop(stdin);

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("cursor_worker read: {e}"))?;

    let status = child.wait().map_err(|e| e.to_string())?;
    let mut stderr_text = String::new();
    if let Some(ref mut s) = stderr {
        let _ = s.read_to_string(&mut stderr_text);
    }

    let trimmed = line.trim();
    if !trimmed.is_empty() {
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                let msg = if err.to_ascii_lowercase().contains("invalid user api key")
                    || err.to_ascii_lowercase().contains("unauthenticated")
                {
                    format!(
                        "Cursor: {err}. Create a User API Key at https://cursor.com/dashboard/cloud-agents (not an OpenAI/Anthropic sk- key), then Connect again under the cursor provider."
                    )
                } else {
                    format!("Cursor: {err}")
                };
                return Err(msg);
            }
            if status.success() {
                return v
                    .get("text")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| format!("unexpected cursor_worker response: {v}"));
            }
        }
    }

    if !status.success() {
        return Err(format!(
            "cursor_worker exited {}{}{}",
            status,
            if stderr_text.is_empty() { "" } else { ": " },
            stderr_text.trim()
        ));
    }

    Err(format!(
        "cursor_worker returned no JSON (stderr: {})",
        stderr_text.trim()
    ))
}

fn find_python_and_worker() -> Result<(String, PathBuf), String> {
    let worker = find_worker()?;
    let python = find_python(&worker);
    Ok((python, worker))
}

fn find_python(worker: &std::path::Path) -> String {
    if let Ok(p) = std::env::var("GTK_NEURON_PYTHON") {
        if PathBuf::from(&p).is_file() {
            return p;
        }
    }
    // Prefer the crate-local venv next to the worker script.
    if let Some(dir) = worker.parent() {
        let venv_py = dir.join(".venv/bin/python");
        if venv_py.is_file() {
            return venv_py.to_string_lossy().into_owned();
        }
        let venv_py3 = dir.join(".venv/bin/python3");
        if venv_py3.is_file() {
            return venv_py3.to_string_lossy().into_owned();
        }
    }
    "python3".into()
}

fn find_worker() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("GTK_NEURON_CURSOR_WORKER") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Ok(pb);
        }
    }
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("python/cursor_worker.py"),
        PathBuf::from("/usr/local/lib/neuronix/gtk-apps/gtk-neuron/python/cursor_worker.py"),
    ];
    for c in candidates {
        if c.is_file() {
            return Ok(c);
        }
    }
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            let c = dir.join("../python/cursor_worker.py");
            if c.is_file() {
                return Ok(c);
            }
        }
    }
    Err("cursor_worker.py not found".into())
}
