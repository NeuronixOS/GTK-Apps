# gtk-neuron

AI Self Driving layer for Neuronix GTK apps. A local daemon (`gtk-neurond`) holds Cursor / Gemini / Claude credentials; each app shows a **ꔮ Self Driving** header control and registers a capability API the model may call.

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

## Pieces

| Piece | Role |
|-------|------|
| `gtk-neuron` (Rust lib) | Protocol, Unix-socket client, Self Driving panel, capability helpers |
| `gtk-neurond` (bin) | Daemon on `$XDG_RUNTIME_DIR/gtk-neuron.sock` |
| `python/cursor_worker.py` | Cursor SDK bridge (stdin/stdout JSON) |
| `examples/credentials.toml` | Template for API keys |

Integrated apps: **gtk-files**, **gtk-term**, **gtk-edit**, **gtk-image**, **gtk-video**.

## Build

```bash
cd gtk-neuron
cargo build --release
# binary: target/release/gtk-neurond
```

Apps that depend on this crate rebuild it automatically. Ensure `gtk-neurond` is on `PATH`, set `GTK_NEUROND` to the binary, or keep the release/debug binary under `gtk-neuron/target/` (the client looks there relative to each app binary).

## Credentials

Copy the template and fill keys (never commit real secrets):

```bash
mkdir -p ~/.config/gtk-apps/gtk-neuron
cp examples/credentials.toml ~/.config/gtk-apps/gtk-neuron/credentials.toml
```

| Provider | Config | Notes |
|----------|--------|--------|
| Cursor | `[cursor] api_key` or `CURSOR_API_KEY` | **User API Key** from [cursor.com/dashboard/cloud-agents](https://cursor.com/dashboard/cloud-agents) (not an OpenAI `sk-…` key). Also needs `python/.venv` with `cursor-sdk`. |
| Gemini | `[gemini] api_key` or `GEMINI_API_KEY` | Google AI Studio key; default model `gemini-flash-latest` |
| Claude | `[claude] api_key` or `ANTHROPIC_API_KEY` / `CLAUDE_API_KEY` | Anthropic Messages API |
| OpenAI | `[openai] api_key` or `OPENAI_API_KEY` | Chat Completions; default `gpt-4o-mini` |
| Groq | `[groq] api_key` or `GROQ_API_KEY` | OpenAI-compatible; default `llama-3.3-70b-versatile` |
| Mistral | `[mistral] api_key` or `MISTRAL_API_KEY` | OpenAI-compatible; default `mistral-small-latest` |
| DeepSeek | `[deepseek] api_key` or `DEEPSEEK_API_KEY` | OpenAI-compatible; default `deepseek-chat` |

You can also paste a key in the Self Driving panel **Edit** field.

## Usage

1. Open gtk-files / gtk-term / gtk-edit / gtk-image / gtk-video.
2. Click the steering-wheel / **ꔮ** button (or Ctrl+D) to open **Self Driving** (right-hand side panel).
3. Choose Cursor, Gemini, or Claude; connect a key if needed.
4. Ask the driver to act (e.g. “list this folder”, “run `ls`”, “rotate the image”).
5. Tools marked as confirming (move/trash/term commands/buffer writes) show **Allow** / **Deny**.

## Capability APIs (per app)

**gtk-files:** `/list-dir`, `/move-files`, `/rename-files`, `/copy-files`, `/trash-files`, `/create-folder`, `/open-path`, `/get-selection`

**gtk-term:** `/run-term-command`, `/read-terminal-output`, `/write-terminal`, `/list-tabs`, `/new-tab`

**gtk-image:** `/open-image`, `/get-current-image`, `/rotate`, `/flip`, `/save-image`, `/edit-image`

**gtk-video:** `/open-video`, `/get-current-video`, `/play-pause`, `/seek`, `/set-in`, `/set-out`, `/set-range`, `/rotate`, `/flip`, `/crop`, `/export`

**gtk-edit:** `/open-file`, `/list-tabs`, `/read-buffer`, `/write-buffer`, `/replace-selection`, `/save-file`, `/find`

Models request tools with a JSON line:

```json
{"tool":"/list-dir","args":{"path":"/home/you"}}
```

## Protocol

Length-prefixed JSON frames on a Unix socket. See `src/protocol.rs` for `hello`, `chat.*`, `credentials.*`, `capability.*` messages.
