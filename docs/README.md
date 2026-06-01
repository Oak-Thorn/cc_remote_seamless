# CC Remote Seamless

通过飞书和 Telegram 远程控制你正在运行的 Claude Code / Pi Agent CLI 会话。离开电脑时，用手机继续与 Agent 交互。

## 功能特性

- **无损终端体验** — 桌面 CLI 保持完整 ANSI 格式、颜色、交互，远程控制零影响
- **多平台 IM** — 同时支持飞书（WebSocket）和 Telegram（Bot API），可配置多个平台
- **多 Agent 支持** — Claude Code 和 Pi Agent，通过 `/change` 切换
- **Slash 命令** — `/status`、`/list`、`/switch`、`/mute`、`/p`、`/allow`、`/answer` 等
- **权限远程审批** — 工具权限请求推送到 IM，支持 `/allow`、`/deny`、`/always`
- **Question 远程回答** — AskUserQuestion 推送紫色卡片，通过 `/answer N` 回复
- **桌面浮窗** — 浮窗图标置顶，颜色随状态变化，展开显示 session 列表
- **音效提醒** — Agent 空闲和权限请求时播放可配置音效
- **自动 Hook 安装** — 启动时自动配置 Claude Code hooks
- **自动绑定** — Hook 事件触发自动绑定最近活跃 session
- **无需公网 IP** — 飞书 WebSocket + Telegram long polling，从客户端主动发起

## 快速开始

### 1. 安装依赖

```bash
# Rust, Node.js 18+, Go 1.21+
cd cc-remote-seamless
npm install
```

### 2. 配置 IM 平台

创建 `~/.cc-remote/config.toml`：

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

飞书应用创建步骤见 [usage-guide.md](usage-guide.md)。

### 3. 启动应用

```bash
# 开发模式
npm run tauri dev

# 或生产构建
./scripts/build.sh
```

应用启动后自动安装 Claude Code hooks 并连接 IM 平台。

### 4. 在 IM 中交互

打开飞书/Telegram → 找到你的机器人 → 直接发消息或使用 Slash 命令。

## Slash 命令参考

| 命令 | 说明 |
|------|------|
| `/status` | 查看当前会话状态 |
| `/list` | 列出所有活跃会话 |
| `/switch <id>` | 切换到指定 session |
| `/change [agent]` | 切换 Agent 类型 |
| `/p <text>` | 发送 prompt |
| `/mute` / `/unmute` | 静音/恢复消息推送 |
| `/full` | 查看完整输出 |
| `/allow` / `/deny` / `/always` | 权限审批 |
| `/answer <n>` | 回答问题 |
| `/clear` | 清除待注入输入 |
| `/stop` | 停止 session |
| `/help` | 显示帮助 |

详见 [slash-commands.md](slash-commands.md)。

## 工作流程

### 典型场景

1. 启动 CC Remote Seamless 应用（自动安装 hooks、连接 IM）
2. 正常在终端中使用 Claude Code 编码
3. 需要离开时，直接走开 — 不需要做任何切换操作
4. 在手机 IM 上发送消息，Agent 收到输入并继续工作
5. Agent 的输出自动推送到 IM
6. 权限请求和问题也会推送到 IM，可远程审批/回答
7. 回到电脑前，终端中一切如常，历史完整保留

### 输入时序

- Agent **空闲**时：IM 消息立即注入
- Agent **忙碌**时：`/p` 命令返回 "Session is busy" 提示
- **权限等待中**：通过 `/allow`、`/deny`、`/always` 响应

## 架构概览

```
┌─────────────────────────────────────────────────────┐
│  CC Remote Seamless (Tauri 2 + Vue 3)               │
│                                                     │
│  ┌─────────┐  ┌────────┐  ┌───────────────────┐    │
│  │ 浮窗 UI │  │ Engine │  │ Hook Server :23399│    │
│  │(浮窗图标)│←→│(Router)│←─│ (21 endpoints)    │←── Claude Code / Pi hooks
│  └─────────┘  └────────┘  └───────────────────┘    │
│                     ↕                               │
│  ┌─────────────────────────────────────────────┐    │
│  │ Agents: ClaudeCode (Hook+AppleScript)       │    │
│  │         Pi (Hook+HTTP inject)               │    │
│  └─────────────────────────────────────────────┘    │
│                     ↕                               │
│  ┌─────────────────────────────────────────────┐    │
│  │ Platforms: Feishu (Go sidecar, WS)          │    │
│  │            Telegram (reqwest, long poll)     │    │
│  └─────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────┘
```

**数据流：**

1. IM 消息 → Platform Connector → Engine → Slash 命令 or inject_input → Agent
2. Agent 输出 → Hook Server → Engine → Platform(s) → IM
3. 权限请求 → Hook Server → 桌面弹窗 + IM 卡片 → 用户响应 → Agent

## 配置说明

### 配置文件

`~/.cc-remote/config.toml` — 详见 [usage.md](usage.md)。

### 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `CC_REMOTE_HOOK_PORT` | `23399` | Hook Server 端口 |

## 常见问题

**Q: 需要公网 IP 或端口映射吗？**

不需要。飞书使用 WebSocket 长连接，Telegram 使用 long polling，均由客户端主动连接。

**Q: 能同时连接多个 IM 平台吗？**

可以。在 config.toml 中配置多个 `[platforms.*]` 段，并在 `[agents.*]` 中关联即可。

**Q: 支持哪些 Agent？**

目前支持 Claude Code 和 Pi Agent。通过实现 `AgentConnector` trait 可扩展更多 Agent。

**Q: 消息延迟大吗？**

Hook Server 本地通信延迟 < 1ms。飞书 WebSocket 延迟通常 100-300ms，Telegram long polling 延迟取决于网络。

## 开发

```bash
npm install
npm run tauri dev          # 开发模式（热重载）
./scripts/build.sh         # 生产构建
cargo test --workspace     # Rust 测试
cd sidecar/feishu-gateway && go test -v ./...  # Go 测试
```

详见 [build-guide.md](build-guide.md)。

## 文档索引

| 文档 | 内容 |
|------|------|
| [technical-design.md](technical-design.md) | 技术架构设计（模块、数据流、协议） |
| [usage.md](usage.md) | 完整使用指南（配置、命令、Hook 事件） |
| [usage-guide.md](usage-guide.md) | 安装部署指南（从零开始） |
| [build-guide.md](build-guide.md) | 构建和分发指南 |
| [slash-commands.md](slash-commands.md) | Slash 命令详细参考 |

## 许可证

AGPL-3.0-only
