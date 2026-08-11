//! AI provider backends.

mod claude;
mod cursor;
mod gemini;
mod openai_compat;

use crate::credentials::CredentialsFile;
use crate::protocol::{CapabilitySpec, ProviderId};

pub use claude::chat_claude;
pub use cursor::chat_cursor;
pub use gemini::chat_gemini;
use openai_compat::{chat_openai_compat, OpenAiCompatConfig};

fn model_or<'a>(configured: &'a str, default: &'a str) -> &'a str {
    let m = configured.trim();
    if m.is_empty() {
        default
    } else {
        m
    }
}

pub fn chat(
    provider: ProviderId,
    creds: &CredentialsFile,
    system: &str,
    user: &str,
    history: &[(String, String)],
    caps: &[CapabilitySpec],
) -> Result<String, String> {
    match provider {
        ProviderId::Gemini => chat_gemini(creds, system, user, history, caps),
        ProviderId::Claude => chat_claude(creds, system, user, history, caps),
        ProviderId::Cursor => chat_cursor(creds, system, user, history, caps),
        ProviderId::OpenAi => chat_openai_compat(
            OpenAiCompatConfig {
                name: "OpenAI",
                api_key: &creds.openai.api_key,
                base_url: "https://api.openai.com/v1",
                model: model_or(&creds.openai.model, "gpt-4o-mini"),
            },
            system,
            user,
            history,
            caps,
        ),
        ProviderId::Groq => chat_openai_compat(
            OpenAiCompatConfig {
                name: "Groq",
                api_key: &creds.groq.api_key,
                base_url: "https://api.groq.com/openai/v1",
                model: model_or(&creds.groq.model, "llama-3.3-70b-versatile"),
            },
            system,
            user,
            history,
            caps,
        ),
        ProviderId::Mistral => chat_openai_compat(
            OpenAiCompatConfig {
                name: "Mistral",
                api_key: &creds.mistral.api_key,
                base_url: "https://api.mistral.ai/v1",
                model: model_or(&creds.mistral.model, "mistral-small-latest"),
            },
            system,
            user,
            history,
            caps,
        ),
        ProviderId::DeepSeek => chat_openai_compat(
            OpenAiCompatConfig {
                name: "DeepSeek",
                api_key: &creds.deepseek.api_key,
                base_url: "https://api.deepseek.com",
                model: model_or(&creds.deepseek.model, "deepseek-chat"),
            },
            system,
            user,
            history,
            caps,
        ),
    }
}
