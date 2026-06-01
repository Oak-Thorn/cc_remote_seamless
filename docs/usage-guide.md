# CC Remote Seamless — 使用指南

## 概述

CC Remote Seamless 让你在离开电脑时，通过飞书手机 App 继续与正在运行的 Claude Code CLI session 交互，回到电脑后无缝继续桌面操作。

---

## 前置条件

- **操作系统：** macOS / Windows / Linux
- **已安装：** Claude Code CLI (`claude`)
- **飞书开发者账号：** 需要创建一个飞书应用（见下方配置）

---

## 依赖安装

### 1. Rust toolchain

```bash
# macOS / Linux
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# 验证
rustc --version   # 需要 1.70+
cargo --version
```

Windows 用户下载 [rustup-init.exe](https://rustup.rs/) 安装。

### 2. Go 1.22+

```bash
# macOS (Homebrew)
brew install go

# Linux (官方二进制)
wget https://go.dev/dl/go1.22.0.linux-amd64.tar.gz
sudo tar -C /usr/local -xzf go1.22.0.linux-amd64.tar.gz
export PATH=$PATH:/usr/local/go/bin

# 验证
go version   # 需要 1.22+
```

**国内网络配置 Go 代理（推荐）：**

```bash
go env -w GOPROXY=https://goproxy.cn,direct
```

### 3. Node.js 18+

```bash
# macOS (Homebrew)
brew install node

# 或使用 nvm
nvm install 18
nvm use 18

# 验证
node --version   # 需要 18+
npm --version
```

### 4. jq（Hook 安装脚本依赖）

```bash
# macOS
brew install jq

# Ubuntu/Debian
sudo apt install jq

# 验证
jq --version
```

---

## 安装

### 从源码构建

```bash
# 克隆项目
git clone https://github.com/your-org/cc-remote-seamless.git
cd cc-remote-seamless

# 0. 确保 Rust 在当前 shell 可用（安装 Rust 后必须执行）
source "$HOME/.cargo/env"

# 1. 构建 Go Sidecar（国内需配置 GOPROXY）
cd sidecar/feishu-gateway
GOPROXY=https://goproxy.cn,direct go build -o feishu-gateway .
cd ../..

# 2. 构建 PTY Proxy
cd crates/cc-remote-pty
cargo build --release
cp target/release/cc-remote-pty ../../src-tauri/binaries/
cd ../..

# 3. 安装前端依赖 + Tauri CLI
npm install
npm install -D @tauri-apps/cli

# 4. 构建 Tauri 应用
npm run tauri build
```

构建产物位于 `src-tauri/target/release/bundle/`。

### 开发模式

```bash
# 启动开发服务器（热重载前端 + Rust 后端）
npm run tauri dev
```

---

## 飞书应用配置

### 步骤 1：创建飞书应用

1. 访问 [飞书开放平台](https://open.feishu.cn/app)
2. 点击「创建企业自建应用」
3. 填写应用名称（如 "CC Remote"）
4. 记录 App ID 和 App Secret

### 步骤 2：配置权限

在应用管理页面 → 权限管理，添加以下权限：

| 权限 | 说明 |
|------|------|
| `im:message` | 读取消息 |
| `im:message:send_as_bot` | 以机器人身份发送消息 |

### 步骤 3：启用事件订阅

1. 进入 事件订阅 页面
2. 选择 **WebSocket 模式**（无需公网 IP）
3. 添加事件：`im.message.receive_v1`（接收消息）

### 步骤 4：发布应用

1. 版本管理 → 创建版本 → 提交审核
2. 审核通过后在企业中启用

---

## 应用配置

创建配置文件 `~/.cc-remote/config.toml`：

```toml
[general]
hook_port = 23399       # Hook Server 端口
auto_start = true       # 登录时自动启动

[feishu]
app_id = "cli_xxxxxxxx"       # 替换为你的 App ID
app_secret = "xxxxxxxxxxxxxxx" # 替换为你的 App Secret

[ui]
float_position = "top-right"   # 浮窗位置: top-right / top-left / bottom-right / bottom-left
theme = "dark"                 # 主题: dark / light
permission_timeout_sec = 60    # 权限弹窗超时时间（秒）

[agents.claude]
type = "claude-code"
socket_dir = "/tmp"            # PTY socket 文件目录
```

---

## 安装 Claude Code Hooks

Hook 让应用感知 Claude Code 的工作状态（空闲/忙碌/等待权限）。

```bash
# 自动安装（推荐）
./scripts/install-hooks.sh

# 或手动安装，自定义端口
CC_REMOTE_HOOK_PORT=23399 ./scripts/install-hooks.sh
```

安装后会在 `~/.claude/settings.json` 中写入以下 hooks：

```json
{
  "hooks": {
    "Stop": [{"type": "command", "command": "curl -s -X POST http://localhost:23399/hook/stop -H \"Content-Type: application/json\" -d \"{}\""}],
    "UserPromptSubmit": [{"type": "command", "command": "curl -s -X POST http://localhost:23399/hook/prompt -H \"Content-Type: application/json\" -d \"{}\""}],
    "PreToolUse": [{"type": "command", "command": "curl -s -X POST http://localhost:23399/hook/pre-tool -H \"Content-Type: application/json\" -d \"{}\""}]
  }
}
```

---

## 使用方式

### 启动应用

1. 运行 CC Remote Seamless 应用
2. 桌面右上角出现浮窗，显示连接状态

### 启动 Claude Code Session（使用 PTY 代理）

```bash
# 使用 PTY 代理包裹 Claude Code
cc-remote-pty --agent claude -- claude

# 指定 session ID（用于飞书绑定）
cc-remote-pty --agent claude --session my-project -- claude
```

PTY 代理会在 `/tmp/cc-remote-claude-{session_id}.sock` 创建 socket 文件，应用自动发现并注册。

### 在飞书中操作

**绑定 session：**

在飞书中向机器人发送消息，首次使用需要绑定 session：

```
/bind my-project
```

**发送消息给 Agent：**

绑定后，直接发送文本即为 Agent 输入：

```
帮我重构 auth 模块，使用 JWT
```

**查看状态：**

```
/status
```

**切换 session：**

```
/switch another-project
```

**查看所有 session：**

```
/sessions
```

**暂停/恢复推送：**

```
/mute
/unmute
```

### 桌面操作

- **浮窗：** 显示 Agent 状态（绿色=空闲，黄色=忙碌，红色=等待权限）和最近消息
- **点击浮窗：** 打开主窗口，查看完整消息历史
- **权限弹窗：** Agent 需要权限时自动弹出，可点击允许/拒绝/始终允许
- **消息来源标记：**
  - `CLI`（灰色）— 桌面终端输入
  - `飞书`（绿色）— 手机飞书输入
  - `Agent`（蓝色）— Agent 输出

### 无缝切换场景

```
电脑前：使用终端正常与 Claude Code 交互
         ↓ 离开电脑
手机上：打开飞书，向机器人发送消息继续交互
         ↓ 回到电脑
电脑前：终端中看到完整历史（包括手机发送的消息和 Agent 回复）
        继续在终端操作
```

---

## 工作原理简述

```
你的终端  ←──→  cc-remote-pty  ←──(socket)──→  Tauri App  ←──(stdio)──→  Go Sidecar  ←──(WS)──→  飞书云端  ←──→  手机飞书
                    │
                    └──→  Claude Code CLI
```

1. `cc-remote-pty` 包裹 Claude Code CLI，同时将 stdin/stdout 暴露给本地 socket
2. Tauri App 通过 socket 读取 Agent 输出并推送到飞书
3. 飞书消息通过 Go Sidecar WebSocket 接收，再注入到 Agent stdin
4. Hook Server 感知 Agent 状态变化，驱动 UI 和推送时机

---

## 常见问题

### Q: `npm run tauri dev` 报 `sh: tauri: command not found`？

需要安装 Tauri CLI 作为项目 devDependency：

```bash
npm install -D @tauri-apps/cli
```

### Q: `npm run tauri dev` 报 `failed to run 'cargo metadata'`？

当前 shell 找不到 `cargo`。安装 Rust 后需要加载环境变量：

```bash
source "$HOME/.cargo/env"
npm run tauri dev
```

如果频繁遇到此问题，将 `source "$HOME/.cargo/env"` 加入 `~/.zshrc` 或 `~/.bashrc`。

### Q: 飞书消息发送后 Agent 没有响应？

检查项：
1. `cc-remote-pty` 是否在运行？（`ls /tmp/cc-remote-*.sock`）
2. 是否已在飞书中执行 `/bind`？
3. Agent 是否处于 busy 状态？（busy 时消息会排队，idle 后自动 flush）

### Q: 权限弹窗没有出现？

检查项：
1. Hook 是否已安装？（检查 `~/.claude/settings.json`）
2. Hook Server 是否在运行？（`curl http://localhost:23399/health`）

### Q: 飞书连接失败？

检查项：
1. `~/.cc-remote/config.toml` 中的 `app_id` 和 `app_secret` 是否正确
2. 飞书应用是否已发布并启用
3. 是否添加了 `im:message` 和 `im:message:send_as_bot` 权限
4. 网络是否能访问 `open.feishu.cn`

### Q: 桌面输入和飞书输入冲突怎么办？

应用内置冲突检测：
- 检测到桌面正在输入时，飞书消息自动排队
- Agent 忙碌时，飞书消息也会排队
- 2 秒无桌面活动后自动 flush 排队消息

### Q: 如何同时管理多个 session？

```bash
# 终端 1
cc-remote-pty --agent claude --session project-a -- claude

# 终端 2
cc-remote-pty --agent claude --session project-b -- claude
```

在飞书中使用 `/switch` 切换：
```
/switch project-a
/switch project-b
```

---

## 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `CC_REMOTE_HOOK_PORT` | `23399` | Hook Server 端口 |
| `CC_REMOTE_CONFIG` | `~/.cc-remote/config.toml` | 配置文件路径 |
| `CC_REMOTE_DB` | `~/.cc-remote/messages.db` | 消息数据库路径 |

---

## 开发相关

### 运行测试

```bash
# Rust 测试
cargo test --workspace

# Go Sidecar 测试
cd sidecar/feishu-gateway
go test -v ./...

# 前端类型检查
npx vue-tsc --noEmit
```

### 项目结构速查

| 目录 | 说明 |
|------|------|
| `src-tauri/` | Tauri Rust 后端（Engine、Hook、Agent、Platform） |
| `crates/cc-remote-pty/` | PTY Proxy 独立 binary |
| `sidecar/feishu-gateway/` | Go 飞书 sidecar |
| `src/` | Vue 3 前端（浮窗、主窗口、权限弹窗） |
| `scripts/` | Hook 安装等辅助脚本 |
| `docs/` | 技术设计和使用文档 |
