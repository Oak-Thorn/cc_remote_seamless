# CC Remote Seamless — 项目级方案设计文档（Solution Design）

> 版本：基于 main 分支当前代码（2026-06-25）
> 范围：整个项目的架构、子系统、数据流、关键实现与设计取舍

---

## 1. 项目概述

**CC Remote Seamless** 是一个桌面应用（Tauri 2 + Vue 3 + Rust，飞书网关用 Go sidecar），用于**远程监控与操控本机运行的 Claude Code / Pi Agent CLI 会话**。用户离开电脑后，可通过飞书或 Telegram 在手机上继续与 Agent 交互：发送 prompt、审批工具权限、回答 AskUserQuestion、查看会话状态。

核心价值：

- **无缝终端体验**：桌面 CLI 保留完整 ANSI 格式与交互，远程控制零侵入。
- **多平台 IM**：同时支持飞书（WebSocket）与 Telegram（Bot API 长轮询）。
- **多 Agent**：Claude Code 与 Pi Agent，可用 `/change` 切换。
- **远程权限审批 / 远程问答**：权限请求与问题以卡片推送到 IM，回复即生效。
- **无需公网 IP**：飞书 WebSocket + Telegram 长轮询均为客户端发起的出站连接。

---

## 2. 技术栈

| 层 | 技术 | 版本 |
|----|------|------|
| 桌面框架 | Tauri | 2（`macos-private-api`、`tray-icon`） |
| 前端 | Vue + Composition API | 3.4 |
| 前端语言 | TypeScript | 5.4 |
| 状态管理 | Pinia | 2.1 |
| 构建 | Vite | 5 |
| 异步运行时 | Tokio | 1 |
| HTTP 服务 | Axum | 0.7 |
| 存储 | rusqlite（bundled） | 0.31 |
| HTTP 客户端 | reqwest | 0.12 |
| 飞书网关 | Go + Larksuite OAPI SDK | Go 1.22 |
| 日志 | tracing + tracing-appender | 0.1 / 0.2 |

---

## 3. 总体架构

```
┌──────────────────────────── 桌面进程 (Tauri 2) ────────────────────────────┐
│                                                                            │
│  前端 (Vue 3 + Pinia)              后端 (Rust)                              │
│  ┌──────────────────┐             ┌────────────────────────────────────┐  │
│  │ FloatingWidget   │  invoke ──▶ │ commands.rs (RPC handlers)         │  │
│  │ MainWindow       │             │                                    │  │
│  │ SettingsWindow   │ ◀── event   │ Engine (EngineState=Arc<Mutex<>>)  │  │
│  │ PermissionPopup  │             │  ├ agents: AgentConnector          │  │
│  └──────────────────┘             │  ├ platforms: IMPlatform           │  │
│                                   │  ├ BindingStore (chat_id↔session)  │  │
│                                   │  ├ MessageStore (SQLite)           │  │
│                                   │  └ PermissionWaiters               │  │
│                                   └───────┬───────────────┬────────────┘  │
│                                           │               │               │
│   ┌───────────────────────────┐   HookEvent          AgentEvent           │
│   │ Hook HTTP Server :23399    │──────┘               │                    │
│   │ (Axum, Claude + Pi 路由)   │                       ▼                    │
│   └───────────────▲───────────┘            ┌──────────────────────┐        │
│                   │ curl POST               │ Agent Connectors     │        │
│                   │                         │  ├ ClaudeCode(AppleScript inject)│
│                   │                         │  └ Pi (HTTP inject)  │        │
│                   │                         └──────────┬───────────┘        │
│                   │                                    │                    │
│   ┌───────────────┴────────────┐          ┌───────────▼──────────┐         │
│   │ Claude Code / Pi CLI hooks │          │ IM Platforms         │         │
│   └────────────────────────────┘          │  ├ Feishu(Go sidecar)│         │
│                                            │  └ Telegram(reqwest) │         │
│                                            └──────────┬───────────┘         │
└───────────────────────────────────────────────────────┼────────────────────┘
                                                         │
                                  ┌──────────────────────┴───────────────────┐
                                  │ Go Sidecar (feishu-gateway)               │
                                  │  stdin ← JSON 命令 / stdout → JSON 事件    │
                                  │  WebSocket ↔ 飞书云                        │
                                  └───────────────────────────────────────────┘
```

四条核心数据通路：

1. **入站 IM → Agent**：IM 消息 → `Engine::handle_im_message` → slash 解析 → 注入 / 回复。
2. **CLI Hook → 状态/输出**：CLI hook `curl` → Hook Server → `HookEvent` → Engine → AgentEvent → 转发到 IM + 前端事件。
3. **权限请求（阻塞）**：CLI permission hook → Hook Server 创建 waiter 并阻塞 → 推送桌面弹窗 + IM 卡片 → 用户响应 → oneshot 唤醒 → HTTP 返回决策。
4. **前端 ↔ 后端**：`invoke` 调命令；`emit`/`listen` 推送实时更新。

---

## 4. 启动流程（`src-tauri/src/lib.rs::run()`）

1. **日志初始化**：建 `~/.cc-remote/logs/`，`logging::init(false, dir)` 挂载双层 subscriber（stdout + 门控文件层），`cleanup_old_logs(7)` 清理过期，安装 panic hook（panic 写入日志）。
2. **启动计时**：`engine::init_start_time()` —— 启动后 5 秒内抑制音效，避免冷启动连环响铃。
3. **加载配置**：`config::load()` 读取 `~/.cc-remote/config.toml`，得到平台、Agent 路由、hook 端口（默认 **23399**）。
4. **安装 Hook / 扩展**：`install_claude_hooks(port)` 写入 `~/.claude/settings.json`；`install_pi_extension()` 安装 Pi 扩展。
5. **Tauri setup 闭包**：
   - 创建 **Engine**（内存 SQLite MessageStore）。
   - 注册 Agent：**ClaudeCodeConnector**（监控 `~/.claude/sessions`）+ **PiConnector**（监控 `~/.cc-remote/pi-sessions`）。
   - 注册平台：**FeishuPlatform**（spawn Go sidecar）+ **TelegramPlatform**（长轮询）。
   - 依据配置建立 agent→platform 路由表与 platform→chat_id 映射。
   - spawn **Hook Server**（Axum，端口 23399）。
   - spawn **IM 消息分发**（IMMessage → `Engine::handle_im_message`）。
   - spawn **Hook 事件分发**（自动绑定会话、转发到 Agent/平台、弹权限窗）。
   - spawn **Agent 事件处理**（Output/StateChange/PermissionRequest → 平台 + Tauri 事件）。
   - spawn **reconcile 循环**（每 5 秒：重新发现会话、清理失活会话）。
   - **初始绑定**：每个 Agent 最近的会话自动绑定到所配置的 chat_id。
6. **注册 Tauri 命令**：暴露 20+ RPC handler。
7. **状态管理**：`app.manage(engine)`、`app.manage(permission_waiters)`。

---

## 5. 后端子系统

### 5.1 Engine（`engine/mod.rs`）

中央协调器，`EngineState = Arc<Mutex<Engine>>` 供各异步任务与 Tauri 命令共享。

关键字段：`agents`、`platforms`、`agent_platforms`（路由表）、`platform_chat_ids`、`bindings`（Arc\<BindingStore\>）、`messages`（Arc\<MessageStore\>）、`permission_waiters`、`last_binding_change`、`idle_sound` / `permission_sound` / `resource_dir`。

关键方法：

- `handle_im_message(IMMessage)`：经 slash.rs 分发；`/p` 注入、权限响应解析、普通文本提示用 `/p`。
- `handle_agent_event(AgentEvent)`：输出落库 MessageStore，按 mute 规则转发到平台，状态变化播放音效。
- `forward_to_platforms` / `forward_card_to_platforms`：仅当绑定匹配该会话时，向该 Agent 配置的所有平台广播文本 / 卡片。
- `get_sessions()`：聚合所有 Agent 的活跃会话。

**音效路径解析**（多级回退）：`~/.cc-remote/sounds/` → Tauri bundle 资源 → `CARGO_MANIFEST_DIR` 资源 → macOS 系统音 `/System/Library/Sounds/`。

### 5.2 BindingStore（`engine/router.rs`）

`RwLock<HashMap>` 维护 chat_id ↔ 会话的双向映射。`Binding { agent_id, session_id, muted, pinned, last_output }`。

- `bind` / `bind_pinned`：建立绑定（保留既有 mute 标记）；pinned 绑定**抗自动改绑**。
- `find_chat_for_session`：反查；`get_active_session_id`：供 UI 自动注入。
- `set_muted` / `is_muted`；`store_last_output*` / `get_last_output`：缓存末次输出供 `/full`。

**Pin 语义**：hook 事件到来时，仅未 pin 的绑定会自动改绑到新会话，让用户可"钉住"某个会话。

### 5.3 Slash 命令（`engine/slash.rs`）

返回 `SlashResult`：`Reply`（回平台）/ `Inject`（注入 Agent）/ `BindingChanged`（回复+更新 UI）/ `Noop`（直接解析 waiter）。

共 **18 个命令**：

| 命令 | 作用 |
|------|------|
| `/help` | 命令帮助 |
| `/status` | 当前绑定会话状态、Agent、CWD、mute |
| `/list` | 列出所有 Agent 的活跃会话（带状态徽标） |
| `/switch <id>` | 按 ID（前缀匹配）绑定会话 |
| `/change [agent]` | 切换 Agent；无参列出可用项 |
| `/p <text>` | 向绑定会话注入 prompt（先检查非 busy） |
| `/t <text>` | 调试：仅记日志，回 ok |
| `/mute` `/unmute` | 抑制 / 恢复输出转发 |
| `/full` | 取末次缓存输出（应对实时截断） |
| `/allow` `/deny` `/always` | 权限审批（解析 waiter 或注入 y/n/a） |
| `/answer <N> [N2] [custom]` | 解析多选问题或自定义输入 |
| `/clear` | 清空 Claude 输入框（注入 Ctrl+C） |
| `/stop` | 中断会话（注入 ESC） |
| `/skill <kw>` | 搜索 `~/.claude/skills/` 与插件缓存中的 skill |
| `/radar` | 强制重新发现并列出全部会话 |

**权限解析流**：slash 侧依当前绑定会话在 `permission_waiters`（key 为 `request_id`，形如 `{session}:{uuid}` 或 `eli:{session}:{uuid}`）中匹配对应 waiter；命中则经 oneshot 回送响应（唤醒阻塞中的 HTTP handler）；未命中（用户在桌面 UI 而非等 HTTP）则注入按键 y/n/a。

### 5.4 MessageStore（`engine/store.rs`）

SQLite（`Mutex<Connection>`）存储每会话聊天历史。表 `messages(id, session_id, source, text, timestamp)`，`source` ∈ cli/agent/feishu/telegram，索引 `idx_session`。提供 `open` / `open_in_memory` / `insert` / `get_by_session` / `get_all` / `clear_by_session`（均按 timestamp 倒序）。当前 `lib.rs` 以 `":memory:"` 打开（进程级内存库，重启不留存）；`open(path)` 支持文件持久化，为后续留出空间。

### 5.5 Hook Server（`hook/server.rs`）

Axum HTTP 服务（默认端口 23399），接收 Claude Code 与 Pi 的 hook 回调，发出 `HookEvent`，并实现**权限阻塞等待**。

- **Claude Code 路由**（POST）：`/hook/session-start`、`/session-end`、`/prompt`、`/pre-tool`、`/post-tool`、`/post-tool-failure`、`/stop`、`/stop_failure`、`/subagent_start`、`/subagent_stop`、`/notification`、`/elicitation`、`/pre_compact`、`/post_compact`，以及 `/permission`（阻塞）。
- **Pi 路由**：`/pi/session_start`（含 pid + inject_port）、`/pi/session_end`、`/pi/input`、`/pi/pre_tool`、`/pi/post_tool`、`/pi/permission`（阻塞）、`/pi/stop`、`/pi/agent_start`、`/pi/pre_compact`、`/pi/post_compact`。
- `/health`：健康检查。

**权限阻塞机制**：

```
PermissionWaiters = Arc<Mutex<HashMap<request_id, PermissionWaiterEntry>>>
PermissionWaiterEntry { sender: oneshot::Sender<PermissionResponse>, suggestions }
```

handler 生成 `request_id`（`{session}:{uuid}`，Elicitation 为 `eli:{session}:{uuid}`）、插入 waiter、广播事件（前端弹窗 + IM 卡片），随后 `tokio::time::timeout(590s, rx)` 阻塞等待。前端 `respond_permission` 或 IM 命令解析 waiter → oneshot 回送 → handler 恢复 → 返回 HTTP 决策。超时则自动 deny。`/permission`、`/pi/permission`、`/elicitation` 三处 handler 均为此模式。

`PermissionResponse { behavior: "allow"|"deny"|"allowAlways", message, updated_permissions }`。

### 5.6 Hook 安装器（`hook/installer.rs`）

启动时幂等地把 cc-remote hook 注入 `~/.claude/settings.json`。每条 hook 为 `curl` command 类型，POST 到 `http://127.0.0.1:{port}{path}`，并带 `_owner: "cc-remote-seamless"` 哨兵字段，用于重装时清理旧条目、保留第三方 hook。`PermissionRequest` 设 600s 超时，其余无超时。仅在内容变化时写盘。

### 5.7 Agent 抽象（`agent/`）

`SessionState` ∈ `Idle` / `Busy` / `WaitingPermission`。`SessionInfo { id, agent, state, working_dir }`。`AgentEvent` ∈ `StateChange` / `Output` / `PermissionRequest`。

`AgentConnector` trait（async）：`id`、`discover_sessions`、`inject_input`、`subscribe`、`rediscover`。

- **ClaudeCodeConnector**（`claude_code.rs`）：扫描 `~/.claude/sessions/*.json`（含 sessionId/cwd/pid/status），`kill -0` 判活；hook 事件驱动状态机（仅状态真正变化时发 `StateChange`）；注入用 **AppleScript**（Ctrl+C 清空、ESC 停止、其余按文本注入）；每 5 秒 `reconcile_from_files`。
- **PiConnector**（`pi.rs`）：扫描 `~/.cc-remote/pi-sessions/*.json`（含 pid/cwd/inject_port）；注入优先经 **HTTP POST**（reqwest）到 `http://127.0.0.1:{inject_port}`，无 inject_port 时回退 AppleScript（按 tty 定位）；`reap_stale` 每 5 秒清理空闲 >300s 或进程已死的会话。

### 5.8 平台抽象（`platform/`）

`IMMessage { chat_id, text, sender, platform, timestamp }`。`IMPlatform` trait（async）：`id`、`connect`、`send_text`、`send_card`（默认 no-op）、`subscribe`、`disconnect`。

- **FeishuPlatform**（`feishu.rs`）：spawn Go sidecar，stdin/stdout JSON 行协议；命令 `connect`/`send_text`/`send_card`/`disconnect`，事件 `connected`/`message_received`/`error`。把飞书 SDK 复杂度隔离在 Go 侧。
- **TelegramPlatform**（`telegram.rs`）：reqwest 长轮询 `getUpdates?timeout=30`，按 chat_id 过滤；`sendMessage` 发文本；网络/解析错误 5s 退避重试；无富卡片。

### 5.9 窗口管理（`window/mod.rs`）

`WebviewWindowBuilder` 创建三类窗口：`show_permission_popup`（`/?view=permission` 带 tool/input/session/request_id 参数）、`open_settings_window`（`/?view=settings`）、`open_main_window`（`/?view=main`）。浮窗为默认视图。

### 5.10 配置（`config.rs`）

`~/.cc-remote/config.toml`：

```toml
[general]
hook_port = 23399
[general.sounds]
idle = "Glass"
permission = "Hero"
[platforms.<id>]      # type = "feishu" | "telegram"
[agents.<id>]         # platforms = ["<platform_id>", ...]
```

解析失败 / 文件缺失时回退默认配置，不阻断启动。

### 5.11 其它

- `pty/mod.rs`：当前为占位（1 行），PTY socket IPC 为预留方向。
- `commands.rs`：见 §7 命令清单。
- `start_feishu_register`：spawn `scripts/feishu-register.mjs`（Node），按 stdout JSON 行 emit `feishu-register-qr` / `-done` / `-error`。

---

## 6. 前端子系统

### 6.1 入口与路由

`main.ts` 创建 Vue + Pinia。`App.vue` 依据 URL `?view=` 分发：`float`（默认，FloatingWidget）/ `main`（MainWindow）/ `settings`（SettingsWindow）/ `permission`（PermissionPopup）。

### 6.2 视图

- **FloatingWidget.vue**：置顶浮窗。折叠态显示状态色图标（绿=Idle/橙=Busy/红=WaitingPermission）；悬停展开显示会话列表 + 消息；5 秒无操作自动收起。调用 `pin_session`/`open_settings`/`open_terminal`/`set_sound_preference`/`read_icon_svg`；监听 `sessions-updated`/`messages-updated`/`binding-changed`/`floating-icon-changed`。
- **MainWindow.vue**：双栏 SessionList + MessageFlow；监听 `messages-updated`/`sessions-updated`。
- **SettingsWindow.vue**：五个标签 —— Floating Icon / Sound / **IMConfig** / **Logs** / About。含飞书扫码注册流（`start_feishu_register` + QR 事件）、自定义图标/音效目录、config 查看、日志目录与落盘开关。
- **PermissionPopup.vue**：两种模式 —— 权限模式（工具名 + 输入字段，危险工具 Bash/Write/Edit/NotebookEdit 高亮，Allow/Always/Deny）与问题模式（AskUserQuestion，单/多选 + Other 自定义）。经 `respond_permission` 提交后关闭窗口。

### 6.3 组件

`SessionList`（点击 emit select）、`MessageFlow`（按时间排序、按 source 着色）、`StatusBadge`（状态色点）、`icons/FloatingIcon`（`import.meta.glob` 动态加载 bundled SVG，或用 `svgContent` 兜底，按 color 叠色）。

### 6.4 Pinia Store

- **sessions.ts**：`sessions` / `activeSessionId`；`refresh()` 调 `get_sessions`，`setActive(id)`。
- **messages.ts**：`messages` / `loading`；`loadForSession(id)` 调 `get_messages`，`loadAll()` 调 `get_all_messages`（limit 50）。
- **settings.ts**（localStorage 持久化）：`floatingIcon`(默认 eagle)、`idleSound`(Glass)、`permissionSound`(Hero)、`logToFile`(默认 false)、`availableSounds`；`setFloatingIcon`（emit 事件）、`setIdleSound`/`setPermissionSound`（同步 `set_sound_preference`）、`setLogToFile`/`syncLogToFile`（同步 `set_log_to_file`）、`loadAvailableSounds`（含硬编码兜底）。

### 6.5 共享类型（`types.ts`）

`SessionInfo { id, agent, state: "Idle"|"Busy"|"WaitingPermission", workingDir }`、`StoredMessage { id, sessionId, source, text, timestamp }`。

---

## 7. Tauri 命令与事件

**命令（前端 → 后端）**：会话/消息 `get_sessions`、`get_messages`、`get_all_messages`；会话控制 `bind_session`、`inject_input`、`pin_session`、`open_terminal`、`get_active_session`；权限 `respond_permission`；设置 `set_sound_preference`、`set_log_to_file`、`play_sound`；文件/资源 `get_config_path`、`get_home_dir`、`get_log_dir`、`read_config_file`、`open_config_dir`、`open_log_dir`、`open_custom_dir`、`list_available_sounds`、`list_available_icons`、`read_icon_svg`、`fix_svg_icons`；窗口 `open_settings`；飞书 `start_feishu_register`。

**事件（后端 → 前端）**：`sessions-updated`、`messages-updated`、`binding-changed`、`floating-icon-changed`、`feishu-register-qr` / `-done` / `-error`。权限请求通过新建 PermissionPopup 窗口（URL 携带参数）传递。

---

## 8. Go Sidecar（`sidecar/feishu-gateway/`）

把飞书 API 复杂度（OAuth、回调签名、WebSocket）封装在独立 Go 进程，与 Rust 仅用 stdin/stdout JSON 行通信，从而解耦飞书 SDK 升级与 Rust 发布周期。

- **protocol.go**：`Command{ type, app_id, app_secret, chat_id, text, card }`（Rust→Go）；`Event{ type, chat_id, text, sender, message_id, reason, message }`（Go→Rust）。
- **client.go**：基于 Larksuite OAPI SDK + WebSocket，订阅 `P2MessageReceiveV1`；`Connect` / `SendText`（msgType=text）/ `SendCard`（msgType=interactive）/ `Disconnect`；收到文本消息 → 回调 → emit `message_received` 事件。
- **main.go**：扫描 stdin 解析命令，按 `type` 分派（`connect`/`send_text`/`send_card`/`disconnect`，连接前发送会回 `not connected` 错误），结果与错误以 JSON 行写 stdout。
- **构建目标**：作为 `externalBin` 打进 Tauri 包；按平台交叉编译（`feishu-gateway-aarch64-apple-darwin` 等）。

---

## 9. 关键设计取舍

1. **Agent / Platform 双抽象**：trait 化使新增 Agent（如 Pi）或 IM 平台无需改动 Engine 核心。
2. **Sidecar JSON 行 IPC**：人类可读、易调试，隔离 Go 飞书 SDK 与 Rust 生命周期。
3. **Hook Server 拦截权限**：CLI 通过 `curl` 回调本地 HTTP，权限请求以 oneshot + 590s 超时实现"阻塞等待用户决策"。
4. **内存 BindingStore + Pin**：路由轻量；Pin 让用户钉住会话不被自动改绑。
5. **可开关的文件日志**：双层 subscriber + 原子布尔门控，开关实时生效且无重建 subscriber 的竞态（详见 `docs/log-to-file-design.md`）。
6. **出站连接优先**：飞书 WebSocket + Telegram 长轮询，免公网 IP / 免开端口。

---

## 10. 已知限制 / 后续方向

- **平台覆盖**：注入与终端定位逻辑以 macOS 为主（AppleScript / Terminal.app）；Linux 仅打开目录可用；Windows 打开目录分支未实现。
- **Telegram 无富卡片**：权限/问题在 Telegram 以文本编号列表呈现。
- **PTY 模块预留**：`pty/mod.rs` 尚未承载实际 PTY/socket IPC。
- **前端错误未落盘**：当前文件日志仅覆盖 Rust 后端与 panic，不含 Vue 运行时错误。

---

## 11. 相关文档

- `docs/usage.md` / `USAGE.md` — 使用与配置指南
- `docs/log-to-file-design.md` — 日志落盘功能设计
- `docs/technical-design.md`、`docs/build-guide.md`、`docs/slash-commands.md`、`docs/feishu-bot-fix.md` — 既有专题文档
- `README.md` / `README.zh-CN.md` — 项目简介与快速开始
