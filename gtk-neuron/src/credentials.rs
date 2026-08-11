//! Credential storage under ~/.config/gtk-apps/gtk-neuron/

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::protocol::{ProviderId, ProviderStatus};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CredentialsFile {
    #[serde(default)]
    pub cursor: ProviderCreds,
    #[serde(default)]
    pub gemini: ProviderCreds,
    #[serde(default)]
    pub claude: ClaudeCreds,
    #[serde(default)]
    pub openai: ProviderCreds,
    #[serde(default)]
    pub groq: ProviderCreds,
    #[serde(default)]
    pub mistral: ProviderCreds,
    #[serde(default)]
    pub deepseek: ProviderCreds,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderCreds {
    #[serde(default)]
    pub api_key: String,
    /// Optional path to OAuth desktop credentials.json (Gemini).
    #[serde(default)]
    pub credentials_json: String,
    /// Optional model id. Empty → provider default.
    #[serde(default)]
    pub model: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeCreds {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
}

impl ClaudeCreds {
    pub fn model_or_default(&self) -> &str {
        if self.model.is_empty() {
            "claude-sonnet-4-20250514"
        } else {
            &self.model
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-apps")
        .join("gtk-neuron")
}

pub fn credentials_path() -> PathBuf {
    config_dir().join("credentials.toml")
}

pub fn load() -> CredentialsFile {
    let path = credentials_path();
    let mut creds = if path.is_file() {
        match fs::read_to_string(&path) {
            Ok(s) => toml::from_str(&s).unwrap_or_default(),
            Err(_) => CredentialsFile::default(),
        }
    } else {
        CredentialsFile::default()
    };

    // Environment overrides (never written back unless set via UI).
    apply_env(&mut creds.cursor.api_key, "CURSOR_API_KEY");
    apply_env(&mut creds.gemini.api_key, "GEMINI_API_KEY");
    apply_env(&mut creds.claude.api_key, "ANTHROPIC_API_KEY");
    if creds.claude.api_key.is_empty() {
        apply_env(&mut creds.claude.api_key, "CLAUDE_API_KEY");
    }
    apply_env(&mut creds.openai.api_key, "OPENAI_API_KEY");
    apply_env(&mut creds.groq.api_key, "GROQ_API_KEY");
    apply_env(&mut creds.mistral.api_key, "MISTRAL_API_KEY");
    apply_env(&mut creds.deepseek.api_key, "DEEPSEEK_API_KEY");

    creds
}

fn apply_env(slot: &mut String, var: &str) {
    if let Ok(k) = std::env::var(var) {
        if !k.is_empty() {
            *slot = k;
        }
    }
}

pub fn save(creds: &CredentialsFile) -> Result<(), String> {
    let dir = config_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let text = toml::to_string_pretty(creds).map_err(|e| e.to_string())?;
    fs::write(credentials_path(), text).map_err(|e| e.to_string())
}

pub fn set_api_key(provider: ProviderId, api_key: &str) -> Result<CredentialsFile, String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err("API key is empty".into());
    }
    // Soft guidance: Cursor User API keys are not OpenAI sk-… provider keys.
    if provider == ProviderId::Cursor && api_key.starts_with("sk-") {
        return Err(
            "That looks like an OpenAI-style sk-… key. Cursor needs a User API Key from https://cursor.com/dashboard/cloud-agents (or Dashboard → API Keys), not a model-provider key."
                .into(),
        );
    }
    let mut creds = load();
    match provider {
        ProviderId::Cursor => creds.cursor.api_key = api_key.to_string(),
        ProviderId::Gemini => creds.gemini.api_key = api_key.to_string(),
        ProviderId::Claude => creds.claude.api_key = api_key.to_string(),
        ProviderId::OpenAi => creds.openai.api_key = api_key.to_string(),
        ProviderId::Groq => creds.groq.api_key = api_key.to_string(),
        ProviderId::Mistral => creds.mistral.api_key = api_key.to_string(),
        ProviderId::DeepSeek => creds.deepseek.api_key = api_key.to_string(),
    }
    save(&creds)?;
    Ok(creds)
}

pub fn provider_configured(creds: &CredentialsFile, id: ProviderId) -> bool {
    match id {
        ProviderId::Cursor => !creds.cursor.api_key.is_empty(),
        ProviderId::Gemini => {
            !creds.gemini.api_key.is_empty() || !creds.gemini.credentials_json.is_empty()
        }
        ProviderId::Claude => !creds.claude.api_key.is_empty(),
        ProviderId::OpenAi => !creds.openai.api_key.is_empty(),
        ProviderId::Groq => !creds.groq.api_key.is_empty(),
        ProviderId::Mistral => !creds.mistral.api_key.is_empty(),
        ProviderId::DeepSeek => !creds.deepseek.api_key.is_empty(),
    }
}

pub fn status_list(creds: &CredentialsFile) -> Vec<ProviderStatus> {
    ProviderId::all()
        .into_iter()
        .map(|id| ProviderStatus {
            configured: provider_configured(creds, id),
            label: id.label().into(),
            id,
        })
        .collect()
}
