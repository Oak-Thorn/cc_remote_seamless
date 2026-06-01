# CC Remote Seamless 使用指南

## 概述

CC Remote Seamless 是一个 macOS 桌面应用（Tauri 2 + Vue 3），用于远程监控和操控本机运行的 Claude Code / Pi Agent session。支持通过飞书和 Telegram 发送 prompt、审批权限请求、回答问题、查看 session 状态，以及通过桌面浮窗快速管理多个 Agent 实例。

## 安装与配置

### 配置文件

配置文件路径：`~/.cc-remote/config.toml`

```toml
[general]
hook_port = 23399        # Hook 服务器端口，默认 23399

[general.sounds]
idle = "Glass"           # Agent 空闲时音效
permission = "Hero"      # 权限请求时音效

[platforms.feishu-main]
type = "feishu"
app_id = "cli_xxxxxxxxx"
app_secret = "xxxxxxxxxxxxxxxxxxxxxxxx"
chat_id = "oc_xxxxxxxxxxxxxxxxxxxxxxxx"

[platforms.telegram]
type = "telegram"
bot_token = "123456:ABC-DEF..."
chat_id = "987654321"

[agents.claude-code]
platforms = ["feishu-main", "telegram"]

[agents.pi]
platforms = ["feishu-main"]
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `general.hook_port` | 否 | HTTP Hook 服务端口，默认 `23399` |
| `general.sounds.idle` | 否 | 空闲音效名，默认 `Glass` |
| `general.sounds.permission` | 否 | 权限音效名，默认 `Hero` |
| `platforms.<id>.type` | 是 | 平台类型：`feishu` 或 `telegram` |
| `platforms.<id>.chat_id` | 是 | 绑定的聊天 ID |
| `agents.<id>.platforms` | 是 | Agent 关联的平台 ID 列表 |

### Claude Code Hook 配置

应用启动时会自动安装 Hook 到 `~/.claude/settings.json`（通过 `hook/installer.rs`）。如需手动配置：

```json
{
  "hooks": {
    "SessionStart": [{ "type": "command", "command": "curl -s -X POST http://127.0.0.1:23399/hook/session-start -H 'Content-Type: application/json' -d \"$(cat)\"" }],
    "Stop": [{ "type": "command", "command": "curl -s -X POST http://127.0.0.1:23399/hook/stop -H 'Content-Type: application/json' -d \"$(cat)\"" }],
    "PreToolUse": [{ "type": "command", "command": "curl -s -X POST http://127.0.0.1:23399/hook/pre-tool -H 'Content-Type: application/json' -d \"$(cat)\"" }],
    "PostToolUse": [{ "type": "command", "command": "curl -s -X POST http://127.0.0.1:23399/hook/post-tool -H 'Content-Type: application/json' -d \"$(cat)\"" }],
    "Notification": [{ "type": "command", "command": "curl -s -X POST http://127.0.0.1:23399/hook/notification -H 'Content-Type: application/json' -d \"$(cat)\"" }],
    "SubagentStart": [{ "type": "command", "command": "curl -s -X POST http://127.0.0.1:23399/hook/subagent_start -H 'Content-Type: application/json' -d \"$(cat)\"" }],
    "SubagentStop": [{ "type": "command", "command": "curl -s -X POST http://127.0.0.1:23399/hook/subagent_stop -H 'Content-Type: application/json' -d \"$(cat)\"" }]
  }
}
```

权限审批 Hook（项目级 `.claude/settings.json`）：

```json
{
  "hooks": {
    "PreToolUse": [{ "type": "command", "command": "curl -s -X POST http://127.0.0.1:23399/permission -H 'Content-Type: application/json' -d \"$(cat)\"" }]
  }
}
```

### macOS 权限要求

飞书远程注入功能需要以下系统权限：

- **辅助功能（Accessibility）**：用于 Terminal.app 的剪贴板粘贴注入方式
- 如果使用 iTerm2，则**不需要**辅助功能权限（使用原生 `write text` 命令）

在「系统设置 → 隐私与安全性 → 辅助功能」中添加 CC Remote Seamless 应用。

## 功能说明

### 1. Session 自动发现

CC Remote 通过 Hook 事件自动发现 Claude Code session。当收到 SessionStart、PromptSubmit、PreToolUse 等事件时，自动注册并绑定到所有配置的 chat_id（除非已 pin）。每 5 秒执行一次 reconcile，检测：

- 新启动的 session（通过 `~/.claude/sessions/*.json`）
- 已退出的 session（进程不存在）
- 状态变化（通过 deferred idle 机制，防抖 1 秒）

### 2. 桌面浮窗

启动后显示一个置顶浮窗（浮窗图标），包含：

- **浮窗图标**：可在设置中选择（猫、狗、鹰、太阳等），颜色随状态变化
- **状态颜色**：绿色=Idle，橙色=Busy，红色=等待权限
- **展开面板**：鼠标悬停展开，显示 session 列表和最近消息
- **自动折叠**：5 秒无操作自动收起

### 3. 权限审批

当 Claude Code 请求工具权限时：

1. 桌面弹出权限审批窗口，显示工具名称和参数 + 播放权限音效
2. 同时在所有关联 IM 平台发送权限卡片（包含工具详情）
3. 用户可通过桌面弹窗或 IM 回复审批

支持三种响应：
- **Allow**（`/allow`）：允许本次操作
- **Deny**（`/deny`）：拒绝本次操作
- **Always Allow**（`/always`）：始终允许此类操作（写入 permission rules）

### 4. Question 回复（Elicitation）

当 Claude Code 使用 `AskUserQuestion` 工具向用户提问时：

1. 桌面弹出问题窗口，显示问题内容和选项
2. IM 平台收到紫色卡片，展示问题内容和编号选项列表
3. 用户通过 `/answer N` 回复选择

支持：
- `/answer 1` — 单选
- `/answer 1 3` 或 `/answer 1,3` — 多选

### 5. 多平台 IM 集成

支持同时连接多个 IM 平台，每个 Agent 可配置关联多个平台：

**飞书**：通过 Go sidecar 进程与飞书 API 通信（WebSocket 长连接）
**Telegram**：直接通过 Rust reqwest 调用 Bot API（long polling）

**自动绑定机制**：
- 启动时自动绑定最近发现的 session 到所有配置的 chat_id
- 每当收到 hook 事件，自动将最近活跃的 session 重新绑定（除非 pinned）

### 6. 多 Agent 支持

支持同时管理多种 Agent：
- **Claude Code**：通过 Hook 事件感知状态，通过 AppleScript 注入输入
- **Pi Agent**：通过独立 Hook 端点 `/pi/*` 接收事件，通过 HTTP inject_port 注入

使用 `/change` 命令在 IM 中切换 Agent 类型。

### 7. 设置页面

桌面设置窗口提供：
- **图标选择**：选择浮窗图标（SVG，自动修复颜色）
- **音效设置**：选择空闲/权限音效
- **配置编辑**：查看和编辑 config.toml
- **关于页面**：版本信息

## IM 命令

在绑定的 IM 聊天中发送以下命令来控制 Agent session。

### Session 管理

| 命令 | 说明 | 示例 |
|------|------|------|
| `/list` | 列出所有活跃 session（含状态和工作目录） | `/list` |
| `/status` | 查看当前活跃 session 的详细状态 | `/status` |
| `/switch <id>` | 切换到指定 session（支持前缀匹配） | `/switch 7052f` |
| `/change [agent]` | 切换 agent 类型（无参数列出可用 agent） | `/change pi` |

### Prompt 发送

| 命令 | 说明 | 示例 |
|------|------|------|
| `/p <text>` | 向当前绑定 session 发送 prompt | `/p 当前工作目录` |
| 普通文本 | 直接注入到当前 session | `帮我重构 auth 模块` |

### 权限控制

| 命令 | 说明 |
|------|------|
| `/allow` | 允许本次操作 |
| `/deny` | 拒绝本次操作 |
| `/always` | 始终允许此类操作（写入永久规则） |

### Question 回复

| 命令 | 说明 | 示例 |
|------|------|------|
| `/answer <n>` | 单选第 n 项 | `/answer 1` |
| `/answer <n> <m>` | 多选（空格分隔） | `/answer 1 3` |
| `/answer <n>,<m>` | 多选（逗号分隔） | `/answer 1,2,3` |

### 输出控制

| 命令 | 说明 |
|------|------|
| `/mute` | 静音，不转发 Agent 输出到 IM |
| `/unmute` | 取消静音 |
| `/full` | 显示最后一条完整输出（不受静音影响） |

### 其他

| 命令 | 说明 |
|------|------|
| `/clear` | 清除当前 session 的待注入输入 |
| `/stop` | 停止当前 session |
| `/skill <query>` | 搜索可用 skills |
| `/help` | 显示命令帮助 |
| `/t <text>` | 测试命令（仅记录日志） |

## 架构

```
┌─────────────────────────────────────────────────────┐
│  CC Remote Seamless (Tauri 2 App)                   │
│                                                     │
│  ┌───────────┐  ┌──────────┐  ┌─────────────────┐  │
│  │ Vue 3 UI  │  │  Engine  │  │  Hook Server    │  │
│  │ (浮窗)    │←→│ (Router) │←─│  :23399         │←── Claude Code / Pi hooks
│  └───────────┘  └──────────┘  └─────────────────┘  │
│                       ↕                             │
│  ┌───────────────────────────────────────────────┐  │
│  │  Agent Connectors                             │  │
│  │  ├ ClaudeCodeConnector (Hook + AppleScript)   │  │
│  │  └ PiConnector (Hook + HTTP inject)           │  │
│  └───────────────────────────────────────────────┘  │
│                       ↕                             │
│  ┌───────────────────────────────────────────────┐  │
│  │  IM Platforms                                 │  │
│  │  ├ Feishu (Go Sidecar, JSON Lines stdio)      │  │
│  │  └ Telegram (reqwest, Bot API long polling)   │  │
│  └───────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

## Hook 事件

CC Remote 接收并处理以下 Hook 事件：

### Claude Code 事件

| 事件 | 路由 | 状态影响 |
|------|------|----------|
| SessionStart | `/hook/session-start` | → Idle，注册 session |
| SessionEnd | `/hook/session-end` | 移除 session |
| PromptSubmit | `/hook/prompt` | → Busy，转发 prompt 到 IM |
| PreToolUse | `/hook/pre-tool` | → Busy |
| PostToolUse | `/hook/post-tool` | → Busy |
| PostToolUseFailure | `/hook/post-tool-failure` | → Busy |
| Stop | `/hook/stop` | → Idle，转发输出到 IM |
| StopFailure | `/hook/stop_failure` | → Idle |
| SubagentStart | `/hook/subagent_start` | → Busy |
| SubagentStop | `/hook/subagent_stop` | → Busy |
| Notification | `/hook/notification` | → Idle |
| Elicitation | `/hook/elicitation` | → WaitingPermission，弹出问题窗 |
| PreCompact | `/hook/pre_compact` | → Busy |
| PostCompact | `/hook/post_compact` | → Idle |
| PermissionRequest | `/permission` | → WaitingPermission（阻塞等待响应） |

### Pi Agent 事件

| 事件 | 路由 | 状态影响 |
|------|------|----------|
| PiSessionStart | `/pi/session_start` | 注册 Pi session |
| PiSessionEnd | `/pi/session_end` | 移除 Pi session |
| PiInput | `/pi/input` | 转发输入到 IM |
| PiPreToolUse | `/pi/pre_tool` | → Busy |
| PiPostToolUse | `/pi/post_tool` | → Busy |
| PiPermissionRequest | `/pi/permission` | → WaitingPermission，弹窗 + 卡片 |

## 开发

```bash
# 安装依赖
npm install

# 开发模式（前端 + Tauri）
npm run tauri dev

# 编译 Feishu sidecar
cd sidecar/feishu-gateway && go build -o feishu-gateway

# 构建生产版本
npm run tauri build
```

## 故障排查

### Session 未显示

1. 确认 Claude Code 正在运行：`ls ~/.claude/sessions/*.json`
2. 检查 hook 是否配置：`cat ~/.claude/settings.json | jq .hooks`
3. 查看 CC Remote 日志中是否有 `discover_sessions()` 输出

### 飞书消息未发送

1. 确认配置文件存在且格式正确：`cat ~/.cc-remote/config.toml`
2. 检查 sidecar 进程：`ps aux | grep feishu-gateway`
3. 确认飞书应用有群聊发消息权限

### Prompt 注入失败

1. 查看日志中 `terminal=` 字段确认检测到正确终端
2. 如果显示 `not_found`，说明 TTY 未在终端 tab 中匹配
3. Terminal.app 用户需要授予辅助功能权限
4. iTerm2 用户无需额外权限，确保 iTerm2 正在运行

### 权限审批超时

- 默认超时 590 秒
- 超时后自动 deny，Claude Code 会收到拒绝响应
- 确认飞书消息正常接收，或使用桌面弹窗审批
