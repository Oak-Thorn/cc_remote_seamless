# CC Remote Seamless V2 — Tauri 跨平台设计规格

## 概述

**目标：** 构建一个跨平台桌面应用，使用户离开电脑时能通过飞书与正在运行的 Claude Code CLI session 无缝交互，且桌面终端体验完全无损。

**核心理念：** IM 是辅助通道（桌面为主、手机为辅），而非唯一界面。

**技术栈：**
- 桌面框架: Tauri 2 (Rust backend + Vue 3 frontend)
- 飞书连接: Go sidecar + oapi-sdk-go/v3
- PTY 代理: Rust (portable-pty)
- 前端: Vue 3 + TypeScript + Vite

**跨平台支持：** Windows / macOS / Linux

---

## 架构

```
┌─────────────────────────────────────────────────────────────┐
│  Tauri App (Rust core + Vue 3 webview)                      │
│                                                             │
│  ┌──────────┐  ┌──────────────┐  ┌────────────────────┐    │
│  │ 浮窗 UI  │→ │  主窗口 UI   │  │  Rust Engine       │    │
│  │(状态概览)│  │(消息/权限)   │  │  ├ AgentConnector  │    │
│  └──────────┘  └──────────────┘  │  ├ IMPlatform      │    │
│                                   │  ├ Router/Store    │    │
│                                   └───────┬────────────┘    │
└───────────────────────────────────────────│─────────────────┘
                    ┌───────────────────────┼──────────────┐
                    │                       │              │
                    ▼                       ▼              ▼
        ┌───────────────────┐  ┌─────────────────┐  ┌──────────┐
        │  PTY Proxy        │  │  Hook Server    │  │  Go      │
        │  (Rust binary)    │  │  (HTTP, Rust内) │  │  Sidecar │
        │                   │  │                 │  │  (飞书)  │
        │  · 包裹 Agent CLI │  │  · Stop         │  │          │
        │  · Socket/Pipe    │  │  · Prompt       │  │  · WS    │
        │  · 注入输入       │  │  · PreToolUse   │  │  · 收发  │
        │  · 转发输出       │  │  · PostToolUse  │  │          │
        └────────┬──────────┘  └────────┬────────┘  └────┬─────┘
                 │                      │                 │
                 ▼                      ▼                 ▼
           Agent CLI              Claude Code         飞书云端
          (stdin/stdout)          (Hook events)           │
                                                     手机飞书
```

---

## 核心抽象

### AgentConnector trait

```rust
pub trait AgentConnector: Send + Sync {
    fn id(&self) -> &str;
    fn discover_sessions(&self) -> Vec<SessionInfo>;
    fn inject_input(&self, session_id: &str, text: &str) -> Result<()>;
    fn subscribe_output(&self, session_id: &str, handler: OutputHandler) -> Result<()>;
    fn on_event(&self, handler: EventHandler);
}

pub struct SessionInfo {
    pub id: String,
    pub agent: String,
    pub state: SessionState,
    pub working_dir: Option<String>,
}

pub enum SessionState { Idle, Busy, WaitingPermission }

pub enum AgentEvent {
    StateChange { session_id: String, state: SessionState },
    Output { session_id: String, text: String, source: MessageSource },
    PermissionRequest { session_id: String, tool: String, input: String, responder: PermissionResponder },
}
```

**已计划实现:** `ClaudeCodeConnector` (PTY Proxy + Hook)

**未来扩展:** `CodexConnector`, `GeminiConnector` — 实现相同 trait 即可接入。

### IMPlatform trait

```rust
pub trait IMPlatform: Send + Sync {
    fn id(&self) -> &str;
    fn connect(&mut self, config: &PlatformConfig) -> Result<()>;
    fn send_text(&self, chat_id: &str, text: &str) -> Result<()>;
    fn send_card(&self, chat_id: &str, card: Card) -> Result<()>;
    fn on_message(&self, handler: MessageHandler);
    fn disconnect(&mut self);
}

pub struct IMMessage {
    pub chat_id: String,
    pub text: String,
    pub sender: String,
    pub platform: String,
    pub timestamp: u64,
}

pub enum Card {
    Permission { tool: String, input: String, session_id: String },
    Status { sessions: Vec<SessionInfo> },
}
```

**已计划实现:** `FeishuPlatform` (通过 Go sidecar)

**未来扩展:** `TelegramPlatform`, `DingtalkPlatform`, `WecomPlatform` — 实现相同 trait 即可接入。

### Engine 路由

```rust
pub struct Engine {
    agents: HashMap<String, Box<dyn AgentConnector>>,
    platforms: HashMap<String, Box<dyn IMPlatform>>,
    bindings: BindingStore,  // chat_id <-> (agent_id, session_id) 映射
    messages: MessageStore,  // 消息持久化
}
```

**路由逻辑：**
- IM 消息到达 → 查 binding → 找到目标 (agent, session) → `agent.inject_input()`
- Agent 输出产生 → 查 binding → 找到目标 (platform, chat_id) → `platform.send_text()`
- 无 binding 时 → 回复提示用户使用 `/bind` 命令

---

## 组件详细设计

### 1. PTY Proxy (Rust 独立 binary: cc-remote-pty)

**职责：** 包裹 Agent CLI 进程，提供 IPC 通道用于输入注入和输出转发。

**通信方式：**
- macOS / Linux: Unix Domain Socket (`/tmp/cc-remote-{agent}-{session_id}.sock`)
- Windows: Named Pipe (`\\.\pipe\cc-remote-{agent}-{session_id}`)

**协议（JSON Lines over Socket/Pipe）：**

```json
{"type": "input", "data": "hello\n"}
{"type": "output", "data": "Agent response..."}
{"type": "resize", "cols": 120, "rows": 40}
{"type": "state", "waiting": true}
{"type": "exit", "code": 0}
```

**CLI 用法：**

```bash
cc-remote-pty --agent claude -- claude
cc-remote-pty --agent codex -- codex
```

**空闲检测：** 500ms 无 stdout 输出 → 发送 `{"type": "state", "waiting": true}`

**终端活动检测：** 检测到本地 stdin 输入 → 标记 terminal_active，阻止远程注入 2s

### 2. Hook Server (Rust, Tauri 内嵌)

**职责：** HTTP Server 接收 Claude Code 生命周期事件。

**端口：** `localhost:23399`（可配置）

**注册方式：** 应用首次启动时自动写入 Claude Code hooks 配置：

```jsonc
// ~/.claude/settings.json hooks 配置
{
  "hooks": {
    "Stop": [{"type": "command", "command": "curl -s -X POST http://localhost:23399/hook/stop -H 'Content-Type: application/json' -d '{}'"}],
    "UserPromptSubmit": [{"type": "command", "command": "curl -s -X POST http://localhost:23399/hook/prompt -H 'Content-Type: application/json' -d '{}'"}],
    "PreToolUse": [{"type": "command", "command": "curl -s -X POST http://localhost:23399/hook/pre-tool -H 'Content-Type: application/json' -d '{}'"}]
  }
}
```

**事件处理：**

| 端点 | Hook | 动作 |
|------|------|------|
| POST /hook/stop | Stop | session 状态→idle，flush 排队消息，推送输出到飞书 |
| POST /hook/prompt | UserPromptSubmit | session 状态→busy，记录来自 CLI 的输入 |
| POST /hook/pre-tool | PreToolUse | 权限类工具→弹出确认窗口 + 推送飞书卡片 |

### 3. Go Sidecar (feishu-gateway)

**职责：** 飞书 WebSocket 长连接，消息收发。

**通信方式：** Tauri sidecar stdio JSON Lines

**依赖：** `github.com/larksuite/oapi-sdk-go/v3`

**Tauri → Go 命令：**

```json
{"type": "connect", "app_id": "cli_xxx", "app_secret": "sec_xxx"}
{"type": "send_text", "chat_id": "oc_xxx", "text": "Agent 输出内容"}
{"type": "send_card", "chat_id": "oc_xxx", "card": {}}
{"type": "disconnect"}
```

**Go → Tauri 事件：**

```json
{"type": "connected"}
{"type": "message_received", "chat_id": "oc_xxx", "text": "/status", "sender": "user_xxx", "message_id": "msg_xxx"}
{"type": "disconnected", "reason": "network error"}
{"type": "error", "message": "token expired"}
```

**飞书事件订阅：**
- `im.message.receive_v1` — 接收用户消息
- 订阅方式：WebSocket 长连接模式（无需公网 IP）

**飞书权限要求：**
- `im:message` — 读取消息
- `im:message:send_as_bot` — 发送消息

### 4. Vue 3 前端 UI

**窗口结构：**

| 窗口 | 属性 | 内容 |
|------|------|------|
| 浮窗 | 200x60px, always-on-top, 可拖动, 无边框 | Agent 状态灯 + 名称 + session 数 + 最近消息摘要 |
| 主窗口 | 600x400px, 普通窗口 | 左栏 session 列表 + 右栏消息流 |
| 权限弹窗 | 350x180px, 浮窗附近弹出, modal | 工具名 + 参数 + 允许/拒绝/始终允许 |

**消息标记：**

每条消息显示来源标签：
- `CLI` (灰色) — 桌面终端发送的 prompt
- `📱 飞书` (绿色) — 手机飞书发送
- `Agent` (蓝色) — Agent 输出/回复

**飞书 Slash 命令（在飞书中使用）：**

| 命令 | 说明 |
|------|------|
| `/status` | 查看当前绑定的 session 状态 |
| `/sessions` | 列出所有活跃 session |
| `/bind <session>` | 绑定当前 chat 到指定 session |
| `/switch <session>` | 切换绑定 |
| `/mute` | 暂停推送 |
| `/unmute` | 恢复推送 |
| `/help` | 帮助 |

非命令文本直接注入绑定的 session。

---

## 项目结构

```
cc-remote-seamless/
├── src-tauri/                    # Tauri Rust 后端
│   ├── src/
│   │   ├── main.rs              # 入口
│   │   ├── engine/
│   │   │   ├── mod.rs           # Engine 路由核心
│   │   │   ├── router.rs        # binding 管理
│   │   │   └── store.rs         # 消息持久化 (SQLite)
│   │   ├── agent/
│   │   │   ├── mod.rs           # AgentConnector trait
│   │   │   └── claude_code.rs   # Claude Code 实现
│   │   ├── platform/
│   │   │   ├── mod.rs           # IMPlatform trait
│   │   │   └── feishu.rs        # 飞书实现 (sidecar 通信)
│   │   ├── hook/
│   │   │   └── server.rs        # HTTP hook receiver
│   │   └── window/
│   │       └── mod.rs           # 窗口管理 (浮窗/主窗口/弹窗)
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                          # Vue 3 前端
│   ├── App.vue
│   ├── views/
│   │   ├── FloatingWidget.vue   # 浮窗
│   │   ├── MainWindow.vue       # 主窗口
│   │   └── PermissionPopup.vue  # 权限弹窗
│   ├── components/
│   │   ├── SessionList.vue
│   │   ├── MessageFlow.vue
│   │   └── StatusBadge.vue
│   └── stores/
│       ├── sessions.ts          # session 状态管理
│       └── messages.ts          # 消息存储
├── crates/
│   └── cc-remote-pty/           # PTY Proxy (独立 Rust crate/binary)
│       ├── src/
│       │   ├── main.rs
│       │   ├── pty.rs           # PTY 管理
│       │   ├── socket.rs        # Unix Socket / Named Pipe
│       │   └── protocol.rs      # JSON Lines 协议
│       └── Cargo.toml
├── sidecar/
│   └── feishu-gateway/          # Go sidecar
│       ├── main.go
│       ├── ws.go                # WebSocket 长连接
│       ├── api.go               # 消息发送 API
│       └── go.mod
├── package.json                  # 前端依赖
├── vite.config.ts
└── docs/
```

---

## 输入冲突处理

| 场景 | 行为 |
|------|------|
| Agent idle + 无桌面活动 | 飞书消息立即注入 |
| Agent busy | 飞书消息排队，idle 时自动 flush |
| 桌面正在输入（2s 窗口）| 飞书消息排队，等待桌面静止 |
| 权限等待中 | 飞书消息排队，权限确认后 flush |

冲突检测基于：
- PTY Proxy 的 terminal_active 信号
- Hook 的 SessionState (idle/busy)

---

## 配置

**应用配置文件：** `~/.cc-remote/config.toml`

```toml
[general]
hook_port = 23399
auto_start = true

[feishu]
app_id = "cli_xxx"
app_secret = "sec_xxx"

[ui]
float_position = "top-right"
theme = "dark"
permission_timeout_sec = 60

[agents.claude]
type = "claude-code"
pty_binary = "cc-remote-pty"
socket_dir = "/tmp"
```

---

## 构建与分发

| 平台 | PTY Proxy | Go Sidecar | Tauri App |
|------|-----------|------------|-----------|
| macOS (arm64/x64) | `cc-remote-pty` | `feishu-gateway` | `.dmg` |
| Windows (x64) | `cc-remote-pty.exe` | `feishu-gateway.exe` | `.msi` |
| Linux (x64) | `cc-remote-pty` | `feishu-gateway` | `.AppImage` / `.deb` |

PTY Proxy 和 Go Sidecar 作为 Tauri sidecar 打包进安装包，无需用户额外安装。

---

## 非目标（V2 不做）

- 飞书交互卡片（仅纯文本，卡片留给未来版本）
- 多用户协作（单用户场景）
- Agent 会话创建（只附着已运行的 session）
- 消息加密/E2E
