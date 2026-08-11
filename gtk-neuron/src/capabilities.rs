//! Helpers for declaring capability surfaces.

use serde_json::{json, Value};

use crate::protocol::{CapabilitySpec, ProviderId};

pub fn spec(name: &str, description: &str, schema: Value, requires_confirm: bool) -> CapabilitySpec {
    CapabilitySpec {
        name: name.to_string(),
        description: description.to_string(),
        schema,
        requires_confirm,
    }
}

pub fn files_capabilities() -> Vec<CapabilitySpec> {
    vec![
        spec(
            "/list-dir",
            "List entries in a directory",
            json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            false,
        ),
        spec(
            "/move-files",
            "Move files into a destination directory",
            json!({"type":"object","properties":{"paths":{"type":"array","items":{"type":"string"}},"dest":{"type":"string"}},"required":["paths","dest"]}),
            true,
        ),
        spec(
            "/rename-files",
            "Rename a single file or folder",
            json!({"type":"object","properties":{"path":{"type":"string"},"new_name":{"type":"string"}},"required":["path","new_name"]}),
            true,
        ),
        spec(
            "/copy-files",
            "Copy files into a destination directory",
            json!({"type":"object","properties":{"paths":{"type":"array","items":{"type":"string"}},"dest":{"type":"string"}},"required":["paths","dest"]}),
            false,
        ),
        spec(
            "/trash-files",
            "Move paths to the trash",
            json!({"type":"object","properties":{"paths":{"type":"array","items":{"type":"string"}}},"required":["paths"]}),
            true,
        ),
        spec(
            "/create-folder",
            "Create a folder under a parent directory",
            json!({"type":"object","properties":{"parent":{"type":"string"},"name":{"type":"string"}},"required":["parent","name"]}),
            false,
        ),
        spec(
            "/open-path",
            "Open or navigate to a path",
            json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            false,
        ),
        spec(
            "/get-selection",
            "Return currently selected paths in the file manager",
            json!({"type":"object","properties":{}}),
            false,
        ),
    ]
}

pub fn term_capabilities() -> Vec<CapabilitySpec> {
    vec![
        spec(
            "/run-term-command",
            "Feed a command into the active terminal (appends newline)",
            json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}),
            true,
        ),
        spec(
            "/read-terminal-output",
            "Best-effort note that VTE text capture is limited; returns tab title",
            json!({"type":"object","properties":{}}),
            false,
        ),
        spec(
            "/write-terminal",
            "Write raw text to the active terminal without adding a newline",
            json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
            true,
        ),
        spec(
            "/list-tabs",
            "List terminal tabs",
            json!({"type":"object","properties":{}}),
            false,
        ),
        spec(
            "/new-tab",
            "Open a new terminal tab",
            json!({"type":"object","properties":{}}),
            false,
        ),
    ]
}

pub fn image_capabilities() -> Vec<CapabilitySpec> {
    vec![
        spec(
            "/open-image",
            "Open an image or directory of images",
            json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            false,
        ),
        spec(
            "/get-current-image",
            "Return the current image path",
            json!({"type":"object","properties":{}}),
            false,
        ),
        spec(
            "/rotate",
            "Rotate the current image (cw or ccw)",
            json!({"type":"object","properties":{"direction":{"type":"string","enum":["cw","ccw"]}},"required":["direction"]}),
            false,
        ),
        spec(
            "/flip",
            "Flip the current image",
            json!({"type":"object","properties":{"axis":{"type":"string","enum":["horizontal","vertical"]}},"required":["axis"]}),
            false,
        ),
        spec(
            "/save-image",
            "Save the current (possibly edited) image to a path",
            json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            true,
        ),
        spec(
            "/edit-image",
            "Apply a named edit op (rotate/flip aliases)",
            json!({"type":"object","properties":{"op":{"type":"string"}},"required":["op"]}),
            false,
        ),
    ]
}

pub fn edit_capabilities() -> Vec<CapabilitySpec> {
    vec![
        spec(
            "/open-file",
            "Open a file in the editor",
            json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            false,
        ),
        spec(
            "/list-tabs",
            "List open editor tabs",
            json!({"type":"object","properties":{}}),
            false,
        ),
        spec(
            "/read-buffer",
            "Read the current (or named) buffer text",
            json!({"type":"object","properties":{"path":{"type":"string"}}}),
            false,
        ),
        spec(
            "/write-buffer",
            "Replace the entire current buffer text",
            json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
            true,
        ),
        spec(
            "/replace-selection",
            "Replace the current selection with text",
            json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
            true,
        ),
        spec(
            "/save-file",
            "Save the current tab",
            json!({"type":"object","properties":{}}),
            false,
        ),
        spec(
            "/find",
            "Find text in the current buffer",
            json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
            false,
        ),
    ]
}

/// Short, user-facing examples for what to ask in Self Driving.
pub fn starter_help(app_id: &str) -> &'static str {
    match app_id {
        "org.neuronix.GtkFiles" => {
            "Try asking to list a folder, open a path, move or copy files, rename something, create a folder, trash items, or check what’s selected."
        }
        "org.neuronix.GtkTerm" => {
            "Try asking to run a command, open a new tab, list tabs, or type text into the active terminal."
        }
        "org.neuronix.GtkImage" => {
            "Try asking to open an image, rotate or flip it, save a copy, or tell you which image is open."
        }
        "org.neuronix.GtkEdit" => {
            "Try asking to open a file, list tabs, read or rewrite the buffer, replace the selection, find text, or save."
        }
        _ => "Ask the driver to use this app’s capability API for you.",
    }
}

pub fn app_display_name(app_id: &str) -> &'static str {
    match app_id {
        "org.neuronix.GtkFiles" => "GTK Files",
        "org.neuronix.GtkTerm" => "GTK Term",
        "org.neuronix.GtkImage" => "GTK Image",
        "org.neuronix.GtkEdit" => "GTK Edit",
        _ => "this app",
    }
}

/// Structured connect instructions for the Help dialog.
pub struct ProviderConnectHelp {
    pub title: &'static str,
    pub intro: &'static str,
    pub link_label: &'static str,
    pub link_url: &'static str,
    pub steps: &'static [&'static str],
    pub notes: &'static [&'static str],
}

pub fn provider_connect_help(provider: ProviderId) -> ProviderConnectHelp {
    match provider {
        ProviderId::Cursor => ProviderConnectHelp {
            title: "Connect Cursor",
            intro: "Use a Cursor User API Key from the cloud-agents dashboard — not an OpenAI or Anthropic sk-… key.",
            link_label: "Open Cursor cloud-agents dashboard",
            link_url: "https://cursor.com/dashboard/cloud-agents",
            steps: &[
                "Create a User API Key on the dashboard linked above.",
                "In this tab, click Edit, paste the key, then Save.",
                "Ensure gtk-neuron/python/.venv includes the cursor-sdk package (used by cursor_worker.py).",
            ],
            notes: &[
                "Or set CURSOR_API_KEY, or put api_key under [cursor] in ~/.config/gtk-apps/gtk-neuron/credentials.toml.",
            ],
        },
        ProviderId::Gemini => ProviderConnectHelp {
            title: "Connect Gemini",
            intro: "Use a Google AI Studio API key. The default model is gemini-flash-latest.",
            link_label: "Open Google AI Studio API keys",
            link_url: "https://aistudio.google.com/apikey",
            steps: &[
                "Create an API key in Google AI Studio.",
                "In this tab, click Edit, paste the key, then Save.",
                "Optional: set [gemini] model in credentials.toml to override the default.",
            ],
            notes: &[
                "Or set GEMINI_API_KEY, or put api_key under [gemini] in ~/.config/gtk-apps/gtk-neuron/credentials.toml.",
            ],
        },
        ProviderId::Claude => ProviderConnectHelp {
            title: "Connect Claude",
            intro: "Use an Anthropic Messages API key from the Claude console.",
            link_label: "Open Anthropic API keys",
            link_url: "https://console.anthropic.com/settings/keys",
            steps: &[
                "Create an Anthropic API key in the console.",
                "In this tab, click Edit, paste the key, then Save.",
                "Optional: set [claude] model in credentials.toml (default is Claude Sonnet).",
            ],
            notes: &[
                "Or set ANTHROPIC_API_KEY / CLAUDE_API_KEY, or put api_key under [claude] in ~/.config/gtk-apps/gtk-neuron/credentials.toml.",
            ],
        },
        ProviderId::OpenAi => ProviderConnectHelp {
            title: "Connect OpenAI",
            intro: "Use an OpenAI API key. Default model is gpt-4o-mini.",
            link_label: "Open OpenAI API keys",
            link_url: "https://platform.openai.com/api-keys",
            steps: &[
                "Create an API key in the OpenAI platform.",
                "In this tab, click Edit, paste the key, then Save.",
                "Optional: set [openai] model in credentials.toml (e.g. gpt-4o).",
            ],
            notes: &[
                "Or set OPENAI_API_KEY, or put api_key under [openai] in ~/.config/gtk-apps/gtk-neuron/credentials.toml.",
            ],
        },
        ProviderId::Groq => ProviderConnectHelp {
            title: "Connect Groq",
            intro: "Use a GroqCloud API key for fast open models. Default model is llama-3.3-70b-versatile.",
            link_label: "Open Groq console API keys",
            link_url: "https://console.groq.com/keys",
            steps: &[
                "Create an API key in the Groq console.",
                "In this tab, click Edit, paste the key, then Save.",
                "Optional: set [groq] model in credentials.toml.",
            ],
            notes: &[
                "Or set GROQ_API_KEY, or put api_key under [groq] in ~/.config/gtk-apps/gtk-neuron/credentials.toml.",
            ],
        },
        ProviderId::Mistral => ProviderConnectHelp {
            title: "Connect Mistral",
            intro: "Use a Mistral AI API key. Default model is mistral-small-latest.",
            link_label: "Open Mistral console API keys",
            link_url: "https://console.mistral.ai/api-keys",
            steps: &[
                "Create an API key in the Mistral console.",
                "In this tab, click Edit, paste the key, then Save.",
                "Optional: set [mistral] model in credentials.toml.",
            ],
            notes: &[
                "Or set MISTRAL_API_KEY, or put api_key under [mistral] in ~/.config/gtk-apps/gtk-neuron/credentials.toml.",
            ],
        },
        ProviderId::DeepSeek => ProviderConnectHelp {
            title: "Connect DeepSeek",
            intro: "Use a DeepSeek API key. Default model is deepseek-chat.",
            link_label: "Open DeepSeek platform API keys",
            link_url: "https://platform.deepseek.com/api_keys",
            steps: &[
                "Create an API key on the DeepSeek platform.",
                "In this tab, click Edit, paste the key, then Save.",
                "Optional: set [deepseek] model in credentials.toml.",
            ],
            notes: &[
                "Or set DEEPSEEK_API_KEY, or put api_key under [deepseek] in ~/.config/gtk-apps/gtk-neuron/credentials.toml.",
            ],
        },
    }
}

/// Build a tool description block for provider prompts.
pub fn tools_prompt(caps: &[CapabilitySpec]) -> String {
    let mut out = String::from(
        "You are driving a Neuronix GTK desktop app. When you need to act, reply with a single JSON object on its own line in this exact form:\n\
         {\"tool\":\"/capability-name\",\"args\":{...}}\n\
         Otherwise reply with normal helpful text. Available tools:\n",
    );
    for c in caps {
        out.push_str(&format!(
            "- {} : {}{}\n",
            c.name,
            c.description,
            if c.requires_confirm {
                " [requires user confirm]"
            } else {
                ""
            }
        ));
    }
    out
}
