//! OpenAI-compatible Chat Completions (OpenAI, Groq, Mistral, DeepSeek, …).

use serde_json::{json, Value};

use crate::capabilities::tools_prompt;
use crate::protocol::CapabilitySpec;

pub struct OpenAiCompatConfig<'a> {
    pub name: &'a str,
    pub api_key: &'a str,
    pub base_url: &'a str,
    pub model: &'a str,
}

pub fn chat_openai_compat(
    cfg: OpenAiCompatConfig<'_>,
    system: &str,
    user: &str,
    history: &[(String, String)],
    caps: &[CapabilitySpec],
) -> Result<String, String> {
    if cfg.api_key.is_empty() {
        return Err(format!("{} API key not configured", cfg.name));
    }

    let mut messages: Vec<Value> = Vec::new();
    messages.push(json!({
        "role": "system",
        "content": format!("{}\n\n{}", system, tools_prompt(caps))
    }));
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
        "model": cfg.model,
        "messages": messages,
        "temperature": 0.4,
    });

    let url = format!(
        "{}/chat/completions",
        cfg.base_url.trim_end_matches('/')
    );

    let resp = match ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {}", cfg.api_key))
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
            return Err(format!("{} HTTP {code}: {detail}", cfg.name));
        }
        Err(e) => return Err(format!("{} request failed: {e}", cfg.name)),
    };

    let v: Value = resp
        .into_json()
        .map_err(|e| format!("{} response parse: {e}", cfg.name))?;

    if let Some(err) = v.get("error") {
        return Err(format!(
            "{} error: {}",
            cfg.name,
            err.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or(&err.to_string())
        ));
    }

    if let Some(text) = v
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
    {
        if !text.is_empty() {
            return Ok(text.to_string());
        }
    }

    Err(format!("unexpected {} response: {v}", cfg.name))
}
