# CC Remote Seamless V2 — Tauri Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a cross-platform Tauri 2 desktop app that bridges Claude Code CLI sessions to Feishu mobile, enabling seamless session switching without disrupting the terminal experience.

**Architecture:** Tauri 2 Rust core manages an Engine that routes messages between AgentConnector(s) (PTY Proxy) and IMPlatform(s) (Go Sidecar for Feishu). A Hook HTTP server receives Claude Code lifecycle events for state tracking. Vue 3 frontend renders floating widget, main window, and permission popups.

**Tech Stack:** Tauri 2, Rust (portable-pty, axum, tokio), Go (oapi-sdk-go/v3), Vue 3 + TypeScript + Vite, SQLite (rusqlite)

---

## File Structure

```
cc-remote-seamless/
├── src-tauri/                          # Tauri Rust backend
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── src/
│   │   ├── main.rs                     # Tauri entry point
│   │   ├── lib.rs                      # Library root (re-exports)
│   │   ├── engine/
│   │   │   ├── mod.rs                  # Engine struct, message routing
│   │   │   ├── router.rs              # BindingStore: chat_id <-> (agent, session)
│   │   │   └── store.rs              # MessageStore: SQLite persistence
│   │   ├── agent/
│   │   │   ├── mod.rs                 # AgentConnector trait definition
│   │   │   └── claude_code.rs         # ClaudeCodeConnector implementation
│   │   ├── platform/
│   │   │   ├── mod.rs                 # IMPlatform trait definition
│   │   │   └── feishu.rs             # FeishuPlatform (Go sidecar communication)
│   │   ├── hook/
│   │   │   └── server.rs             # HTTP server for Claude Code hooks
│   │   ├── pty/
│   │   │   ├── mod.rs                # PTY proxy management
│   │   │   ├── proxy.rs              # Single PTY session (spawn, inject, subscribe)
│   │   │   └── protocol.rs           # JSON Lines IPC protocol types
│   │   ├── window/
│   │   │   └── mod.rs                # Window manager (float, main, popup)
│   │   └── commands.rs               # Tauri IPC commands (frontend <-> backend)
│   └── icons/                         # App icons
├── src/                                # Vue 3 frontend
│   ├── main.ts                        # Vue app entry
│   ├── App.vue                        # Root component with router
│   ├── views/
│   │   ├── FloatingWidget.vue         # Always-on-top floating status
│   │   ├── MainWindow.vue             # Full message view
│   │   └── PermissionPopup.vue        # Permission confirmation
│   ├── components/
│   │   ├── SessionList.vue            # Left panel session list
│   │   ├── MessageFlow.vue            # Right panel message stream
│   │   └── StatusBadge.vue            # Session state indicator
│   ├── stores/
│   │   ├── sessions.ts               # Pinia store: session state
│   │   └── messages.ts               # Pinia store: message history
│   ├── types.ts                       # Shared TypeScript types
│   └── style.css                      # Global styles
├── crates/
│   └── cc-remote-pty/                 # Standalone PTY proxy binary
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs               # CLI entry: wraps agent process
│           ├── pty.rs                # portable-pty spawn + IO
│           ├── ipc.rs                # Unix Socket / Named Pipe server
│           └── protocol.rs           # JSON Lines encode/decode
├── sidecar/
│   └── feishu-gateway/               # Go sidecar binary
│       ├── go.mod
│       ├── go.sum
│       ├── main.go                   # Entry: stdio JSON Lines <-> Feishu WS
│       ├── client.go                 # Feishu SDK client wrapper
│       └── protocol.go              # JSON Lines message types
├── package.json                       # Frontend dependencies
├── vite.config.ts                     # Vite config for Tauri
├── tsconfig.json                      # TypeScript config
├── index.html                         # Vite HTML entry
└── docs/
    └── superpowers/
        └── specs/
            └── 2026-05-23-cc-remote-tauri-design.md
```

---

## Task 1: Tauri 2 Project Scaffolding

**Files:**
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/build.rs`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/lib.rs`
- Create: `package.json`
- Create: `vite.config.ts`
- Create: `tsconfig.json`
- Create: `index.html`
- Create: `src/main.ts`
- Create: `src/App.vue`
- Create: `src/style.css`

- [ ] **Step 1: Initialize Tauri 2 project with cargo**

Run:
```bash
cd /Users/yanbin07/work/project/open_source/cc_remote_seamless
cargo init src-tauri --name cc-remote-seamless
```

- [ ] **Step 2: Write src-tauri/Cargo.toml**

```toml
[package]
name = "cc-remote-seamless"
version = "0.1.0"
edition = "2021"

[dependencies]
tauri = { version = "2", features = ["tray-icon"] }
tauri-plugin-shell = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
axum = "0.7"
rusqlite = { version = "0.31", features = ["bundled"] }
portable-pty = "0.8"
uuid = { version = "1", features = ["v4"] }
tracing = "0.1"
tracing-subscriber = "0.3"
directories = "5"
toml = "0.8"
async-trait = "0.1"
chrono = "0.4"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[lib]
name = "cc_remote_seamless_lib"
crate-type = ["lib", "cdylib", "staticlib"]
```

- [ ] **Step 3: Write src-tauri/build.rs**

```rust
fn main() {
    tauri_build::build()
}
```

- [ ] **Step 4: Write src-tauri/tauri.conf.json**

```json
{
  "$schema": "https://raw.githubusercontent.com/nicedoc/tauri/main/packages/api/schema.json",
  "productName": "CC Remote Seamless",
  "version": "0.1.0",
  "identifier": "com.cc-remote.seamless",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:1420",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "windows": [
      {
        "label": "float",
        "title": "CC Remote",
        "width": 220,
        "height": 70,
        "decorations": false,
        "alwaysOnTop": true,
        "resizable": false,
        "transparent": true,
        "skipTaskbar": true
      }
    ],
    "security": {
      "csp": null
    }
  },
  "plugins": {
    "shell": {
      "sidecar": true
    }
  }
}
```

- [ ] **Step 5: Write src-tauri/src/main.rs**

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    cc_remote_seamless_lib::run();
}
```

- [ ] **Step 6: Write src-tauri/src/lib.rs**

```rust
mod commands;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_sessions,
            commands::get_messages,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 7: Write src-tauri/src/commands.rs (stub)**

```rust
use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub agent: String,
    pub state: String,
    pub working_dir: Option<String>,
}

#[tauri::command]
pub fn get_sessions() -> Vec<SessionInfo> {
    vec![]
}

#[tauri::command]
pub fn get_messages(_session_id: String) -> Vec<String> {
    vec![]
}
```

- [ ] **Step 8: Write package.json (frontend)**

```json
{
  "name": "cc-remote-seamless",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc --noEmit && vite build",
    "preview": "vite preview",
    "tauri": "tauri"
  },
  "dependencies": {
    "vue": "^3.4",
    "pinia": "^2.1",
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-shell": "^2"
  },
  "devDependencies": {
    "@vitejs/plugin-vue": "^5",
    "typescript": "^5.4",
    "vite": "^5",
    "vue-tsc": "^2"
  }
}
```

- [ ] **Step 9: Write vite.config.ts**

```typescript
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
});
```

- [ ] **Step 10: Write tsconfig.json**

```json
{
  "compilerOptions": {
    "target": "ES2021",
    "useDefineForClassFields": true,
    "module": "ESNext",
    "lib": ["ES2021", "DOM", "DOM.Iterable"],
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "preserve",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true
  },
  "include": ["src/**/*.ts", "src/**/*.vue"],
  "references": [{ "path": "./tsconfig.node.json" }]
}
```

- [ ] **Step 11: Write index.html**

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>CC Remote Seamless</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

- [ ] **Step 12: Write src/main.ts**

```typescript
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import "./style.css";

const app = createApp(App);
app.use(createPinia());
app.mount("#app");
```

- [ ] **Step 13: Write src/App.vue**

```vue
<script setup lang="ts">
</script>

<template>
  <div class="app">
    <p>CC Remote Seamless</p>
  </div>
</template>

<style scoped>
.app {
  font-family: system-ui, sans-serif;
  color: #eee;
  background: #1a1a2e;
  height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
}
</style>
```

- [ ] **Step 14: Write src/style.css**

```css
:root {
  --bg-primary: #1a1a2e;
  --bg-secondary: #1e293b;
  --bg-hover: #2a2a4e;
  --text-primary: #eee;
  --text-secondary: #888;
  --accent-green: #4ade80;
  --accent-yellow: #facc15;
  --accent-red: #ef4444;
  --border: #333;
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body {
  background: var(--bg-primary);
  color: var(--text-primary);
  font-family: system-ui, -apple-system, sans-serif;
  font-size: 13px;
  overflow: hidden;
}
```

- [ ] **Step 15: Verify build compiles**

Run:
```bash
cd src-tauri && cargo check
```
Expected: Compilation succeeds (warnings OK).

- [ ] **Step 16: Commit**

```bash
git add src-tauri/ src/ package.json vite.config.ts tsconfig.json index.html
git commit -m "feat: scaffold Tauri 2 project with Vue 3 frontend"
```

---

## Task 2: PTY Proxy Crate (cc-remote-pty)

**Files:**
- Create: `crates/cc-remote-pty/Cargo.toml`
- Create: `crates/cc-remote-pty/src/main.rs`
- Create: `crates/cc-remote-pty/src/protocol.rs`
- Create: `crates/cc-remote-pty/src/pty.rs`
- Create: `crates/cc-remote-pty/src/ipc.rs`

- [ ] **Step 1: Write crates/cc-remote-pty/Cargo.toml**

```toml
[package]
name = "cc-remote-pty"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "cc-remote-pty"
path = "src/main.rs"

[dependencies]
portable-pty = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
uuid = { version = "1", features = ["v4"] }
tracing = "0.1"
tracing-subscriber = "0.3"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Write protocol.rs — JSON Lines message types**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PtyMessage {
    #[serde(rename = "input")]
    Input { data: String },
    #[serde(rename = "output")]
    Output { data: String },
    #[serde(rename = "resize")]
    Resize { cols: u16, rows: u16 },
    #[serde(rename = "state")]
    State { waiting: bool },
    #[serde(rename = "exit")]
    Exit { code: i32 },
}

pub fn encode(msg: &PtyMessage) -> String {
    let mut s = serde_json::to_string(msg).expect("serialize PtyMessage");
    s.push('\n');
    s
}

pub fn decode(line: &str) -> Option<PtyMessage> {
    serde_json::from_str(line.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_input() {
        let msg = PtyMessage::Input { data: "hello\n".into() };
        let encoded = encode(&msg);
        let decoded = decode(&encoded).unwrap();
        match decoded {
            PtyMessage::Input { data } => assert_eq!(data, "hello\n"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn roundtrip_state() {
        let msg = PtyMessage::State { waiting: true };
        let encoded = encode(&msg);
        let decoded = decode(&encoded).unwrap();
        match decoded {
            PtyMessage::State { waiting } => assert!(waiting),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn decode_invalid_returns_none() {
        assert!(decode("not json").is_none());
    }
}
```

- [ ] **Step 3: Run protocol tests**

Run: `cd crates/cc-remote-pty && cargo test protocol`
Expected: 3 tests pass.

- [ ] **Step 4: Write pty.rs — PTY spawn and IO management**

```rust
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::sync::mpsc;

pub struct PtySession {
    master_write: Arc<Mutex<Box<dyn Write + Send>>>,
    output_rx: mpsc::UnboundedReceiver<String>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtySession {
    pub fn spawn(cmd: &str, args: &[String]) -> Result<Self, Box<dyn std::error::Error>> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut command = CommandBuilder::new(cmd);
        for arg in args {
            command.arg(arg);
        }

        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let (output_tx, output_rx) = mpsc::unbounded_channel();

        thread::spawn(move || {
            let buf = BufReader::new(reader);
            for line in buf.lines() {
                match line {
                    Ok(text) => {
                        if output_tx.send(text).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            master_write: Arc::new(Mutex::new(writer)),
            output_rx,
            child,
        })
    }

    pub fn inject_input(&self, data: &str) -> Result<(), std::io::Error> {
        let mut writer = self.master_write.lock().unwrap();
        writer.write_all(data.as_bytes())?;
        writer.flush()
    }

    pub async fn next_output(&mut self) -> Option<String> {
        self.output_rx.recv().await
    }

    pub fn wait(&mut self) -> Option<i32> {
        self.child.wait().ok().map(|s| {
            if s.success() { 0 } else { 1 }
        })
    }
}
```

- [ ] **Step 5: Write ipc.rs — Unix Socket / Named Pipe IPC server**

```rust
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tracing::{info, error};

use crate::protocol::{self, PtyMessage};

pub fn socket_path(agent: &str, session_id: &str) -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from(format!("/tmp/cc-remote-{}-{}.sock", agent, session_id))
    }
    #[cfg(windows)]
    {
        PathBuf::from(format!(r"\\.\pipe\cc-remote-{}-{}", agent, session_id))
    }
}

pub struct IpcServer {
    input_rx: mpsc::UnboundedReceiver<String>,
    output_tx: mpsc::UnboundedSender<String>,
}

impl IpcServer {
    #[cfg(unix)]
    pub async fn listen(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        use tokio::net::UnixListener;

        if path.exists() {
            std::fs::remove_file(path)?;
        }

        let listener = UnixListener::bind(path)?;
        info!("IPC listening on {:?}", path);

        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (output_tx, output_rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            Self::accept_loop(listener, input_tx, output_rx).await;
        });

        Ok(Self { input_rx, output_tx })
    }

    #[cfg(unix)]
    async fn accept_loop(
        listener: tokio::net::UnixListener,
        input_tx: mpsc::UnboundedSender<String>,
        mut output_rx: mpsc::UnboundedReceiver<String>,
    ) {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let (reader, mut writer) = stream.into_split();
                    let mut buf_reader = BufReader::new(reader);
                    let tx = input_tx.clone();

                    let read_handle = tokio::spawn(async move {
                        let mut line = String::new();
                        loop {
                            line.clear();
                            match buf_reader.read_line(&mut line).await {
                                Ok(0) => break,
                                Ok(_) => {
                                    if let Some(msg) = protocol::decode(&line) {
                                        if let PtyMessage::Input { data } = msg {
                                            let _ = tx.send(data);
                                        }
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    });

                    let write_handle = tokio::spawn(async move {
                        while let Some(text) = output_rx.recv().await {
                            let msg = protocol::encode(&PtyMessage::Output { data: text });
                            if writer.write_all(msg.as_bytes()).await.is_err() {
                                break;
                            }
                        }
                    });

                    let _ = read_handle.await;
                    let _ = write_handle.await;
                }
                Err(e) => {
                    error!("IPC accept error: {}", e);
                    break;
                }
            }
        }
    }

    pub async fn next_input(&mut self) -> Option<String> {
        self.input_rx.recv().await
    }

    pub fn send_output(&self, text: String) {
        let _ = self.output_tx.send(text);
    }
}
```

- [ ] **Step 6: Write main.rs — CLI entry point**

```rust
mod protocol;
mod pty;
mod ipc;

use clap::Parser;
use tracing::info;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "cc-remote-pty")]
#[command(about = "PTY proxy for remote Agent interaction")]
struct Cli {
    #[arg(long, default_value = "claude")]
    agent: String,

    #[arg(long)]
    session_id: Option<String>,

    #[arg(last = true)]
    command: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let session_id = cli.session_id.unwrap_or_else(|| Uuid::new_v4().to_string()[..8].to_string());

    let (cmd, args) = if cli.command.is_empty() {
        ("claude".to_string(), vec![])
    } else {
        (cli.command[0].clone(), cli.command[1..].to_vec())
    };

    info!("Starting PTY proxy: agent={}, session={}, cmd={}", cli.agent, session_id, cmd);

    let mut pty_session = pty::PtySession::spawn(&cmd, &args)?;
    let sock_path = ipc::socket_path(&cli.agent, &session_id);
    let mut ipc = ipc::IpcServer::listen(&sock_path).await?;

    let idle_threshold = std::time::Duration::from_millis(500);
    let mut last_output = std::time::Instant::now();

    loop {
        tokio::select! {
            Some(input) = ipc.next_input() => {
                pty_session.inject_input(&input)?;
            }
            Some(output) = pty_session.next_output() => {
                last_output = std::time::Instant::now();
                ipc.send_output(output);
            }
            _ = tokio::time::sleep(idle_threshold) => {
                if last_output.elapsed() >= idle_threshold {
                    let state_msg = protocol::encode(&protocol::PtyMessage::State { waiting: true });
                    ipc.send_output(state_msg);
                }
            }
        }
    }
}
```

- [ ] **Step 7: Verify PTY crate compiles**

Run: `cd crates/cc-remote-pty && cargo check`
Expected: Compilation succeeds.

- [ ] **Step 8: Run all tests in PTY crate**

Run: `cd crates/cc-remote-pty && cargo test`
Expected: 3 protocol tests pass.

- [ ] **Step 9: Commit**

```bash
git add crates/
git commit -m "feat: add cc-remote-pty crate with protocol, pty spawn, and IPC server"
```

---

## Task 3: Go Sidecar (feishu-gateway)

**Files:**
- Create: `sidecar/feishu-gateway/go.mod`
- Create: `sidecar/feishu-gateway/main.go`
- Create: `sidecar/feishu-gateway/client.go`
- Create: `sidecar/feishu-gateway/protocol.go`
- Create: `sidecar/feishu-gateway/protocol_test.go`

- [ ] **Step 1: Write go.mod**

```
module github.com/cc-remote-seamless/feishu-gateway

go 1.22

require github.com/larksuite/oapi-sdk-go/v3 v3.4.3
```

- [ ] **Step 2: Write protocol.go — JSON Lines types for stdio communication**

```go
package main

import (
	"encoding/json"
	"fmt"
)

// Messages FROM Tauri TO Go sidecar (commands)
type Command struct {
	Type      string          `json:"type"`
	AppID     string          `json:"app_id,omitempty"`
	AppSecret string          `json:"app_secret,omitempty"`
	ChatID    string          `json:"chat_id,omitempty"`
	Text      string          `json:"text,omitempty"`
	Card      json.RawMessage `json:"card,omitempty"`
}

// Messages FROM Go sidecar TO Tauri (events)
type Event struct {
	Type      string `json:"type"`
	ChatID    string `json:"chat_id,omitempty"`
	Text      string `json:"text,omitempty"`
	Sender    string `json:"sender,omitempty"`
	MessageID string `json:"message_id,omitempty"`
	Reason    string `json:"reason,omitempty"`
	Message   string `json:"message,omitempty"`
}

func encodeEvent(e Event) string {
	b, _ := json.Marshal(e)
	return string(b)
}

func decodeCommand(line string) (Command, error) {
	var cmd Command
	err := json.Unmarshal([]byte(line), &cmd)
	if err != nil {
		return cmd, fmt.Errorf("decode command: %w", err)
	}
	return cmd, nil
}
```

- [ ] **Step 3: Write protocol_test.go**

```go
package main

import (
	"testing"
)

func TestEncodeEvent(t *testing.T) {
	e := Event{Type: "connected"}
	result := encodeEvent(e)
	if result != `{"type":"connected"}` {
		t.Errorf("unexpected: %s", result)
	}
}

func TestDecodeCommand(t *testing.T) {
	line := `{"type":"connect","app_id":"cli_123","app_secret":"sec_456"}`
	cmd, err := decodeCommand(line)
	if err != nil {
		t.Fatal(err)
	}
	if cmd.Type != "connect" || cmd.AppID != "cli_123" || cmd.AppSecret != "sec_456" {
		t.Errorf("unexpected: %+v", cmd)
	}
}

func TestDecodeCommandInvalid(t *testing.T) {
	_, err := decodeCommand("not json")
	if err == nil {
		t.Error("expected error for invalid JSON")
	}
}
```

- [ ] **Step 4: Run protocol tests**

Run: `cd sidecar/feishu-gateway && go test -run TestEncode -v && go test -run TestDecode -v`
Expected: 3 tests pass.

- [ ] **Step 5: Write client.go — Feishu SDK wrapper**

```go
package main

import (
	"context"
	"encoding/json"
	"fmt"

	lark "github.com/larksuite/oapi-sdk-go/v3"
	larkcore "github.com/larksuite/oapi-sdk-go/v3/core"
	larkim "github.com/larksuite/oapi-sdk-go/v3/service/im/v1"
	larkws "github.com/larksuite/oapi-sdk-go/v3/ws"
)

type FeishuClient struct {
	apiClient *lark.Client
	wsClient  *larkws.Client
	onMessage func(chatID, text, sender, messageID string)
}

func NewFeishuClient(appID, appSecret string, onMessage func(chatID, text, sender, messageID string)) *FeishuClient {
	apiClient := lark.NewClient(appID, appSecret)

	eventHandler := larkws.NewEventHandler()
	eventHandler.OnP2MessageReceiveV1(func(ctx context.Context, event *larkim.P2MessageReceiveV1) error {
		msg := event.Event.Message
		chatID := *msg.ChatId
		msgID := *msg.MessageId
		sender := *event.Event.Sender.SenderId.OpenId

		var content struct {
			Text string `json:"text"`
		}
		_ = json.Unmarshal([]byte(*msg.Content), &content)

		if onMessage != nil {
			onMessage(chatID, content.Text, sender, msgID)
		}
		return nil
	})

	wsClient := larkws.NewClient(appID, appSecret,
		larkws.WithEventHandler(eventHandler),
		larkws.WithLogLevel(larkcore.LogLevelInfo),
	)

	return &FeishuClient{
		apiClient: apiClient,
		wsClient:  wsClient,
		onMessage: onMessage,
	}
}

func (c *FeishuClient) Connect() error {
	return c.wsClient.Start(context.Background())
}

func (c *FeishuClient) SendText(chatID, text string) error {
	content, _ := json.Marshal(map[string]string{"text": text})
	req := larkim.NewCreateMessageReqBuilder().
		ReceiveIdType("chat_id").
		Body(larkim.NewCreateMessageReqBodyBuilder().
			ReceiveId(chatID).
			MsgType("text").
			Content(string(content)).
			Build()).
		Build()

	resp, err := c.apiClient.Im.Message.Create(context.Background(), req)
	if err != nil {
		return fmt.Errorf("send failed: %w", err)
	}
	if !resp.Success() {
		return fmt.Errorf("send failed: code=%d msg=%s", resp.Code, resp.Msg)
	}
	return nil
}

func (c *FeishuClient) Disconnect() {
	// wsClient will be garbage collected
}
```

- [ ] **Step 6: Write main.go — stdio JSON Lines bridge**

```go
package main

import (
	"bufio"
	"fmt"
	"os"
)

func main() {
	var client *FeishuClient
	scanner := bufio.NewScanner(os.Stdin)

	for scanner.Scan() {
		line := scanner.Text()
		cmd, err := decodeCommand(line)
		if err != nil {
			emitEvent(Event{Type: "error", Message: err.Error()})
			continue
		}

		switch cmd.Type {
		case "connect":
			client = NewFeishuClient(cmd.AppID, cmd.AppSecret, func(chatID, text, sender, messageID string) {
				emitEvent(Event{
					Type:      "message_received",
					ChatID:    chatID,
					Text:      text,
					Sender:    sender,
					MessageID: messageID,
				})
			})
			if err := client.Connect(); err != nil {
				emitEvent(Event{Type: "error", Message: err.Error()})
			} else {
				emitEvent(Event{Type: "connected"})
			}

		case "send_text":
			if client == nil {
				emitEvent(Event{Type: "error", Message: "not connected"})
				continue
			}
			if err := client.SendText(cmd.ChatID, cmd.Text); err != nil {
				emitEvent(Event{Type: "error", Message: err.Error()})
			}

		case "disconnect":
			if client != nil {
				client.Disconnect()
				client = nil
			}
			emitEvent(Event{Type: "disconnected", Reason: "requested"})

		default:
			emitEvent(Event{Type: "error", Message: fmt.Sprintf("unknown command: %s", cmd.Type)})
		}
	}
}

func emitEvent(e Event) {
	fmt.Println(encodeEvent(e))
}
```

- [ ] **Step 7: Run go mod tidy and verify build**

Run:
```bash
cd sidecar/feishu-gateway && go mod tidy && go build ./...
```
Expected: Build succeeds.

- [ ] **Step 8: Run tests**

Run: `cd sidecar/feishu-gateway && go test -v`
Expected: 3 protocol tests pass.

- [ ] **Step 9: Commit**

```bash
git add sidecar/
git commit -m "feat: add feishu-gateway Go sidecar with stdio JSON Lines protocol"
```

---

## Task 4: Rust Engine Core — Traits and Router

**Files:**
- Create: `src-tauri/src/agent/mod.rs`
- Create: `src-tauri/src/platform/mod.rs`
- Create: `src-tauri/src/engine/mod.rs`
- Create: `src-tauri/src/engine/router.rs`
- Create: `src-tauri/src/engine/store.rs`

- [ ] **Step 1: Write agent/mod.rs — AgentConnector trait**

```rust
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionState {
    Idle,
    Busy,
    WaitingPermission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub agent: String,
    pub state: SessionState,
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    StateChange { session_id: String, state: SessionState },
    Output { session_id: String, text: String },
    PermissionRequest { session_id: String, tool: String, input: String },
}

pub type EventSender = mpsc::UnboundedSender<AgentEvent>;

#[async_trait::async_trait]
pub trait AgentConnector: Send + Sync {
    fn id(&self) -> &str;
    async fn discover_sessions(&self) -> Vec<SessionInfo>;
    async fn inject_input(&self, session_id: &str, text: &str) -> Result<(), String>;
    fn subscribe(&self, sender: EventSender);
}

pub mod claude_code;
```

- [ ] **Step 2: Write platform/mod.rs — IMPlatform trait**

```rust
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IMMessage {
    pub chat_id: String,
    pub text: String,
    pub sender: String,
    pub platform: String,
    pub timestamp: u64,
}

pub type MessageSender = mpsc::UnboundedSender<IMMessage>;

#[async_trait::async_trait]
pub trait IMPlatform: Send + Sync {
    fn id(&self) -> &str;
    async fn connect(&mut self) -> Result<(), String>;
    async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String>;
    fn subscribe(&self, sender: MessageSender);
    async fn disconnect(&mut self);
}

pub mod feishu;
```

- [ ] **Step 3: Write engine/router.rs — BindingStore**

```rust
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone)]
pub struct Binding {
    pub agent_id: String,
    pub session_id: String,
}

pub struct BindingStore {
    bindings: RwLock<HashMap<String, Binding>>,
}

impl BindingStore {
    pub fn new() -> Self {
        Self {
            bindings: RwLock::new(HashMap::new()),
        }
    }

    pub fn bind(&self, chat_id: &str, agent_id: &str, session_id: &str) {
        let mut map = self.bindings.write().unwrap();
        map.insert(chat_id.to_string(), Binding {
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
        });
    }

    pub fn unbind(&self, chat_id: &str) {
        let mut map = self.bindings.write().unwrap();
        map.remove(chat_id);
    }

    pub fn get(&self, chat_id: &str) -> Option<Binding> {
        let map = self.bindings.read().unwrap();
        map.get(chat_id).cloned()
    }

    pub fn find_chat_for_session(&self, agent_id: &str, session_id: &str) -> Option<String> {
        let map = self.bindings.read().unwrap();
        map.iter()
            .find(|(_, b)| b.agent_id == agent_id && b.session_id == session_id)
            .map(|(chat_id, _)| chat_id.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_and_get() {
        let store = BindingStore::new();
        store.bind("chat_1", "claude", "sess_a");
        let binding = store.get("chat_1").unwrap();
        assert_eq!(binding.agent_id, "claude");
        assert_eq!(binding.session_id, "sess_a");
    }

    #[test]
    fn unbind_removes() {
        let store = BindingStore::new();
        store.bind("chat_1", "claude", "sess_a");
        store.unbind("chat_1");
        assert!(store.get("chat_1").is_none());
    }

    #[test]
    fn find_chat_for_session() {
        let store = BindingStore::new();
        store.bind("chat_1", "claude", "sess_a");
        store.bind("chat_2", "claude", "sess_b");
        let found = store.find_chat_for_session("claude", "sess_a");
        assert_eq!(found, Some("chat_1".to_string()));
    }
}
```

- [ ] **Step 4: Run router tests**

Run: `cd src-tauri && cargo test router`
Expected: 3 tests pass.

- [ ] **Step 5: Write engine/store.rs — MessageStore (SQLite)**

```rust
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: i64,
    pub session_id: String,
    pub source: String,
    pub text: String,
    pub timestamp: i64,
}

pub struct MessageStore {
    conn: Mutex<Connection>,
}

impl MessageStore {
    pub fn open(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                source TEXT NOT NULL,
                text TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_session ON messages(session_id);"
        ).map_err(|e| e.to_string())?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn open_in_memory() -> Result<Self, String> {
        Self::open(":memory:")
    }

    pub fn insert(&self, session_id: &str, source: &str, text: &str, timestamp: i64) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO messages (session_id, source, text, timestamp) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, source, text, timestamp],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_by_session(&self, session_id: &str, limit: usize) -> Vec<StoredMessage> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, source, text, timestamp FROM messages WHERE session_id = ?1 ORDER BY timestamp DESC LIMIT ?2"
        ).unwrap();
        stmt.query_map(params![session_id, limit as i64], |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                source: row.get(2)?,
                text: row.get(3)?,
                timestamp: row.get(4)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_query() {
        let store = MessageStore::open_in_memory().unwrap();
        store.insert("sess_1", "cli", "hello", 1000).unwrap();
        store.insert("sess_1", "feishu", "world", 1001).unwrap();
        let msgs = store.get_by_session("sess_1", 10);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].text, "world");
        assert_eq!(msgs[1].text, "hello");
    }

    #[test]
    fn different_sessions_isolated() {
        let store = MessageStore::open_in_memory().unwrap();
        store.insert("sess_1", "cli", "msg1", 1000).unwrap();
        store.insert("sess_2", "feishu", "msg2", 1001).unwrap();
        let msgs = store.get_by_session("sess_1", 10);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "msg1");
    }
}
```

- [ ] **Step 6: Run store tests**

Run: `cd src-tauri && cargo test store`
Expected: 2 tests pass.

- [ ] **Step 7: Write engine/mod.rs — Engine orchestrator**

```rust
pub mod router;
pub mod store;

use crate::agent::{AgentConnector, AgentEvent, SessionInfo, SessionState};
use crate::platform::IMMessage;
use router::BindingStore;
use store::MessageStore;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

pub struct Engine {
    agents: HashMap<String, Arc<dyn AgentConnector>>,
    platforms: HashMap<String, Arc<dyn crate::platform::IMPlatform>>,
    pub bindings: Arc<BindingStore>,
    pub messages: Arc<MessageStore>,
}

impl Engine {
    pub fn new(store_path: &str) -> Result<Self, String> {
        Ok(Self {
            agents: HashMap::new(),
            platforms: HashMap::new(),
            bindings: Arc::new(BindingStore::new()),
            messages: Arc::new(MessageStore::open(store_path)?),
        })
    }

    pub fn register_agent(&mut self, agent: Arc<dyn AgentConnector>) {
        let id = agent.id().to_string();
        self.agents.insert(id, agent);
    }

    pub fn register_platform(&mut self, platform: Arc<dyn crate::platform::IMPlatform>) {
        let id = platform.id().to_string();
        self.platforms.insert(id, platform);
    }

    pub fn agents_iter(&self) -> impl Iterator<Item = &Arc<dyn AgentConnector>> {
        self.agents.values()
    }

    pub async fn get_sessions(&self) -> Vec<SessionInfo> {
        let mut all = vec![];
        for agent in self.agents.values() {
            all.extend(agent.discover_sessions().await);
        }
        all
    }

    pub async fn handle_im_message(&self, msg: IMMessage) {
        let timestamp = msg.timestamp as i64;

        let binding = match self.bindings.get(&msg.chat_id) {
            Some(b) => b,
            None => {
                warn!("No binding for chat_id={}", msg.chat_id);
                return;
            }
        };

        self.messages.insert(&binding.session_id, &msg.platform, &msg.text, timestamp).ok();

        if let Some(agent) = self.agents.get(&binding.agent_id) {
            if let Err(e) = agent.inject_input(&binding.session_id, &msg.text).await {
                warn!("Inject failed: {}", e);
            }
        }
    }

    pub async fn handle_agent_event(&self, event: AgentEvent) {
        match event {
            AgentEvent::Output { ref session_id, ref text } => {
                let now = chrono::Utc::now().timestamp();
                self.messages.insert(session_id, "agent", text, now).ok();

                for (platform_id, platform) in &self.platforms {
                    if let Some(chat_id) = self.bindings.find_chat_for_session("claude-code", session_id) {
                        if let Err(e) = platform.send_text(&chat_id, text).await {
                            warn!("Send to {} failed: {}", platform_id, e);
                        }
                    }
                }
            }
            AgentEvent::StateChange { ref session_id, ref state } => {
                info!("Session {} state -> {:?}", session_id, state);
            }
            AgentEvent::PermissionRequest { ref session_id, ref tool, .. } => {
                info!("Permission request: session={} tool={}", session_id, tool);
            }
        }
    }
}
```

- [ ] **Step 8: Update lib.rs to include all modules**

```rust
pub mod agent;
pub mod platform;
pub mod engine;
pub mod hook;
pub mod pty;
pub mod window;
mod commands;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_sessions,
            commands::get_messages,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 9: Verify full build**

Run: `cd src-tauri && cargo check`
Expected: Compilation succeeds.

- [ ] **Step 10: Run all tests**

Run: `cd src-tauri && cargo test`
Expected: 5 tests pass (3 router + 2 store).

- [ ] **Step 11: Commit**

```bash
git add src-tauri/src/
git commit -m "feat: add Engine core with AgentConnector/IMPlatform traits, router, and message store"
```

---

## Task 5: Hook Server (HTTP)

**Files:**
- Create: `src-tauri/src/hook/server.rs`
- Create: `src-tauri/src/hook/mod.rs`

- [ ] **Step 1: Write hook/mod.rs**

```rust
pub mod server;
```

- [ ] **Step 2: Write hook/server.rs — axum HTTP server for Claude Code hooks**

```rust
use axum::{
    extract::State,
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPayload {
    pub session_id: Option<String>,
    pub tool: Option<String>,
    pub input: Option<String>,
}

#[derive(Debug, Clone)]
pub enum HookEvent {
    Stop { session_id: String },
    PromptSubmit { session_id: String },
    PreToolUse { session_id: String, tool: String, input: String },
}

pub type HookEventSender = mpsc::UnboundedSender<HookEvent>;

struct HookState {
    tx: HookEventSender,
}

pub async fn start_hook_server(port: u16, tx: HookEventSender) -> Result<(), String> {
    let state = Arc::new(HookState { tx });

    let app = Router::new()
        .route("/hook/stop", post(handle_stop))
        .route("/hook/prompt", post(handle_prompt))
        .route("/hook/pre-tool", post(handle_pre_tool))
        .route("/health", axum::routing::get(|| async { "ok" }))
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    info!("Hook server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("bind failed: {}", e))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {}", e))
}

async fn handle_stop(
    State(state): State<Arc<HookState>>,
    Json(payload): Json<HookPayload>,
) -> StatusCode {
    let session_id = payload.session_id.unwrap_or_default();
    let _ = state.tx.send(HookEvent::Stop { session_id });
    StatusCode::OK
}

async fn handle_prompt(
    State(state): State<Arc<HookState>>,
    Json(payload): Json<HookPayload>,
) -> StatusCode {
    let session_id = payload.session_id.unwrap_or_default();
    let _ = state.tx.send(HookEvent::PromptSubmit { session_id });
    StatusCode::OK
}

async fn handle_pre_tool(
    State(state): State<Arc<HookState>>,
    Json(payload): Json<HookPayload>,
) -> StatusCode {
    let session_id = payload.session_id.unwrap_or_default();
    let tool = payload.tool.unwrap_or_default();
    let input = payload.input.unwrap_or_default();
    let _ = state.tx.send(HookEvent::PreToolUse { session_id, tool, input });
    StatusCode::OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn hook_event_sent_on_stop() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let state = Arc::new(HookState { tx });

        let payload = HookPayload {
            session_id: Some("test_sess".into()),
            tool: None,
            input: None,
        };

        handle_stop(State(state), Json(payload)).await;

        let event = rx.recv().await.unwrap();
        match event {
            HookEvent::Stop { session_id } => assert_eq!(session_id, "test_sess"),
            _ => panic!("wrong event type"),
        }
    }

    #[tokio::test]
    async fn hook_event_sent_on_pre_tool() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let state = Arc::new(HookState { tx });

        let payload = HookPayload {
            session_id: Some("s1".into()),
            tool: Some("Write".into()),
            input: Some("file.rs".into()),
        };

        handle_pre_tool(State(state), Json(payload)).await;

        let event = rx.recv().await.unwrap();
        match event {
            HookEvent::PreToolUse { session_id, tool, input } => {
                assert_eq!(session_id, "s1");
                assert_eq!(tool, "Write");
                assert_eq!(input, "file.rs");
            }
            _ => panic!("wrong event type"),
        }
    }
}
```

- [ ] **Step 3: Run hook tests**

Run: `cd src-tauri && cargo test hook`
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/hook/
git commit -m "feat: add HTTP hook server for Claude Code lifecycle events"
```

---

## Task 6: ClaudeCodeConnector — Agent Implementation

**Files:**
- Create: `src-tauri/src/agent/claude_code.rs`

- [ ] **Step 1: Write agent/claude_code.rs**

```rust
use crate::agent::{AgentConnector, AgentEvent, EventSender, SessionInfo, SessionState};
use crate::hook::server::HookEvent;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};
use tokio::sync::mpsc;
use tracing::info;

struct PtyConnection {
    session_id: String,
    socket_path: PathBuf,
    state: SessionState,
}

pub struct ClaudeCodeConnector {
    sessions: RwLock<HashMap<String, PtyConnection>>,
    event_senders: Mutex<Vec<EventSender>>,
    socket_dir: String,
}

impl ClaudeCodeConnector {
    pub fn new(socket_dir: &str) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            event_senders: Mutex::new(vec![]),
            socket_dir: socket_dir.to_string(),
        }
    }

    pub fn handle_hook_event(&self, event: HookEvent) {
        match event {
            HookEvent::Stop { session_id } => {
                self.update_state(&session_id, SessionState::Idle);
            }
            HookEvent::PromptSubmit { session_id } => {
                self.update_state(&session_id, SessionState::Busy);
            }
            HookEvent::PreToolUse { session_id, tool, input } => {
                self.update_state(&session_id, SessionState::WaitingPermission);
                self.emit(AgentEvent::PermissionRequest { session_id, tool, input });
            }
        }
    }

    pub fn register_session(&self, session_id: &str) {
        let socket_path = PathBuf::from(format!(
            "{}/cc-remote-claude-{}.sock",
            self.socket_dir, session_id
        ));
        let conn = PtyConnection {
            session_id: session_id.to_string(),
            socket_path,
            state: SessionState::Idle,
        };
        self.sessions.write().unwrap().insert(session_id.to_string(), conn);
    }

    fn update_state(&self, session_id: &str, state: SessionState) {
        if let Some(conn) = self.sessions.write().unwrap().get_mut(session_id) {
            conn.state = state.clone();
        }
        self.emit(AgentEvent::StateChange {
            session_id: session_id.to_string(),
            state,
        });
    }

    fn emit(&self, event: AgentEvent) {
        let senders = self.event_senders.lock().unwrap();
        for tx in senders.iter() {
            let _ = tx.send(event.clone());
        }
    }
}

#[async_trait::async_trait]
impl AgentConnector for ClaudeCodeConnector {
    fn id(&self) -> &str {
        "claude-code"
    }

    async fn discover_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.read().unwrap();
        sessions.values().map(|conn| SessionInfo {
            id: conn.session_id.clone(),
            agent: "claude-code".to_string(),
            state: conn.state.clone(),
            working_dir: None,
        }).collect()
    }

    async fn inject_input(&self, session_id: &str, text: &str) -> Result<(), String> {
        let socket_path = {
            let sessions = self.sessions.read().unwrap();
            match sessions.get(session_id) {
                Some(conn) => conn.socket_path.clone(),
                None => return Err(format!("session {} not found", session_id)),
            }
        };

        #[cfg(unix)]
        {
            use tokio::io::AsyncWriteExt;
            use tokio::net::UnixStream;

            let mut stream = UnixStream::connect(&socket_path)
                .await
                .map_err(|e| format!("connect to PTY socket failed: {}", e))?;

            let msg = serde_json::json!({"type": "input", "data": format!("{}\n", text)});
            let mut line = serde_json::to_string(&msg).unwrap();
            line.push('\n');

            stream.write_all(line.as_bytes())
                .await
                .map_err(|e| format!("write failed: {}", e))?;
        }

        info!("Injected input to session {}", session_id);
        Ok(())
    }

    fn subscribe(&self, sender: EventSender) {
        self.event_senders.lock().unwrap().push(sender);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_discover() {
        let connector = ClaudeCodeConnector::new("/tmp");
        connector.register_session("abc123");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let sessions = rt.block_on(connector.discover_sessions());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "abc123");
        assert_eq!(sessions[0].state, SessionState::Idle);
    }

    #[test]
    fn hook_updates_state() {
        let connector = ClaudeCodeConnector::new("/tmp");
        connector.register_session("s1");

        connector.handle_hook_event(HookEvent::PromptSubmit { session_id: "s1".into() });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let sessions = rt.block_on(connector.discover_sessions());
        assert_eq!(sessions[0].state, SessionState::Busy);
    }

    #[test]
    fn event_emitted_on_state_change() {
        let connector = ClaudeCodeConnector::new("/tmp");
        connector.register_session("s1");

        let (tx, mut rx) = mpsc::unbounded_channel();
        connector.subscribe(tx);

        connector.handle_hook_event(HookEvent::Stop { session_id: "s1".into() });

        let event = rx.try_recv().unwrap();
        match event {
            AgentEvent::StateChange { session_id, state } => {
                assert_eq!(session_id, "s1");
                assert_eq!(state, SessionState::Idle);
            }
            _ => panic!("wrong event"),
        }
    }
}
```

- [ ] **Step 2: Run connector tests**

Run: `cd src-tauri && cargo test claude_code`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/agent/claude_code.rs
git commit -m "feat: add ClaudeCodeConnector with PTY socket injection and hook state tracking"
```

---

## Task 7: FeishuPlatform — Go Sidecar Communication

**Files:**
- Create: `src-tauri/src/platform/feishu.rs`

- [ ] **Step 1: Write platform/feishu.rs**

```rust
use crate::platform::{IMMessage, IMPlatform, MessageSender};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{info, error};

#[derive(Debug, Serialize)]
struct SidecarCommand {
    #[serde(rename = "type")]
    cmd_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SidecarEvent {
    #[serde(rename = "type")]
    event_type: String,
    chat_id: Option<String>,
    text: Option<String>,
    sender: Option<String>,
    message_id: Option<String>,
    #[allow(dead_code)]
    reason: Option<String>,
    message: Option<String>,
}

pub struct FeishuPlatform {
    app_id: String,
    app_secret: String,
    sidecar_path: String,
    stdin_tx: Option<mpsc::UnboundedSender<String>>,
    message_senders: Mutex<Vec<MessageSender>>,
}

impl FeishuPlatform {
    pub fn new(app_id: &str, app_secret: &str, sidecar_path: &str) -> Self {
        Self {
            app_id: app_id.to_string(),
            app_secret: app_secret.to_string(),
            sidecar_path: sidecar_path.to_string(),
            stdin_tx: None,
            message_senders: Mutex::new(vec![]),
        }
    }

    fn send_command(&self, cmd: SidecarCommand) -> Result<(), String> {
        let tx = self.stdin_tx.as_ref().ok_or("sidecar not running")?;
        let mut line = serde_json::to_string(&cmd).map_err(|e| e.to_string())?;
        line.push('\n');
        tx.send(line).map_err(|e| e.to_string())
    }
}

#[async_trait::async_trait]
impl IMPlatform for FeishuPlatform {
    fn id(&self) -> &str {
        "feishu"
    }

    async fn connect(&mut self) -> Result<(), String> {
        let mut child = Command::new(&self.sidecar_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn sidecar failed: {}", e))?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<String>();

        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(line) = stdin_rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        self.stdin_tx = Some(stdin_tx);

        let senders = self.message_senders.lock().unwrap().clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(event) = serde_json::from_str::<SidecarEvent>(&line) {
                    match event.event_type.as_str() {
                        "connected" => info!("Feishu sidecar connected"),
                        "message_received" => {
                            let msg = IMMessage {
                                chat_id: event.chat_id.unwrap_or_default(),
                                text: event.text.unwrap_or_default(),
                                sender: event.sender.unwrap_or_default(),
                                platform: "feishu".to_string(),
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs(),
                            };
                            for tx in &senders {
                                let _ = tx.send(msg.clone());
                            }
                        }
                        "error" => {
                            error!("Feishu sidecar error: {}", event.message.unwrap_or_default());
                        }
                        _ => {}
                    }
                }
            }
        });

        self.send_command(SidecarCommand {
            cmd_type: "connect".into(),
            app_id: Some(self.app_id.clone()),
            app_secret: Some(self.app_secret.clone()),
            chat_id: None,
            text: None,
        })?;

        Ok(())
    }

    async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String> {
        self.send_command(SidecarCommand {
            cmd_type: "send_text".into(),
            app_id: None,
            app_secret: None,
            chat_id: Some(chat_id.to_string()),
            text: Some(text.to_string()),
        })
    }

    fn subscribe(&self, sender: MessageSender) {
        self.message_senders.lock().unwrap().push(sender);
    }

    async fn disconnect(&mut self) {
        let _ = self.send_command(SidecarCommand {
            cmd_type: "disconnect".into(),
            app_id: None,
            app_secret: None,
            chat_id: None,
            text: None,
        });
        self.stdin_tx = None;
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: Compilation succeeds.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/platform/feishu.rs
git commit -m "feat: add FeishuPlatform with Go sidecar stdio communication"
```

---

## Task 8: Tauri Commands and App Wiring

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src/types.ts`
- Create: `src-tauri/src/pty/mod.rs`
- Create: `src-tauri/src/window/mod.rs`

- [ ] **Step 1: Write src-tauri/src/pty/mod.rs (stub)**

```rust
// PTY proxy management - connects to cc-remote-pty processes via IPC
```

- [ ] **Step 2: Write src-tauri/src/window/mod.rs**

```rust
use tauri::{AppHandle, Manager, WebviewWindowBuilder, WebviewUrl};
use tracing::info;

pub fn show_permission_popup(
    app: &AppHandle,
    session_id: &str,
    tool: &str,
    input: &str,
) -> Result<(), String> {
    let url = format!(
        "/?view=permission&session={}&tool={}&input={}",
        session_id, tool, input
    );

    if let Some(win) = app.get_webview_window("permission") {
        win.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    WebviewWindowBuilder::new(app, "permission", WebviewUrl::App(url.into()))
        .title("Permission")
        .inner_size(350.0, 180.0)
        .always_on_top(true)
        .resizable(false)
        .decorations(false)
        .build()
        .map_err(|e| e.to_string())?;

    info!("Permission popup shown: tool={}", tool);
    Ok(())
}

pub fn open_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("/?view=main".into()))
        .title("CC Remote Seamless")
        .inner_size(600.0, 400.0)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(())
}
```

- [ ] **Step 3: Rewrite commands.rs with Engine integration**

```rust
use crate::agent::SessionInfo;
use crate::engine::store::StoredMessage;
use crate::engine::Engine;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

pub type EngineState = Arc<Mutex<Engine>>;

#[tauri::command]
pub async fn get_sessions(engine: State<'_, EngineState>) -> Result<Vec<SessionInfo>, String> {
    let eng = engine.lock().await;
    Ok(eng.get_sessions().await)
}

#[tauri::command]
pub async fn get_messages(
    engine: State<'_, EngineState>,
    session_id: String,
    limit: Option<usize>,
) -> Result<Vec<StoredMessage>, String> {
    let eng = engine.lock().await;
    Ok(eng.messages.get_by_session(&session_id, limit.unwrap_or(50)))
}

#[tauri::command]
pub async fn bind_session(
    engine: State<'_, EngineState>,
    chat_id: String,
    agent_id: String,
    session_id: String,
) -> Result<(), String> {
    let eng = engine.lock().await;
    eng.bindings.bind(&chat_id, &agent_id, &session_id);
    Ok(())
}

#[tauri::command]
pub async fn inject_input(
    engine: State<'_, EngineState>,
    session_id: String,
    text: String,
) -> Result<(), String> {
    let eng = engine.lock().await;
    for agent in eng.agents_iter() {
        let sessions = agent.discover_sessions().await;
        if sessions.iter().any(|s| s.id == session_id) {
            return agent.inject_input(&session_id, &text).await;
        }
    }
    Err(format!("session {} not found", session_id))
}
```

- [ ] **Step 4: Rewrite lib.rs with full setup**

```rust
pub mod agent;
pub mod platform;
pub mod engine;
pub mod hook;
pub mod pty;
pub mod window;
mod commands;

use agent::claude_code::ClaudeCodeConnector;
use agent::{AgentEvent, EventSender};
use commands::EngineState;
use engine::Engine;
use hook::server::{start_hook_server, HookEvent};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::{mpsc, Mutex};

pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let handle = app.handle().clone();

            let mut engine = Engine::new(":memory:").expect("create engine");
            let connector = Arc::new(ClaudeCodeConnector::new("/tmp"));
            engine.register_agent(connector.clone());

            let engine = Arc::new(Mutex::new(engine));

            // Hook server
            let (hook_tx, mut hook_rx) = mpsc::unbounded_channel::<HookEvent>();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = start_hook_server(23399, hook_tx).await {
                    tracing::error!("Hook server failed: {}", e);
                }
            });

            // Hook event dispatch
            let connector_for_hooks = connector.clone();
            let handle_for_hooks = handle.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(event) = hook_rx.recv().await {
                    if let HookEvent::PreToolUse { ref session_id, ref tool, ref input } = event {
                        let _ = window::show_permission_popup(
                            &handle_for_hooks, session_id, tool, input,
                        );
                    }
                    connector_for_hooks.handle_hook_event(event);
                }
            });

            // Agent event dispatch
            let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AgentEvent>();
            connector.subscribe(agent_tx);

            let engine_for_events = engine.clone();
            let handle_for_events = handle.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(event) = agent_rx.recv().await {
                    let eng = engine_for_events.lock().await;
                    eng.handle_agent_event(event).await;
                    let _ = handle_for_events.emit("sessions-updated", ());
                }
            });

            app.manage(engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_sessions,
            commands::get_messages,
            commands::bind_session,
            commands::inject_input,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 5: Write src/types.ts**

```typescript
export interface SessionInfo {
  id: string;
  agent: string;
  state: "Idle" | "Busy" | "WaitingPermission";
  working_dir: string | null;
}

export interface StoredMessage {
  id: number;
  session_id: string;
  source: string;
  text: string;
  timestamp: number;
}
```

- [ ] **Step 6: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: Compilation succeeds.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/ src/types.ts
git commit -m "feat: wire Engine into Tauri commands with session/message/binding IPC"
```

---

## Task 9: Vue 3 Frontend — Floating Widget + Main Window

**Files:**
- Modify: `src/App.vue`
- Create: `src/views/FloatingWidget.vue`
- Create: `src/views/MainWindow.vue`
- Create: `src/views/PermissionPopup.vue`
- Create: `src/stores/sessions.ts`
- Create: `src/stores/messages.ts`
- Create: `src/components/StatusBadge.vue`
- Create: `src/components/SessionList.vue`
- Create: `src/components/MessageFlow.vue`

- [ ] **Step 1: Write src/stores/sessions.ts**

```typescript
import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { SessionInfo } from "../types";

export const useSessionStore = defineStore("sessions", () => {
  const sessions = ref<SessionInfo[]>([]);
  const activeSessionId = ref<string | null>(null);

  async function refresh() {
    sessions.value = await invoke<SessionInfo[]>("get_sessions");
  }

  function setActive(id: string) {
    activeSessionId.value = id;
  }

  return { sessions, activeSessionId, refresh, setActive };
});
```

- [ ] **Step 2: Write src/stores/messages.ts**

```typescript
import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { StoredMessage } from "../types";

export const useMessageStore = defineStore("messages", () => {
  const messages = ref<StoredMessage[]>([]);
  const loading = ref(false);

  async function loadForSession(sessionId: string) {
    loading.value = true;
    messages.value = await invoke<StoredMessage[]>("get_messages", {
      sessionId,
      limit: 50,
    });
    loading.value = false;
  }

  return { messages, loading, loadForSession };
});
```

- [ ] **Step 3: Write src/components/StatusBadge.vue**

```vue
<script setup lang="ts">
defineProps<{
  state: "Idle" | "Busy" | "WaitingPermission";
}>();

const colors: Record<string, string> = {
  Idle: "#4ade80",
  Busy: "#facc15",
  WaitingPermission: "#ef4444",
};

const labels: Record<string, string> = {
  Idle: "idle",
  Busy: "busy",
  WaitingPermission: "waiting",
};
</script>

<template>
  <span class="badge">
    <span class="dot" :style="{ background: colors[state] }"></span>
    <span class="label">{{ labels[state] }}</span>
  </span>
</template>

<style scoped>
.badge { display: inline-flex; align-items: center; gap: 4px; }
.dot { width: 6px; height: 6px; border-radius: 50%; }
.label { font-size: 10px; color: var(--text-secondary); }
</style>
```

- [ ] **Step 4: Write src/components/SessionList.vue**

```vue
<script setup lang="ts">
import { useSessionStore } from "../stores/sessions";
import StatusBadge from "./StatusBadge.vue";

const store = useSessionStore();
const emit = defineEmits<{ select: [sessionId: string] }>();

function selectSession(id: string) {
  store.setActive(id);
  emit("select", id);
}
</script>

<template>
  <div class="session-list">
    <div class="header">SESSIONS</div>
    <div
      v-for="session in store.sessions" :key="session.id"
      class="session-item" :class="{ active: session.id === store.activeSessionId }"
      @click="selectSession(session.id)"
    >
      <div class="session-row">
        <StatusBadge :state="session.state" />
        <span class="name">{{ session.agent }} #{{ session.id.slice(0, 4) }}</span>
      </div>
      <div class="meta">{{ session.working_dir || "~" }}</div>
    </div>
  </div>
</template>

<style scoped>
.session-list { width: 180px; border-right: 1px solid var(--border); padding: 8px; overflow-y: auto; }
.header { font-size: 10px; color: var(--text-secondary); margin-bottom: 8px; }
.session-item { padding: 6px 8px; border-radius: 4px; cursor: pointer; margin-bottom: 4px; }
.session-item:hover, .session-item.active { background: var(--bg-hover); }
.session-row { display: flex; align-items: center; gap: 6px; }
.name { font-size: 12px; }
.meta { font-size: 10px; color: var(--text-secondary); margin-top: 2px; margin-left: 10px; }
</style>
```

- [ ] **Step 5: Write src/components/MessageFlow.vue**

```vue
<script setup lang="ts">
import { useMessageStore } from "../stores/messages";
import { computed } from "vue";

const store = useMessageStore();

const sortedMessages = computed(() =>
  [...store.messages].sort((a, b) => a.timestamp - b.timestamp)
);

function sourceLabel(source: string) {
  const map: Record<string, string> = { cli: "CLI", feishu: "飞书", agent: "Agent" };
  return map[source] || source;
}

function formatTime(ts: number) {
  return new Date(ts * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
</script>

<template>
  <div class="message-flow">
    <div v-if="store.loading" class="empty">Loading...</div>
    <div v-else-if="sortedMessages.length === 0" class="empty">No messages yet</div>
    <div v-else class="messages">
      <div v-for="msg in sortedMessages" :key="msg.id" class="message">
        <div class="msg-header">
          <span class="source" :class="`source-${msg.source}`">{{ sourceLabel(msg.source) }}</span>
          <span class="time">{{ formatTime(msg.timestamp) }}</span>
        </div>
        <div class="msg-body">{{ msg.text }}</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.message-flow { flex: 1; padding: 8px; overflow-y: auto; }
.empty { color: var(--text-secondary); font-size: 12px; text-align: center; margin-top: 40px; }
.message { margin-bottom: 12px; }
.msg-header { display: flex; align-items: center; gap: 6px; margin-bottom: 3px; }
.source { font-size: 10px; padding: 1px 5px; border-radius: 2px; background: #334155; }
.source-feishu { background: #065f46; color: #6ee7b7; }
.source-agent { background: #1e3a5f; color: #93c5fd; }
.time { font-size: 10px; color: var(--text-secondary); }
.msg-body { padding: 6px 8px; background: var(--bg-secondary); border-radius: 4px; font-size: 12px; white-space: pre-wrap; }
</style>
```

- [ ] **Step 6: Write src/views/FloatingWidget.vue**

```vue
<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { useSessionStore } from "../stores/sessions";
import StatusBadge from "../components/StatusBadge.vue";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

const store = useSessionStore();
let interval: number;

onMounted(() => {
  store.refresh();
  interval = window.setInterval(() => store.refresh(), 2000);
});
onUnmounted(() => clearInterval(interval));

async function openMainWindow() {
  const existing = await WebviewWindow.getByLabel("main");
  if (existing) { await existing.setFocus(); }
  else { new WebviewWindow("main", { url: "/?view=main", title: "CC Remote Seamless", width: 600, height: 400 }); }
}
</script>

<template>
  <div class="float-widget" @click="openMainWindow">
    <div class="row">
      <StatusBadge v-if="store.sessions.length > 0" :state="store.sessions[0].state" />
      <span class="agent-name">{{ store.sessions.length > 0 ? store.sessions[0].agent : "no session" }}</span>
      <span class="count">{{ store.sessions.length }} sessions</span>
    </div>
  </div>
</template>

<style scoped>
.float-widget { padding: 12px 16px; background: var(--bg-primary); border-radius: 8px; cursor: pointer; border: 1px solid var(--border); }
.float-widget:hover { background: var(--bg-hover); }
.row { display: flex; align-items: center; gap: 8px; }
.agent-name { font-weight: 600; font-size: 13px; }
.count { margin-left: auto; font-size: 11px; color: var(--text-secondary); }
</style>
```

- [ ] **Step 7: Write src/views/MainWindow.vue**

```vue
<script setup lang="ts">
import { onMounted } from "vue";
import { useSessionStore } from "../stores/sessions";
import { useMessageStore } from "../stores/messages";
import SessionList from "../components/SessionList.vue";
import MessageFlow from "../components/MessageFlow.vue";

const sessionStore = useSessionStore();
const messageStore = useMessageStore();

onMounted(() => sessionStore.refresh());

function onSelectSession(id: string) {
  messageStore.loadForSession(id);
}
</script>

<template>
  <div class="main-window">
    <SessionList @select="onSelectSession" />
    <div class="content">
      <div v-if="!sessionStore.activeSessionId" class="placeholder">Select a session</div>
      <MessageFlow v-else />
    </div>
  </div>
</template>

<style scoped>
.main-window { display: flex; height: 100vh; background: var(--bg-primary); }
.content { flex: 1; display: flex; flex-direction: column; }
.placeholder { flex: 1; display: flex; align-items: center; justify-content: center; color: var(--text-secondary); }
</style>
```

- [ ] **Step 8: Write src/views/PermissionPopup.vue**

```vue
<script setup lang="ts">
import { ref } from "vue";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

const params = new URLSearchParams(window.location.search);
const tool = ref(params.get("tool") || "Unknown");
const input = ref(params.get("input") || "");
const sessionId = ref(params.get("session") || "");

async function respond(_action: string) {
  const win = getCurrentWebviewWindow();
  await win.close();
}
</script>

<template>
  <div class="permission-popup">
    <div class="header">
      <span>⚠️</span>
      <span class="title">Permission Request</span>
      <span class="session">{{ sessionId.slice(0, 6) }}</span>
    </div>
    <div class="tool-info"><code>{{ tool }}: {{ input }}</code></div>
    <div class="actions">
      <button class="btn allow" @click="respond('allow')">Allow</button>
      <button class="btn deny" @click="respond('deny')">Deny</button>
      <button class="btn always" @click="respond('always_allow')">Always Allow</button>
    </div>
  </div>
</template>

<style scoped>
.permission-popup { padding: 16px; background: var(--bg-primary); min-height: 100vh; }
.header { display: flex; align-items: center; gap: 6px; margin-bottom: 12px; }
.title { font-weight: 600; font-size: 14px; }
.session { margin-left: auto; font-size: 10px; color: var(--text-secondary); }
.tool-info { background: var(--bg-secondary); padding: 8px 12px; border-radius: 4px; margin-bottom: 16px; font-size: 12px; font-family: monospace; }
.actions { display: flex; gap: 8px; }
.btn { padding: 6px 16px; border: none; border-radius: 4px; font-size: 12px; cursor: pointer; }
.btn.allow { background: var(--accent-green); color: #000; }
.btn.deny { background: var(--accent-red); color: #fff; }
.btn.always { background: #334155; color: var(--text-primary); }
</style>
```

- [ ] **Step 9: Update src/App.vue**

```vue
<script setup lang="ts">
import { computed } from "vue";
import FloatingWidget from "./views/FloatingWidget.vue";
import MainWindow from "./views/MainWindow.vue";
import PermissionPopup from "./views/PermissionPopup.vue";

const view = computed(() => {
  const params = new URLSearchParams(window.location.search);
  return params.get("view") || "float";
});
</script>

<template>
  <FloatingWidget v-if="view === 'float'" />
  <MainWindow v-else-if="view === 'main'" />
  <PermissionPopup v-else-if="view === 'permission'" />
</template>
```

- [ ] **Step 10: Verify frontend types**

Run: `npm install && npx vue-tsc --noEmit`
Expected: No type errors.

- [ ] **Step 11: Commit**

```bash
git add src/
git commit -m "feat: add Vue 3 UI with floating widget, main window, and permission popup"
```

---

## Task 10: Integration Smoke Tests

**Files:**
- Create: `src-tauri/tests/e2e_smoke.rs`

- [ ] **Step 1: Write integration test**

```rust
use cc_remote_seamless_lib::agent::claude_code::ClaudeCodeConnector;
use cc_remote_seamless_lib::agent::{AgentConnector, AgentEvent, SessionState};
use cc_remote_seamless_lib::engine::Engine;
use cc_remote_seamless_lib::hook::server::HookEvent;
use cc_remote_seamless_lib::platform::IMMessage;
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::test]
async fn full_message_routing() {
    let mut engine = Engine::new(":memory:").unwrap();
    let connector = Arc::new(ClaudeCodeConnector::new("/tmp"));
    connector.register_session("test_sess");
    engine.register_agent(connector.clone());
    engine.bindings.bind("chat_abc", "claude-code", "test_sess");

    let msg = IMMessage {
        chat_id: "chat_abc".into(),
        text: "hello agent".into(),
        sender: "user1".into(),
        platform: "feishu".into(),
        timestamp: 1000,
    };
    engine.handle_im_message(msg).await;

    let msgs = engine.messages.get_by_session("test_sess", 10);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].text, "hello agent");
    assert_eq!(msgs[0].source, "feishu");
}

#[tokio::test]
async fn hook_updates_session_state() {
    let connector = Arc::new(ClaudeCodeConnector::new("/tmp"));
    connector.register_session("s1");

    let (tx, mut rx) = mpsc::unbounded_channel();
    connector.subscribe(tx);

    connector.handle_hook_event(HookEvent::PromptSubmit { session_id: "s1".into() });
    let sessions = connector.discover_sessions().await;
    assert_eq!(sessions[0].state, SessionState::Busy);

    connector.handle_hook_event(HookEvent::Stop { session_id: "s1".into() });
    let sessions = connector.discover_sessions().await;
    assert_eq!(sessions[0].state, SessionState::Idle);

    let e1 = rx.recv().await.unwrap();
    match e1 {
        AgentEvent::StateChange { state, .. } => assert_eq!(state, SessionState::Busy),
        _ => panic!("unexpected"),
    }
}

#[tokio::test]
async fn unbound_chat_does_not_inject() {
    let mut engine = Engine::new(":memory:").unwrap();
    let connector = Arc::new(ClaudeCodeConnector::new("/tmp"));
    connector.register_session("s1");
    engine.register_agent(connector.clone());

    // No binding — message should be dropped
    let msg = IMMessage {
        chat_id: "unknown_chat".into(),
        text: "lost message".into(),
        sender: "user1".into(),
        platform: "feishu".into(),
        timestamp: 2000,
    };
    engine.handle_im_message(msg).await;

    let msgs = engine.messages.get_by_session("s1", 10);
    assert_eq!(msgs.len(), 0);
}
```

- [ ] **Step 2: Run integration tests**

Run: `cd src-tauri && cargo test e2e_smoke`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/
git commit -m "test: add E2E smoke tests for message routing, hook state, and unbound chat"
```

---

## Task 11: Hook Installation Script

**Files:**
- Create: `scripts/install-hooks.sh`

- [ ] **Step 1: Write scripts/install-hooks.sh**

```bash
#!/usr/bin/env bash
set -euo pipefail

SETTINGS_FILE="$HOME/.claude/settings.json"
HOOK_PORT="${CC_REMOTE_HOOK_PORT:-23399}"
BASE_URL="http://localhost:${HOOK_PORT}"

echo "Installing CC Remote Seamless hooks..."
echo "  Hook server: ${BASE_URL}"
echo "  Settings: ${SETTINGS_FILE}"

mkdir -p "$(dirname "$SETTINGS_FILE")"

if [ ! -f "$SETTINGS_FILE" ]; then
    echo '{}' > "$SETTINGS_FILE"
fi

TMP=$(mktemp)

if command -v jq &>/dev/null; then
    jq --arg url "$BASE_URL" '.hooks = {
        "Stop": [{"type": "command", "command": ("curl -s -X POST " + $url + "/hook/stop -H \"Content-Type: application/json\" -d \"{}\"")}],
        "UserPromptSubmit": [{"type": "command", "command": ("curl -s -X POST " + $url + "/hook/prompt -H \"Content-Type: application/json\" -d \"{}\"")}],
        "PreToolUse": [{"type": "command", "command": ("curl -s -X POST " + $url + "/hook/pre-tool -H \"Content-Type: application/json\" -d \"{}\"")}]
    }' "$SETTINGS_FILE" > "$TMP" && mv "$TMP" "$SETTINGS_FILE"
else
    echo "ERROR: jq required. Install with: brew install jq"
    exit 1
fi

echo "Done. Hooks installed."
```

- [ ] **Step 2: Make executable and commit**

```bash
chmod +x scripts/install-hooks.sh
git add scripts/
git commit -m "feat: add hook installation script for Claude Code settings"
```

---

## Task 12: Workspace Cargo.toml

**Files:**
- Create: `Cargo.toml` (workspace root)

- [ ] **Step 1: Write workspace Cargo.toml**

```toml
[workspace]
members = [
    "src-tauri",
    "crates/cc-remote-pty",
]
resolver = "2"
```

- [ ] **Step 2: Verify workspace build**

Run: `cargo check --workspace`
Expected: Both crates compile.

- [ ] **Step 3: Run all workspace tests**

Run: `cargo test --workspace`
Expected: All tests pass (~18 total).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml
git commit -m "feat: add workspace Cargo.toml for multi-crate build"
```

---

## Summary

| Task | Component | Tests |
|------|-----------|-------|
| 1 | Tauri scaffolding + Vue shell | cargo check |
| 2 | cc-remote-pty (protocol, pty, ipc) | 3 unit |
| 3 | feishu-gateway Go sidecar | 3 unit |
| 4 | Engine core (traits, router, store) | 5 unit |
| 5 | Hook HTTP server | 2 unit |
| 6 | ClaudeCodeConnector | 3 unit |
| 7 | FeishuPlatform (sidecar comm) | cargo check |
| 8 | Tauri commands + wiring | cargo check |
| 9 | Vue 3 full UI | vue-tsc |
| 10 | E2E smoke tests | 3 integration |
| 11 | Hook install script | — |
| 12 | Workspace Cargo.toml | cargo check |

**Total: 12 tasks, ~19 unit/integration tests**
