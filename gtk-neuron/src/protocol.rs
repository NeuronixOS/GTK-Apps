//! JSON-RPC style messages over length-prefixed Unix socket frames.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

/// Advertised tool the host app allows the AI to call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitySpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema-ish object describing args (free-form for v1).
    #[serde(default)]
    pub schema: Value,
    /// If true, Driving UI must Confirm before the daemon invokes it.
    #[serde(default)]
    pub requires_confirm: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Cursor,
    Gemini,
    Claude,
    OpenAi,
    Groq,
    Mistral,
    DeepSeek,
}

impl ProviderId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cursor => "cursor",
            Self::Gemini => "gemini",
            Self::Claude => "claude",
            Self::OpenAi => "openai",
            Self::Groq => "groq",
            Self::Mistral => "mistral",
            Self::DeepSeek => "deepseek",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cursor => "Cursor",
            Self::Gemini => "Gemini",
            Self::Claude => "Claude",
            Self::OpenAi => "OpenAI",
            Self::Groq => "Groq",
            Self::Mistral => "Mistral",
            Self::DeepSeek => "DeepSeek",
        }
    }

    pub fn key_placeholder(self) -> &'static str {
        match self {
            Self::Cursor => "Cursor User API Key (dashboard/cloud-agents)",
            Self::Gemini => "Gemini API key (AI Studio)",
            Self::Claude => "Anthropic API key",
            Self::OpenAi => "OpenAI API key (platform.openai.com)",
            Self::Groq => "Groq API key (console.groq.com)",
            Self::Mistral => "Mistral API key (console.mistral.ai)",
            Self::DeepSeek => "DeepSeek API key (platform.deepseek.com)",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "cursor" => Some(Self::Cursor),
            "gemini" => Some(Self::Gemini),
            "claude" => Some(Self::Claude),
            "openai" => Some(Self::OpenAi),
            "groq" => Some(Self::Groq),
            "mistral" => Some(Self::Mistral),
            "deepseek" => Some(Self::DeepSeek),
            _ => None,
        }
    }

    pub fn all() -> [Self; 7] {
        [
            Self::Cursor,
            Self::Gemini,
            Self::Claude,
            Self::OpenAi,
            Self::Groq,
            Self::Mistral,
            Self::DeepSeek,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub id: ProviderId,
    pub configured: bool,
    pub label: String,
}

/// Envelope: every frame is one of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Message {
    // --- App → daemon ---
    #[serde(rename = "hello")]
    Hello(HelloParams),
    #[serde(rename = "chat.start")]
    ChatStart(ChatStartParams),
    #[serde(rename = "chat.send")]
    ChatSend(ChatSendParams),
    #[serde(rename = "chat.cancel")]
    ChatCancel(ChatCancelParams),
    #[serde(rename = "credentials.set")]
    CredentialsSet(CredentialsSetParams),
    #[serde(rename = "credentials.status")]
    CredentialsStatus,
    #[serde(rename = "providers.list")]
    ProvidersList,
    #[serde(rename = "capability.result")]
    CapabilityResult(CapabilityResultParams),
    #[serde(rename = "capability.error")]
    CapabilityError(CapabilityErrorParams),
    #[serde(rename = "capability.confirm")]
    CapabilityConfirm(CapabilityConfirmParams),

    // --- Daemon → app ---
    #[serde(rename = "hello.ok")]
    HelloOk(HelloOkParams),
    #[serde(rename = "providers.status")]
    ProvidersStatus(ProvidersStatusParams),
    #[serde(rename = "chat.delta")]
    ChatDelta(ChatDeltaParams),
    #[serde(rename = "chat.done")]
    ChatDone(ChatDoneParams),
    #[serde(rename = "chat.error")]
    ChatError(ChatErrorParams),
    #[serde(rename = "capability.invoke")]
    CapabilityInvoke(CapabilityInvokeParams),
    #[serde(rename = "capability.propose")]
    CapabilityPropose(CapabilityProposeParams),
    #[serde(rename = "error")]
    Error(ErrorParams),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloParams {
    pub app_id: String,
    pub protocol: u32,
    pub capabilities: Vec<CapabilitySpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloOkParams {
    pub daemon_version: String,
    pub protocol: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatStartParams {
    pub session_id: String,
    pub provider: ProviderId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSendParams {
    pub session_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCancelParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialsSetParams {
    pub provider: ProviderId,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersStatusParams {
    pub providers: Vec<ProviderStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatDeltaParams {
    pub session_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatDoneParams {
    pub session_id: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatErrorParams {
    pub session_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityInvokeParams {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityProposeParams {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub args: Value,
    pub requires_confirm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityConfirmParams {
    pub id: String,
    pub allow: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityResultParams {
    pub id: String,
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityErrorParams {
    pub id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorParams {
    pub message: String,
}
