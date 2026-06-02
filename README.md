<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" alt="CC Remote Seamless">
</p>
<h1 align="center">CC Remote Seamless</h1>
<p align="center">
  <a href="README.zh-CN.md">中文</a>
</p>
<p align="center">
  <a href="https://github.com/Oak-Thorn/cc_remote_seamless/releases"><img src="https://img.shields.io/github/v/release/Oak-Thorn/cc_remote_seamless" alt="Version"></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey" alt="Platform">
  <a href="LICENSE"><img src="https://img.shields.io/github/license/Oak-Thorn/cc_remote_seamless" alt="License"></a>
</p>

<p align="center">
  <img src="pictures/claudecode_feishu.png" width="320" alt="Claude Code Feishu">
  &nbsp;&nbsp;
  <img src="pictures/claudecode_telegram.png" width="320" alt="Claude Code Telegram">
</p>
<p align="center">
  <img src="pictures/feishu.jpg" width="320" alt="Feishu">
  &nbsp;&nbsp;
  <img src="pictures/telegram.jpg" width="320" alt="Telegram">
</p>

Remote control your running Claude Code / Pi Agent CLI sessions via Feishu or Telegram. Walk away from your computer and keep interacting with your Agent from your phone.

## Features

- **Seamless terminal experience** — desktop CLI keeps full ANSI formatting, colors, and interactivity; remote control has zero impact
- **Multi-platform IM** — supports Feishu (WebSocket) and Telegram (Bot API) simultaneously
- **Multi-Agent** — Claude Code and Pi Agent, switch with `/change`
- **Slash commands** — `/status`, `/list`, `/switch`, `/mute`, `/p`, `/allow`, `/answer`, etc.
- **Remote permission approval** — tool permission requests pushed to IM, respond with `/allow`, `/deny`, `/always`
- **Remote Q&A** — AskUserQuestion pushed as cards, reply with `/answer N`
- **Desktop floating widget** — always-on-top icon with status color, expandable session list
- **Sound alerts** — configurable sounds on idle and permission events
- **Auto hook installation** — Claude Code hooks configured automatically on startup
- **No public IP needed** — Feishu WebSocket + Telegram long polling, client-initiated connections

## Quick Start

### Prerequisites

- **Rust** toolchain (stable)
- **Node.js** 20+
- **Go** 1.22+

### Build from Source

```bash
# 1. Clone the repository
git clone https://github.com/Oak-Thorn/cc_remote_seamless.git
cd cc_remote_seamless

# 2. Install frontend dependencies
npm ci

# 3. Build the sidecar (Feishu gateway)
cd sidecar/feishu-gateway
go build -o feishu-gateway-aarch64-apple-darwin .   # macOS Apple Silicon
# go build -o feishu-gateway-x86_64-apple-darwin .  # macOS Intel
# GOOS=windows GOARCH=amd64 go build -o feishu-gateway-x86_64-pc-windows-msvc.exe .  # Windows
cd ../..

# 4. Build the Tauri app
npm run tauri build
```

### Build Outputs

| Platform | Path |
|----------|------|
| macOS .app | `target/release/bundle/macos/CC Remote Seamless.app` |
| macOS .dmg | `target/release/bundle/dmg/CC Remote Seamless_<ver>_aarch64.dmg` |
| Windows .msi | `target/release/bundle/msi/*.msi` |
| Windows .exe | `target/release/bundle/nsis/*.exe` |

### Development Mode

```bash
npm run tauri dev
```

## Configuration

Create `~/.cc-remote/config.toml`:

```toml
[general]
hook_port = 23399

[platforms.feishu-main]
type = "feishu"
app_id = "cli_xxxxxxxxx"
app_secret = "xxxxxxxxxxxxxxxxxxxxxxxx"
chat_id = "oc_xxxxxxxxxxxxxxxxxxxxxxxx"

[agents.claude-code]
platforms = ["feishu-main"]
```

## Slash Commands

| Command | Description |
|---------|-------------|
| `/status` | View current session status |
| `/list` | List all active sessions |
| `/switch <id>` | Switch to specified session |
| `/change [agent]` | Switch agent type |
| `/p <text>` | Send prompt |
| `/mute` / `/unmute` | Mute/unmute message push |
| `/allow` / `/deny` / `/always` | Permission approval |
| `/answer <n>` | Answer a question |
| `/radar` | Re-discover running agents |
| `/stop` | Stop session |
| `/help` | Show help |

## Architecture

```
┌─────────────────────────────────────────────────────┐
│  CC Remote Seamless (Tauri 2 + Vue 3 + Rust)        │
│                                                     │
│  ┌─────────┐  ┌────────┐  ┌───────────────────┐    │
│  │ Float UI│  │ Engine │  │ Hook Server :23399│    │
│  │ (Widget)│←→│(Router)│←─│                   │←── Claude Code / Pi hooks
│  └─────────┘  └────────┘  └───────────────────┘    │
│                     ↕                               │
│  ┌─────────────────────────────────────────────┐    │
│  │ Agents: ClaudeCode | Pi                     │    │
│  └─────────────────────────────────────────────┘    │
│                     ↕                               │
│  ┌─────────────────────────────────────────────┐    │
│  │ Platforms: Feishu (Go sidecar, WebSocket)   │    │
│  │            Telegram (reqwest, long poll)     │    │
│  └─────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────┘
```

## CI/CD

GitHub Actions automatically builds for macOS (aarch64 + x86_64) and Windows (x86_64) on tag push or manual trigger. See `.github/workflows/build.yml`.

## License

[MIT](LICENSE)
