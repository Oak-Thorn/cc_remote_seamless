# Feishu Remote Control for clawd-on-desk

> 在电脑端使用 Claude Code CLI 开发时，离开电脑后通过飞书无缝继续与同一 session 交互。

## 1. 需求概述

### 核心目标

基于 [clawd-on-desk](https://github.com/rullerzhou-afk/clawd-on-desk) 开发飞书远程控制模块，实现：

- 飞书端发消息 → 注入到桌面端已运行的 Claude Code / Codex session
- Agent 回复 → 摘要推送到飞书
- 桌面端 CLI 体验完全无损（保留所有终端格式、交互特性）

### 手机端功能

1. **切换 Agent** — 支持 Claude Code、Codex 等多 Agent 切换
2. **消息开关** — 支持静音/恢复推送
3. **Session 管理** — 切换 session、查询当前状态

### 设计约束

- 飞书个人 Bot 私聊（单人使用）
- 附加到已运行的 session（非新建）
- 斜杠命令交互
- 默认摘要回复，可查看完整输出

## 2. 架构设计

### 整体架构

```
┌─────────────────────────────────────────────────────────┐
│                    clawd-on-desk (Electron)              │
│                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │ Hook Server  │  │ PTY Manager  │  │ Feishu Bot   │  │
│  │ (port 23333) │  │              │  │ (WebSocket)  │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  │
│         │                  │                  │          │
│         └──────────┬───────┴──────────┬───────┘          │
│                    │  Session Router   │                  │
│                    └──────────────────┘                  │
└─────────────────────────────────────────────────────────┘
         ↑ Hook POST              ↕ Unix Socket
         │                        │
┌────────┴────────────────────────┴───────────┐
│          PTY Proxy (node-pty)                │
│   ┌─────────────┐    ┌─────────────────┐    │
│   │ Agent CLI   │    │ stdout/stdin     │    │
│   │ (claude/    │    │ → user terminal  │    │
│   │  codex)     │    └─────────────────┘    │
│   └─────────────┘                           │
└─────────────────────────────────────────────┘
```

### 核心组件

| 组件 | 职责 |
|------|------|
| PTY Proxy (clawd-wrap) | 包装 Agent CLI，转发 I/O 到终端，通过 Unix Socket 暴露给 clawd-on-desk |
| PTY Manager | 管理多个 PTY 实例，维护 session 状态 |
| Hook Server | 现有组件，接收 Agent 生命周期事件 |
| Session Router | 协调 Hook 事件 + PTY 状态，决定何时可注入输入 |
| Feishu Bot | WebSocket 长连接飞书，收发消息，解析斜杠命令 |

### 方案选型理由

选择 PTY Proxy + Unix Socket 方案（而非 tmux 代理或 Hook 双向扩展），因为：

- 终端体验 100% 无损（PTY 保留所有 ANSI、交互式特性）
- 与 clawd-on-desk 同技术栈（Node.js / node-pty）
- Unix Socket 零网络开销
- PTY Proxy 独立于 clawd-on-desk 进程，互不影响

## 3. PTY Proxy 层

### 启动方式

```bash
# clawd-on-desk 安装时自动写入 .zshrc
alias claude="clawd-wrap claude"
alias codex="clawd-wrap codex"
```

用户在终端中正常使用 `claude` 命令，底层通过 PTY 代理运行，体验完全一致。

### Unix Socket 协议

Socket 路径：`/tmp/clawd-pty-<agent>-<session-id>.sock`

**clawd-on-desk → PTY Proxy：**

```json
{ "type": "input", "data": "帮我重构这个函数\n" }
{ "type": "resize", "cols": 80, "rows": 24 }
{ "type": "kill" }
```

**PTY Proxy → clawd-on-desk：**

```json
{ "type": "output", "data": "..." }
{ "type": "exit", "code": 0 }
{ "type": "state", "waiting": true }
```

### 等待输入检测

结合两个信号判断 Agent 是否在等待用户输入：

1. **Hook 信号**：收到 `Stop` 事件（上一轮回复结束）
2. **PTY 输出模式**：检测到 prompt 标记后无新输出超过 500ms

两个条件同时满足才允许注入。

### 终端体验保证

- PTY 的 cols/rows 实时同步 SIGWINCH
- 所有 ANSI escape codes 原样透传
- Ctrl+C / Ctrl+D 等信号正常传递
- 用户终端输入优先级高于飞书注入

## 4. 飞书 Bot 模块

### 连接方式

使用飞书开放平台企业自建应用的 WebSocket 长连接：

- 协议：`wss://open.feishu.cn/open-apis/bot/v2/ws`
- 无需公网 IP、域名、HTTPS 证书
- 断线自动重连（指数退避）

### 飞书端配置

在 clawd-on-desk Settings 面板中新增"飞书远程控制"区域：

- App ID / App Secret 输入框
- 连接状态指示灯
- 一键测试连接

### 认证流程

1. 用户在飞书开放平台创建企业自建应用
2. 开启 Bot 能力，添加权限：`im:message:send_as_bot`、`im:message.p2p_msg:readonly`
3. 订阅事件：`im.message.receive_v1`
4. 在 clawd-on-desk Settings 中填入 App ID / App Secret
5. clawd-on-desk 获取 tenant_access_token → 建立 WebSocket 长连接

### 斜杠命令

| 命令 | 功能 | 示例回复 |
|------|------|----------|
| `/status` | 查看当前 Agent 状态和 session | `claude · session abc123 · idle` |
| `/sessions` | 列出所有活跃 session | session 列表 + agent 类型 |
| `/switch <agent>` | 切换控制目标 | `/switch codex` → 切到 codex |
| `/mute` | 关闭消息推送 | `已静音，发 /unmute 恢复` |
| `/unmute` | 恢复消息推送 | `已恢复推送` |
| `/full` | 查看上一次完整回复 | 完整文本（strip ANSI） |
| `/help` | 命令列表 | 简短帮助 |

非斜杠命令的文本消息直接作为 prompt 注入当前活跃 Agent。

### 消息处理流程

```
飞书消息到达
    │
    ├─ 以 "/" 开头 → 解析为命令 → 执行
    │
    └─ 普通文本 → 作为 prompt 注入
                    │
                    ├─ Agent 在等待输入 → 立即注入 → 回复 "✓ 已发送"
                    │
                    └─ Agent 在执行中 → 排队 → 回复 "⏳ Agent 忙碌中，已排队"
```

### 回复推送

当 Agent 回复结束时（非 mute 状态）：

- 短回复（< 500 字）：直接发送完整内容
- 长回复（>= 500 字）：首 200 字 + `...` + 末 100 字 + 提示 `/full`
- 含代码块：保留第一个代码块前 10 行

不依赖 LLM 生成摘要，纯规则处理，零额外成本。

## 5. 多 Agent / 多 Session 管理

### Session 模型

```
PTY Manager
  ├── Session: claude-abc123
  │     ├── agent: "claude"
  │     ├── pty: <node-pty instance>
  │     ├── socket: /tmp/clawd-pty-claude-abc123.sock
  │     ├── state: "idle" | "thinking" | "executing"
  │     └── buffer: <最近一次回复>
  │
  ├── Session: codex-def456
  │     └── ...
  │
  └── activeSession: "claude-abc123"  ← 飞书当前控制目标
```

### 生命周期

| 事件 | 行为 |
|------|------|
| 用户运行 `clawd-wrap claude` | PTY 启动 → 注册 Socket → PTY Manager 记录 |
| 用户运行 `clawd-wrap codex` | 同上，新增 session |
| 飞书 `/switch codex` | activeSession 切换 |
| Agent 进程退出 | PTY Proxy 发 exit → 移除 session → 飞书通知 |
| clawd-on-desk 退出/重启 | 不影响 PTY Proxy；重启后自动重连现有 Socket |

### 关键设计决策

1. **PTY Proxy 独立进程** — clawd-on-desk 崩溃不影响用户终端
2. **飞书一次控制一个 session** — `/switch` 切换，避免歧义
3. **Session 发现** — 启动时扫描 `/tmp/clawd-pty-*.sock` 自动连接

### 输入冲突处理

```
飞书发来 prompt
    │
    ├─ 用户终端 2s 内有键盘输入 → 排队 + 通知 "桌面端正在输入，已排队"
    │
    └─ 用户终端无活动
          ├─ Agent 等待输入 → 注入
          └─ Agent 执行中 → 排队
```

## 6. 技术实现

### 技术栈

| 组件 | 技术 |
|------|------|
| PTY Proxy | Node.js + node-pty |
| Unix Socket | Node.js `net` 模块 |
| 飞书 WebSocket | `ws` 库 |
| ANSI 清理 | `strip-ansi` |
| clawd-wrap CLI | Node.js 可执行脚本（npm bin） |

### 文件结构（clawd-on-desk 新增）

```
src/
  feishu/
    bot.js          # 飞书 WebSocket 连接管理、消息收发
    commands.js     # 斜杠命令解析和执行
    formatter.js    # 输出格式化、摘要生成
  pty/
    manager.js      # PTY session 管理、Socket 发现
    protocol.js     # Unix Socket JSON 协议定义
bin/
  clawd-wrap        # CLI wrapper 脚本
```

## 7. 实现优先级

### Phase 1 — MVP（核心链路）

1. clawd-wrap CLI — PTY 启动 + Unix Socket 暴露
2. PTY Manager — 单 session 管理
3. Feishu Bot — WebSocket 连接 + 消息收发
4. 基本注入 — 飞书文本 → Agent stdin
5. 基本推送 — Agent 回复 → 飞书（strip ANSI，无摘要）

### Phase 2 — 完善体验

6. 斜杠命令完整实现
7. 摘要策略（长文本截断 + `/full`）
8. 等待输入检测 + 排队机制
9. 输入冲突检测

### Phase 3 — 多 Agent

10. 多 session 管理
11. `/switch` + `/sessions` 命令
12. Session 自动发现（Socket 扫描）
13. clawd-on-desk Settings UI（飞书配置面板）
