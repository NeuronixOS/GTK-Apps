use serde_json::{json, Value};

use crate::capabilities::tools_prompt;
use crate::credentials::CredentialsFile;
use crate::protocol::CapabilitySpec;

fn gemini_model(creds: &CredentialsFile) -> &str {
    // Prefer explicit config; default to a current alias that accepts new keys.
    let m = creds.gemini.model.trim();
    if m.is_empty() {
        "gemini-flash-latest"
    } else {
        m
    }
}

pub fn chat_gemini(
    creds: &CredentialsFile,
    system: &str,
    user: &str,
    history: &[(String, String)],
    caps: &[CapabilitySpec],
) -> Result<String, String> {
    if creds.gemini.api_key.is_empty() {
        return Err("Gemini API key not configured".into());
    }

    let mut contents: Vec<Value> = Vec::new();
    for (role, text) in history {
        let gem_role = if role == "assistant" { "model" } else { "user" };
        contents.push(json!({
            "role": gem_role,
            "parts": [{"text": text}]
        }));
    }
    contents.push(json!({
        "role": "user",
        "parts": [{"text": user}]
    }));

    let body = json!({
        "systemInstruction": {
            "parts": [{"text": format!("{}\n\n{}", system, tools_prompt(caps))}]
        },
        "contents": contents
    });

    let model = gemini_model(creds);
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={}",
        creds.gemini.api_key
    );

    let resp = match ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_json(body)
    {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            let detail = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| {
                    v.pointer("/error/message")
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or(body);
            return Err(format!("Gemini HTTP {code}: {detail}"));
        }
        Err(e) => return Err(format!("Gemini request failed: {e}")),
    };

    let v: Value = resp
        .into_json()
        .map_err(|e| format!("Gemini response parse: {e}"))?;

    if let Some(err) = v.get("error") {
        return Err(format!(
            "Gemini error: {}",
            err.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or(&err.to_string())
        ));
    }

    // Collect all text parts (some responses split across parts).
    let mut out = String::new();
    if let Some(parts) = v.pointer("/candidates/0/content/parts").and_then(|p| p.as_array())
    {
        for part in parts {
            if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                out.push_str(t);
            }
        }
    }
    if out.is_empty() {
        let reason = v
            .pointer("/candidates/0/finishReason")
            .and_then(|r| r.as_str())
            .unwrap_or("unknown");
        return Err(format!(
            "Gemini returned no text (finishReason={reason}): {v}"
        ));
    }
    Ok(out)
}
