use serde_json::{json, Value};

use crate::capabilities::tools_prompt;
use crate::credentials::CredentialsFile;
use crate::protocol::CapabilitySpec;

pub fn chat_claude(
    creds: &CredentialsFile,
    system: &str,
    user: &str,
    history: &[(String, String)],
    caps: &[CapabilitySpec],
) -> Result<String, String> {
    if creds.claude.api_key.is_empty() {
        return Err("Claude API key not configured".into());
    }

    let mut messages: Vec<Value> = Vec::new();
    for (role, text) in history {
        messages.push(json!({
            "role": if role == "assistant" { "assistant" } else { "user" },
            "content": text
        }));
    }
    messages.push(json!({
        "role": "user",
        "content": user
    }));

    let body = json!({
        "model": creds.claude.model_or_default(),
        "max_tokens": 4096,
        "system": format!("{}\n\n{}", system, tools_prompt(caps)),
        "messages": messages
    });

    let resp = ureq::post("https://api.anthropic.com/v1/messages")
        .set("Content-Type", "application/json")
        .set("x-api-key", &creds.claude.api_key)
        .set("anthropic-version", "2023-06-01")
        .send_json(body)
        .map_err(|e| format!("Claude request failed: {e}"))?;

    let v: Value = resp
        .into_json()
        .map_err(|e| format!("Claude response parse: {e}"))?;

    if let Some(err) = v.get("error") {
        return Err(format!(
            "Claude error: {}",
            err.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or(&err.to_string())
        ));
    }

    // content is an array of blocks; collect text
    let mut out = String::new();
    if let Some(arr) = v.get("content").and_then(|c| c.as_array()) {
        for block in arr {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    out.push_str(t);
                }
            }
        }
    }
    if out.is_empty() {
        return Err(format!("unexpected Claude response: {v}"));
    }
    Ok(out)
}
