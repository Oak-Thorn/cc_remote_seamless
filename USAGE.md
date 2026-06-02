# Usage Guide

## Settings Page

Right-click the floating icon or use the system tray menu to open Settings. The settings page has four tabs:

### Floating Icon

Choose the desktop floating icon style. The icon shows different colors based on agent status:

- **Original** — no active session
- **Green** — agent is running
- **Orange** — waiting for permission approval
- **Red** — error or disconnected

Custom icons: place `.svg` files in `~/.cc-remote/icons/`, then click **Load** to refresh.

### Sound

Configure notification sounds for two events:

| Event | Description | Default |
|-------|-------------|---------|
| Task Complete (Idle) | Plays when the agent finishes a task | Glass |
| Permission Request | Plays when a tool permission approval is needed | Hero |

Custom sounds: place audio files in `~/.cc-remote/sounds/`, then click **Load** to refresh.

### Config

The config tab provides guided setup for Feishu and Telegram platforms.

Config file location: `~/.cc-remote/config.toml`

#### Full Config Example

```toml
[general]
hook_port = 23399       # Hook server port (default: 23399)

[general.sounds]
idle = "Glass"          # Sound on task complete
permission = "Hero"     # Sound on permission request

# --- Feishu Platform ---
[platforms.feishu-main]
type = "feishu"
app_id = "cli_xxxxxxxxx"
app_secret = "xxxxxxxxxxxxxxxxxxxxxxxx"
chat_id = "oc_xxxxxxxxxxxxxxxxxxxxxxxx"

# --- Telegram Platform ---
[platforms.telegram]
type = "telegram"
bot_token = "123456:ABC-DEF..."
chat_id = "123456789"

# --- Agent Routing ---
# Each agent specifies which platforms receive its messages
[agents.claude-code]
platforms = ["feishu-main", "telegram"]

[agents.pi]
platforms = ["feishu-main"]
```

#### Feishu Setup

1. **Create App** — In the Settings > Config tab, click "Scan to Create" and scan the QR code with Feishu. This auto-creates a bot app and saves `app_id` / `app_secret` to your config.
2. **Get chat_id** — From the Feishu App's agent settings page, copy the `chat_id` (format: `oc_xxxx`).
3. **Edit Config** — Fill in the `chat_id` field in `~/.cc-remote/config.toml`.

#### Telegram Setup

1. **Create Bot** — Send `/newbot` to [@BotFather](https://t.me/BotFather) on Telegram, follow the prompts, and copy the `bot_token`.
2. **Get chat_id** — Send any message to your bot, then visit `https://api.telegram.org/bot<TOKEN>/getUpdates` and find `chat.id` in the response.
3. **Edit Config** — Add the `[platforms.telegram]` section with `bot_token` and `chat_id`.

### About

Displays app version and basic info.
