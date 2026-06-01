# CC Remote Seamless — 技术设计文档

## 1. 项目定位

CC Remote Seamless 是一个跨平台桌面应用，让用户离开电脑时能通过飞书或 Telegram 与正在运行的 Claude Code / Pi Agent CLI session 无缝交互，且桌面终端体验完全无损。

**核心理念：** IM 是辅助通道（桌面为主、手机为辅），而非唯一界面。

**与同类项目的关键差异：**

| 维度 | cc-connect | 本项目 |
|------|-----------|--------|
| Agent 连接方式 | 启动新会话 | 附着到已运行的 session（PTY 代理） |
| 终端体验 | 输出仅在 IM 可见，终端格式丧失 | 桌面终端完全无损 |
| 适用场景 | 纯远程操作 | 离开电脑时临时远程，回来后继续桌面操作 |

---

## 2. 技术栈

| 层 | 技术 | 版本 | 选型理由 |
|----|------|------|----------|
| 桌面框架 | Tauri 2 | 2.x | 跨平台、体积小、Rust 性能、原生窗口管理 |
| 后端核心 | Rust | 2021 edition | 内存安全、高性能 IPC、portable-pty 生态 |
| 前端 | Vue 3 + TypeScript + Vite | Vue 3.x | 轻量、组合式 API、类型安全 |
| 状态管理 | Pinia | 2.x | Vue 3 官方推荐，TypeScript 友好 |
| PTY 代理 | portable-pty | 0.8 | 跨平台 PTY 抽象（Windows ConPTY / Unix forkpty） |
| 飞书连接 | Go sidecar + oapi-sdk-go/v3 | v3.4.3 | 飞书官方 SDK，WebSocket 长连接模式 |
| Telegram | reqwest (Rust HTTP) | — | Bot API long polling + sendMessage |
| HTTP 框架 | axum | 0.7 | 轻量异步 HTTP，用于 Hook Server |
| 数据持久化 | rusqlite (SQLite) | 0.31 | 嵌入式数据库，零配置 |
| 异步运行时 | tokio | 1.x | Rust 异步标准选择 |
| IPC 协议 | JSON Lines | — | 简单、可读、流式解析 |

---

## 3. 系统架构

```
┌─────────────────────────────────────────────────────────────┐
│  Tauri App (Rust core + Vue 3 webview)                      │
│                                                             │
│  ┌──────────┐  ┌──────────────┐  ┌────────────────────┐    │
│  │ 浮窗 UI  │→ │  主窗口 UI   │  │  Rust Engine       │    │
│  │(浮窗图标)│  │(消息/权限)   │  │  ├ AgentConnector  │    │
│  └──────────┘  └──────────────┘  │  ├ IMPlatform      │    │
│  ┌──────────┐                    │  ├ Router/Store    │    │
│  │ 设置窗口 │                    │  ├ Slash Commands  │    │
│  └──────────┘                    └───────┬────────────┘    │
└───────────────────────────────────────────│─────────────────┘
                    ┌───────────────────────┼──────────────┐
                    │                       │              │
                    ▼                       ▼              ▼
        ┌───────────────────┐  ┌─────────────────┐  ┌──────────────┐
        │  Hook Server      │  │  Go Sidecar     │  │  Telegram    │
        │  (axum, 内嵌)     │  │  (飞书 WS)      │  │  Bot API     │
        │                   │  │                 │  │  (long poll) │
        │  · 21 个端点      │  │  · 收发消息     │  │              │
        │  · 权限阻塞等待   │  │  · 发送卡片     │  │              │
        └────────┬──────────┘  └────────┬────────┘  └──────┬───────┘
                 │                      │                   │
                 ▼                      ▼                   ▼
           Claude Code /            飞书云端            Telegram 云端
           Pi Agent                     │                   │
          (Hook events)            手机飞书            手机 Telegram
```

---

## 4. 核心模块设计

### 4.1 AgentConnector trait

**文件：** `src-tauri/src/agent/mod.rs`

```rust
#[async_trait]
pub trait AgentConnector: Send + Sync {
    fn id(&self) -> &str;
    async fn discover_sessions(&self) -> Vec<SessionInfo>;
    async fn inject_input(&self, session_id: &str, text: &str) -> Result<(), String>;
    fn subscribe(&self, sender: EventSender);
    fn rediscover(&self) {}
}
```

**设计要点：**
- `Send + Sync` 确保跨线程安全
- `discover_sessions()` 支持动态发现（扫描 socket 文件）
- `inject_input()` 通过 Unix Socket / Named Pipe 写入
- `subscribe()` 使用 tokio mpsc unbounded channel 推送事件
- `rediscover()` 重新扫描已运行的 session（默认空实现），由 `/radar` 命令触发

**Session 状态机：**
```
Idle ──(UserPromptSubmit)──→ Busy ──(Stop)──→ Idle
  │                           │
  └──(PreToolUse)──→ WaitingPermission ──(允许/拒绝)──→ Busy/Idle
```

**实现：**
- `ClaudeCodeConnector` (`src-tauri/src/agent/claude_code.rs`) — 通过 Hook Server 接收生命周期事件
- `PiConnector` (`src-tauri/src/agent/pi.rs`) — 通过独立 Hook 端点 `/pi/*` 接收事件

### 4.2 IMPlatform trait

**文件：** `src-tauri/src/platform/mod.rs`

```rust
#[async_trait]
pub trait IMPlatform: Send + Sync {
    fn id(&self) -> &str;
    async fn connect(&mut self) -> Result<(), String>;
    async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String>;
    fn subscribe(&self, sender: MessageSender);
    async fn disconnect(&mut self);
}
```

**实现：**
- `FeishuPlatform` (`src-tauri/src/platform/feishu.rs`) — 通过 Go sidecar stdio JSON Lines 通信
- `TelegramPlatform` (`src-tauri/src/platform/telegram.rs`) — 直接调用 Telegram Bot API（long polling + sendMessage）

### 4.3 Engine 路由核心

**文件：** `src-tauri/src/engine/mod.rs`

```rust
pub struct Engine {
    agents: HashMap<String, Arc<dyn AgentConnector>>,
    platforms: HashMap<String, Arc<dyn IMPlatform>>,
    agent_platforms: HashMap<String, Vec<String>>,  // agent_id → [platform_id]
    platform_chat_ids: HashMap<String, String>,     // platform_id → chat_id
    pub bindings: Arc<BindingStore>,   // chat_id ↔ (agent_id, session_id)
    pub messages: Arc<MessageStore>,   // SQLite 持久化
    pub permission_waiters: Option<PermissionWaiters>,
    idle_sound: String,
    permission_sound: String,
}
```

**路由逻辑：**
1. IM 消息到达 → 查 BindingStore → 找到 (agent, session) → 执行 slash 命令或 `agent.inject_input()`
2. Agent 输出产生 → 查 agent_platforms 映射 → 找到对应 platform → `platform.send_text()`
3. 无 binding 时 → 自动绑定最近活跃的 session（通过 Hook 事件触发）
4. 多平台路由 → 每个 agent 可配置多个 platform，输出同时转发到所有关联平台

### 4.4 PTY Proxy

**Crate：** `crates/cc-remote-pty/`（独立 Rust binary）

包裹 Agent CLI 进程，提供 IPC 通道用于输入注入和输出转发。

**IPC 通信：**
- macOS / Linux: Unix Domain Socket (`/tmp/cc-remote-{agent}-{session_id}.sock`)
- Windows: Named Pipe (`\\.\pipe\cc-remote-{agent}-{session_id}`)

**协议（JSON Lines）：**

```json
{"type": "input", "data": "hello\n"}
{"type": "output", "data": "Agent response..."}
{"type": "resize", "cols": 120, "rows": 40}
{"type": "state", "waiting": true}
{"type": "exit", "code": 0}
```

**启动方式：**
```bash
cc-remote-pty --agent claude -- claude
```

**空闲检测：** 500ms 无 stdout 输出 → 发送 `{"type": "state", "waiting": true}`

### 4.5 Hook Server

**文件：** `src-tauri/src/hook/server.rs`

内嵌 axum HTTP Server，接收 Claude Code 生命周期事件。

**端口：** `localhost:23399`（可通过环境变量 `CC_REMOTE_HOOK_PORT` 配置）

| 端点 | Hook 事件 | 动作 |
|------|-----------|------|
| `POST /hook/session-start` | SessionStart | 注册 session，状态 → Idle |
| `POST /hook/session-end` | SessionEnd | 移除 session |
| `POST /hook/stop` | Stop | 状态 → Idle，推送输出到 IM |
| `POST /hook/prompt` | PromptSubmit | 状态 → Busy，转发 prompt 到 IM |
| `POST /hook/pre-tool` | PreToolUse | 状态 → Busy |
| `POST /hook/post-tool` | PostToolUse | 状态 → Busy |
| `POST /hook/post-tool-failure` | PostToolUseFailure | 状态 → Busy |
| `POST /hook/stop_failure` | StopFailure | 状态 → Idle |
| `POST /hook/subagent_start` | SubagentStart | 状态 → Busy |
| `POST /hook/subagent_stop` | SubagentStop | 状态 → Busy |
| `POST /hook/notification` | Notification | 状态 → Idle |
| `POST /hook/elicitation` | Elicitation | 状态 → WaitingPermission，弹出问题窗 |
| `POST /hook/pre_compact` | PreCompact | 状态 → Busy |
| `POST /hook/post_compact` | PostCompact | 状态 → Idle |
| `POST /permission` | PermissionRequest | 状态 → WaitingPermission，弹窗 + 推送卡片（阻塞等待响应） |
| `POST /pi/session_start` | PiSessionStart | Pi Agent session 注册 |
| `POST /pi/session_end` | PiSessionEnd | Pi Agent session 移除 |
| `POST /pi/input` | PiInput | Pi Agent 输入转发 |
| `POST /pi/pre_tool` | PiPreToolUse | Pi Agent 工具调用 |
| `POST /pi/post_tool` | PiPostToolUse | Pi Agent 工具完成 |
| `POST /pi/permission` | PiPermissionRequest | Pi Agent 权限请求 |
| `POST /pi/stop` | PiStop | Pi Agent 停止，状态 → Idle |
| `POST /pi/agent_start` | PiAgentStart | Pi Agent 开始处理，状态 → Busy |
| `POST /pi/pre_compact` | PiPreCompact | Pi Agent 上下文压缩中 |
| `POST /pi/post_compact` | PiPostCompact | Pi Agent 上下文压缩完成 |

### 4.6 Go Sidecar (feishu-gateway)

**目录：** `sidecar/feishu-gateway/`

**职责：** 维持飞书 WebSocket 长连接，收发消息和卡片。

**通信方式：** Tauri sidecar 通过 stdin/stdout JSON Lines 双向通信。

**依赖：**
- `github.com/larksuite/oapi-sdk-go/v3` v3.4.3
- `github.com/larksuite/oapi-sdk-go/v3/ws` — WebSocket 长连接

**Tauri → Sidecar 命令：**

```json
{"type": "connect", "app_id": "cli_xxx", "app_secret": "sec_xxx"}
{"type": "send_text", "chat_id": "oc_xxx", "text": "Agent 输出内容"}
{"type": "send_card", "chat_id": "oc_xxx", "card": {...}}
{"type": "disconnect"}
```

**Sidecar → Tauri 事件：**

```json
{"type": "connected"}
{"type": "message_received", "chat_id": "oc_xxx", "text": "用户消息", "sender": "user_xxx", "message_id": "msg_xxx"}
{"type": "disconnected", "reason": "network error"}
{"type": "error", "message": "token expired"}
```

### 4.7 Telegram Platform

**文件：** `src-tauri/src/platform/telegram.rs`

**职责：** 通过 Telegram Bot API 收发消息（无需 sidecar）。

**实现方式：**
- 接收消息：Long Polling（`getUpdates?timeout=30`）
- 发送消息：`sendMessage` API（支持 Markdown 格式）
- 发送卡片：将飞书卡片格式转换为纯文本 Markdown 发送

**配置：**
```toml
[platforms.my-telegram]
type = "telegram"
bot_token = "123456:ABC-DEF..."
chat_id = "987654321"
```

### 4.8 多平台路由

**文件：** `src-tauri/src/engine/mod.rs`

每个 Agent 可配置关联多个 IM 平台。当 Agent 产生输出时，Engine 根据 `agent_platforms` 映射将消息转发到所有关联平台。

**配置示例：**
```toml
[agents.claude-code]
platforms = ["feishu-main", "telegram-personal"]

[agents.pi]
platforms = ["feishu-main"]
```

### 4.9 Vue 3 前端

**窗口结构：**

| 窗口 | 尺寸 | 属性 | 路由 |
|------|------|------|------|
| 浮窗 | 54x54 / 330x225px | always-on-top, 无边框, 可拖动 | `?view=float` |
| 主窗口 | 600x400px | 普通窗口 | `?view=main` |
| 权限弹窗 | 350x200px | 浮窗附近弹出 | `?view=permission` |
| 设置窗口 | 500x400px | 普通窗口 | `?view=settings` |

**组件：**
- `FloatingWidget.vue` — 浮窗图标 + 状态颜色 + 展开后显示 session 列表和消息
- `MainWindow.vue` — 左栏 SessionList + 右栏 MessageFlow
- `PermissionPopup.vue` — 工具名 + 参数 + 允许/拒绝/始终允许 + Question 回复
- `SettingsWindow.vue` — 图标选择、音效设置、配置文件编辑、关于页
- `StatusBadge.vue` — 状态颜色指示器
- `SessionList.vue` — session 列表
- `MessageFlow.vue` — 消息流（标记 CLI / IM / Agent 来源）

**状态管理（Pinia）：**
- `stores/sessions.ts` — session 列表、活跃 session、状态
- `stores/messages.ts` — 消息历史
- `stores/settings.ts` — 浮窗图标、音效、主题设置

**Tauri IPC 命令：**
- `get_sessions` — 获取所有活跃 session
- `get_messages` / `get_all_messages` — 获取消息历史
- `bind_session` — 绑定 chat_id 到 session
- `inject_input` — 向 session 注入输入
- `respond_permission` — 响应权限请求
- `pin_session` — 固定 session 绑定（不被自动切换覆盖）
- `get_active_session` — 获取当前活跃 session
- `open_settings` — 打开设置窗口
- `open_terminal` — 打开终端定位到 session
- `play_sound` / `list_available_sounds` — 音效播放
- `fix_svg_icons` — 修复 SVG 图标颜色

---

## 5. 数据流详解

### 5.1 IM 消息 → Agent 注入

```
手机 IM (飞书/Telegram) → 平台云端 → Platform Connector → Engine
  → Slash 命令检测（以 / 开头则执行命令并回复）
  → BindingStore 查 chat_id → 找到 (agent_id, session_id)
  → AgentConnector.inject_input()
  → 注入到 Agent CLI（AppleScript / PTY Socket）
```

### 5.2 Agent 输出 → IM 推送

```
Claude Code Hook (Stop event with response)
  → Hook Server 收到 → 存入 MessageStore
  → 查 agent_platforms 映射 → 找到关联的 platform_id 列表
  → 对每个 platform: platform.send_text(chat_id, text)
  → 飞书: Go Sidecar → 飞书 API → 手机飞书
  → Telegram: HTTP API → Telegram Bot → 手机 Telegram
```

### 5.3 权限确认流程

```
Agent CLI 调用工具 → Claude Code 触发 PreToolUse Hook
  → curl POST http://localhost:23399/permission
  → Hook Server 收到 → 创建 PermissionWaiter（阻塞等待响应）
  → Engine 触发 PermissionRequest 事件
  → 桌面：打开 PermissionPopup 窗口 + 播放权限音效
  → IM：推送权限卡片到所有关联平台
  → 用户在桌面或 IM 操作（/allow, /deny, /always）
  → PermissionWaiter 收到响应 → HTTP 返回给 Claude Code
```

### 5.4 Question（Elicitation）流程

```
Agent CLI 调用 AskUserQuestion → Claude Code 触发 Elicitation Hook
  → curl POST http://localhost:23399/hook/elicitation
  → Hook Server 收到 → 创建 PermissionWaiter
  → 桌面：打开 PermissionPopup 窗口（显示问题和选项）
  → IM：推送紫色卡片（问题内容 + 选项列表）
  → 用户回复 /answer N → 解析选项 → 返回给 Claude Code
```

---

## 6. 输入冲突处理

| 场景 | 行为 |
|------|------|
| Agent idle + 无桌面活动 | 飞书消息立即注入 |
| Agent busy | 飞书消息排队，idle 时自动 flush |
| 桌面正在输入（2s 窗口）| 飞书消息排队，等待桌面静止 |
| 权限等待中 | 飞书消息排队，权限确认后 flush |

检测机制：
- PTY Proxy 的 `terminal_active` 信号（检测本地 stdin 活动）
- Hook Server 的 SessionState（idle/busy/waiting）

---

## 7. 消息持久化

使用 SQLite（rusqlite bundled），表结构：

```sql
CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    source TEXT NOT NULL,       -- 'cli' | 'feishu' | 'agent'
    content TEXT NOT NULL,
    timestamp INTEGER NOT NULL
);
CREATE INDEX idx_session ON messages(session_id);
```

存储路径：`~/.cc-remote/messages.db`

---

## 8. 配置

**应用配置文件：** `~/.cc-remote/config.toml`

```toml
[general]
hook_port = 23399

[general.sounds]
idle = "Glass"           # Agent 空闲时播放的音效
permission = "Hero"      # 权限请求时播放的音效

[platforms.feishu-main]
type = "feishu"
app_id = "cli_xxx"
app_secret = "sec_xxx"
chat_id = "oc_xxx"

[platforms.telegram-personal]
type = "telegram"
bot_token = "123456:ABC-DEF..."
chat_id = "987654321"

[agents.claude-code]
platforms = ["feishu-main", "telegram-personal"]

[agents.pi]
platforms = ["feishu-main"]
```

**配置结构：**

| 字段 | 类型 | 说明 |
|------|------|------|
| `general.hook_port` | u16 | Hook Server 端口，默认 23399 |
| `general.sounds.idle` | String | 空闲音效名（对应 `resources/sounds/` 下的 .aiff 文件） |
| `general.sounds.permission` | String | 权限音效名 |
| `platforms.<id>.type` | "feishu" / "telegram" | 平台类型 |
| `platforms.<id>.chat_id` | String | 绑定的聊天 ID |
| `agents.<id>.platforms` | [String] | 该 Agent 关联的平台 ID 列表 |

---

## 9. 项目结构

```
cc-remote-seamless/
├── Cargo.toml                    # Workspace 根
├── src-tauri/                    # Tauri Rust 后端
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── resources/
│   │   ├── icons/                # 浮窗图标 SVG
│   │   └── sounds/               # 音效文件 (.aiff)
│   └── src/
│       ├── main.rs               # 入口
│       ├── lib.rs                # 应用初始化、setup、事件分发
│       ├── commands.rs           # Tauri IPC 命令（16 个）
│       ├── config.rs             # 配置加载（TOML）
│       ├── agent/
│       │   ├── mod.rs            # AgentConnector trait
│       │   ├── claude_code.rs    # Claude Code 实现
│       │   └── pi.rs             # Pi Agent 实现
│       ├── platform/
│       │   ├── mod.rs            # IMPlatform trait
│       │   ├── feishu.rs         # 飞书 sidecar 通信
│       │   └── telegram.rs       # Telegram Bot API
│       ├── engine/
│       │   ├── mod.rs            # Engine 路由核心
│       │   ├── router.rs         # BindingStore（绑定管理）
│       │   ├── slash.rs          # Slash 命令解析执行
│       │   └── store.rs          # MessageStore (SQLite)
│       ├── hook/
│       │   ├── mod.rs
│       │   ├── server.rs         # axum HTTP Hook Server（21 个端点）
│       │   └── installer.rs      # Claude Code Hook 自动安装
│       ├── pty/
│       │   └── mod.rs
│       └── window/
│           └── mod.rs            # 窗口管理（浮窗、权限弹窗）
├── crates/
│   └── cc-remote-pty/            # PTY Proxy 独立 binary
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs           # clap CLI 入口
│           ├── protocol.rs       # JSON Lines 编解码
│           ├── pty.rs            # portable-pty 包装
│           └── ipc.rs            # Unix Socket IPC
├── sidecar/
│   └── feishu-gateway/           # Go sidecar
│       ├── go.mod
│       ├── main.go               # stdio 循环 + 命令分发
│       ├── client.go             # 飞书 SDK 封装（WS + API）
│       ├── protocol.go           # Command/Event JSON 类型
│       └── protocol_test.go      # 协议测试
├── src/                          # Vue 3 前端
│   ├── main.ts
│   ├── App.vue                   # 路由分发（view 参数）
│   ├── types.ts
│   ├── stores/
│   │   ├── sessions.ts           # Session 状态管理
│   │   ├── messages.ts           # 消息管理
│   │   └── settings.ts           # 设置（图标、音效）
│   ├── views/
│   │   ├── FloatingWidget.vue    # 浮窗（浮窗图标 + session 列表）
│   │   ├── MainWindow.vue        # 主窗口
│   │   ├── PermissionPopup.vue   # 权限/问题弹窗
│   │   └── SettingsWindow.vue    # 设置页面
│   └── components/
│       ├── icons/                # 浮窗图标组件
│       ├── StatusBadge.vue
│       ├── SessionList.vue
│       └── MessageFlow.vue
├── resource/
│   └── pi-extension/
│       └── cc-remote.ts          # Pi Agent 扩展（自动安装）
├── scripts/
│   └── build.sh                  # 构建脚本
├── package.json
├── vite.config.ts
├── vitest.config.ts
├── tsconfig.json
└── docs/
```

---

## 10. 构建与分发

| 平台 | PTY Proxy | Go Sidecar | Tauri App |
|------|-----------|------------|-----------|
| macOS (arm64/x64) | `cc-remote-pty` | `feishu-gateway` | `.dmg` |
| Windows (x64) | `cc-remote-pty.exe` | `feishu-gateway.exe` | `.msi` |
| Linux (x64) | `cc-remote-pty` | `feishu-gateway` | `.AppImage` / `.deb` |

PTY Proxy 和 Go Sidecar 作为 Tauri sidecar 打包进安装包。

---

## 11. 测试策略

| 层 | 工具 | 覆盖 |
|----|------|------|
| Rust 单元测试 | `cargo test` | protocol 编解码、BindingStore、MessageStore、Hook 解析 |
| Go 单元测试 | `go test` | sidecar 协议编解码 |
| 前端类型检查 | `vue-tsc --noEmit` | TypeScript 类型正确性 |
| 集成测试 | 手动 | 飞书消息 → Agent 注入 → 输出推送全链路 |

---

## 12. 后续方向

- 更多 Agent 接入（Codex、Gemini 等）
- 更多 IM 平台接入（钉钉、企业微信等）
- 飞书交互卡片增强（代码 diff、进度条等）
- E2E 加密 IM 通道
- 多用户支持（不同 chat_id 绑定不同 session）
- PTY 模式自动 Session 发现（扫描 `/tmp/cc-remote-*.sock`）
