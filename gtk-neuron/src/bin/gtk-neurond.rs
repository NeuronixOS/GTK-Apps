//! gtk-neurond — local AI driving daemon.

use std::collections::HashMap;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{json, Value};

use gtk_neuron::credentials;
use gtk_neuron::protocol::*;
use gtk_neuron::providers;
use gtk_neuron::socket::{read_message, socket_path, write_message};

fn main() {
    if let Err(e) = run() {
        eprintln!("gtk-neurond: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let path = socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| format!("bind {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    eprintln!("gtk-neurond listening on {}", path.display());

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream) {
                        eprintln!("client error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("accept: {e}"),
        }
    }
    Ok(())
}

struct Session {
    provider: ProviderId,
    history: Vec<(String, String)>,
}

struct SharedState {
    app_id: String,
    capabilities: Vec<CapabilitySpec>,
    sessions: HashMap<String, Session>,
    pending_tools: HashMap<String, Arc<Mutex<Option<Result<Value, String>>>>>,
}

fn handle_client(stream: UnixStream) -> Result<(), String> {
    // Full-duplex: read on one fd, write on a clone. Holding a mutex across
    // blocking read() prevented chat workers from delivering replies until the
    // next client message arrived.
    let mut stream_read = stream;
    let stream_write = stream_read
        .try_clone()
        .map_err(|e| format!("clone socket: {e}"))?;
    let stream = Arc::new(Mutex::new(stream_write));

    let state = Arc::new(Mutex::new(SharedState {
        app_id: String::new(),
        capabilities: Vec::new(),
        sessions: HashMap::new(),
        pending_tools: HashMap::new(),
    }));

    loop {
        let msg = match read_message(&mut stream_read) {
            Ok(m) => m,
            Err(_) => return Ok(()),
        };

        match msg {
            Message::Hello(h) => {
                {
                    let mut st = state.lock().map_err(|e| e.to_string())?;
                    st.app_id = h.app_id;
                    st.capabilities = h.capabilities;
                }
                write_locked(
                    &stream,
                    &Message::HelloOk(HelloOkParams {
                        daemon_version: env!("CARGO_PKG_VERSION").into(),
                        protocol: PROTOCOL_VERSION,
                    }),
                )?;
            }
            Message::ProvidersList | Message::CredentialsStatus => {
                let creds = credentials::load();
                write_locked(
                    &stream,
                    &Message::ProvidersStatus(ProvidersStatusParams {
                        providers: credentials::status_list(&creds),
                    }),
                )?;
            }
            Message::CredentialsSet(p) => {
                match credentials::set_api_key(p.provider, &p.api_key) {
                    Ok(creds) => write_locked(
                        &stream,
                        &Message::ProvidersStatus(ProvidersStatusParams {
                            providers: credentials::status_list(&creds),
                        }),
                    )?,
                    Err(e) => write_locked(
                        &stream,
                        &Message::Error(ErrorParams { message: e }),
                    )?,
                }
            }
            Message::ChatStart(p) => {
                let mut st = state.lock().map_err(|e| e.to_string())?;
                st.sessions.insert(
                    p.session_id,
                    Session {
                        provider: p.provider,
                        history: Vec::new(),
                    },
                );
            }
            Message::ChatSend(p) => {
                let stream_c = stream.clone();
                let state_c = state.clone();
                thread::spawn(move || {
                    if let Err(e) = run_chat(state_c, stream_c, p) {
                        eprintln!("chat error: {e}");
                    }
                });
            }
            Message::ChatCancel(_) => {}
            Message::CapabilityResult(r) => {
                let st = state.lock().map_err(|e| e.to_string())?;
                if let Some(slot) = st.pending_tools.get(&r.id) {
                    *slot.lock().map_err(|e| e.to_string())? = Some(Ok(r.result));
                }
            }
            Message::CapabilityError(e) => {
                let st = state.lock().map_err(|e| e.to_string())?;
                if let Some(slot) = st.pending_tools.get(&e.id) {
                    *slot.lock().map_err(|e| e.to_string())? = Some(Err(e.message.clone()));
                }
            }
            Message::CapabilityConfirm(c) => {
                if !c.allow {
                    let st = state.lock().map_err(|e| e.to_string())?;
                    if let Some(slot) = st.pending_tools.get(&c.id) {
                        *slot.lock().map_err(|e| e.to_string())? =
                            Some(Err("user denied capability".into()));
                    }
                }
            }
            other => {
                write_locked(
                    &stream,
                    &Message::Error(ErrorParams {
                        message: format!("unexpected message from client: {other:?}"),
                    }),
                )?;
            }
        }
    }
}

fn run_chat(
    state: Arc<Mutex<SharedState>>,
    stream: Arc<Mutex<UnixStream>>,
    p: ChatSendParams,
) -> Result<(), String> {
    let (provider, history, caps, app_id) = {
        let mut st = state.lock().map_err(|e| e.to_string())?;
        let session = st
            .sessions
            .get_mut(&p.session_id)
            .ok_or_else(|| "unknown session; call chat.start first".to_string())?;
        session.history.push(("user".into(), p.text.clone()));
        (
            session.provider,
            session.history.clone(),
            st.capabilities.clone(),
            st.app_id.clone(),
        )
    };

    let session_id = p.session_id.clone();
    let user_text = p.text.clone();
    let creds = credentials::load();
    let system = format!(
        "You are ꔮ, the Neuronix driving assistant for {app_id}. Be concise. Prefer using tools when the user asks to act on files, terminal, images, or editor buffers."
    );

    let prior: Vec<(String, String)> = history
        .iter()
        .rev()
        .skip(1)
        .rev()
        .cloned()
        .collect();

    let reply = match providers::chat(provider, &creds, &system, &user_text, &prior, &caps) {
        Ok(r) => r,
        Err(e) => {
            write_locked(
                &stream,
                &Message::ChatError(ChatErrorParams {
                    session_id,
                    message: e,
                }),
            )?;
            return Ok(());
        }
    };

    if let Some((tool, args)) = extract_tool_call(&reply) {
        let cap = caps.iter().find(|c| c.name == tool).cloned();
        let Some(cap) = cap else {
            write_locked(
                &stream,
                &Message::ChatError(ChatErrorParams {
                    session_id,
                    message: format!("model requested unknown tool {tool}"),
                }),
            )?;
            return Ok(());
        };

        let invoke_id = format!("inv-{}", uuid_like());
        let slot: Arc<Mutex<Option<Result<Value, String>>>> = Arc::new(Mutex::new(None));
        {
            let mut st = state.lock().map_err(|e| e.to_string())?;
            st.pending_tools.insert(invoke_id.clone(), slot.clone());
        }

        if cap.requires_confirm {
            write_locked(
                &stream,
                &Message::CapabilityPropose(CapabilityProposeParams {
                    id: invoke_id.clone(),
                    name: tool.clone(),
                    args: args.clone(),
                    requires_confirm: true,
                }),
            )?;
        } else {
            write_locked(
                &stream,
                &Message::CapabilityInvoke(CapabilityInvokeParams {
                    id: invoke_id.clone(),
                    name: tool.clone(),
                    args,
                }),
            )?;
        }

        let result = wait_for_tool(&slot, 120)?;
        {
            let mut st = state.lock().map_err(|e| e.to_string())?;
            st.pending_tools.remove(&invoke_id);
        }

        let summary = match result {
            Ok(v) => format!("Tool {tool} result: {v}"),
            Err(e) => format!("Tool {tool} failed: {e}"),
        };
        finish_chat(&state, &stream, &session_id, &summary)?;
        return Ok(());
    }

    finish_chat(&state, &stream, &session_id, &reply)
}

fn finish_chat(
    state: &Arc<Mutex<SharedState>>,
    stream: &Arc<Mutex<UnixStream>>,
    session_id: &str,
    text: &str,
) -> Result<(), String> {
    write_locked(
        stream,
        &Message::ChatDelta(ChatDeltaParams {
            session_id: session_id.to_string(),
            text: text.to_string(),
        }),
    )?;
    write_locked(
        stream,
        &Message::ChatDone(ChatDoneParams {
            session_id: session_id.to_string(),
            text: text.to_string(),
        }),
    )?;
    let mut st = state.lock().map_err(|e| e.to_string())?;
    if let Some(s) = st.sessions.get_mut(session_id) {
        s.history.push(("assistant".into(), text.to_string()));
    }
    Ok(())
}

fn write_locked(stream: &Arc<Mutex<UnixStream>>, msg: &Message) -> Result<(), String> {
    let mut s = stream.lock().map_err(|e| e.to_string())?;
    write_message(&mut s, msg)
}

fn wait_for_tool(
    slot: &Arc<Mutex<Option<Result<Value, String>>>>,
    timeout_secs: u64,
) -> Result<Result<Value, String>, String> {
    let start = std::time::Instant::now();
    loop {
        {
            let guard = slot.lock().map_err(|e| e.to_string())?;
            if let Some(r) = guard.as_ref() {
                return Ok(r.clone());
            }
        }
        if start.elapsed().as_secs() > timeout_secs {
            return Ok(Err("capability invoke timed out".into()));
        }
        thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn extract_tool_call(text: &str) -> Option<(String, Value)> {
    for line in text.lines() {
        let line = line.trim();
        if !(line.starts_with('{') && line.contains("\"tool\"")) {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if let Some(tool) = v.get("tool").and_then(|t| t.as_str()) {
                let args = v.get("args").cloned().unwrap_or(json!({}));
                return Some((tool.to_string(), args));
            }
        }
    }
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if end > start {
                if let Ok(v) = serde_json::from_str::<Value>(&text[start..=end]) {
                    if let Some(tool) = v.get("tool").and_then(|t| t.as_str()) {
                        let args = v.get("args").cloned().unwrap_or(json!({}));
                        return Some((tool.to_string(), args));
                    }
                }
            }
        }
    }
    None
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{t:x}")
}
