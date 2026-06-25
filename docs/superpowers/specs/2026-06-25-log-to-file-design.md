# 设计:日志与崩溃信息落盘

日期:2026-06-25
作者:thorn / Ducc

## 背景与目标

软件近期出现「收到消息无响应」「莫名崩溃」等问题,难以排查。需要将运行日志与崩溃(panic)信息持久化到本地文件,便于事后分析。

目标:

1. 日志保存到 `~/.cc-remote/logs/` 目录。
2. 设置页显示日志目录路径,支持一键打开该目录;提供开关控制是否落盘,**默认关闭**。
3. 开关实时生效,无需重启。
4. 捕获 Rust 端 panic,写入同一套日志。
5. 按天轮转日志文件,并保留最近 N 天(默认 7 天),启动时清理过期文件。

非目标(YAGNI):

- 不捕获前端 JS 错误(本期仅 Rust 后端 + panic)。
- 不做日志上传 / 远程聚合。
- 不做日志等级的运行时动态调整。

## 现状

- 后端用 `tracing`,初始化方式为 `tracing_subscriber::fmt::init()`(仅 stdout),位于 `src-tauri/src/lib.rs:26`。
- `~/.cc-remote/` 目录已有先例:`config.toml`、`icons/`、`sounds/`。
- 设置页 Config tab(`src/views/SettingsWindow.vue`)已有「显示路径 + Open 目录」成熟模式;`open_config_dir` 命令(`src-tauri/src/commands.rs:140`)封装了 macOS/Linux 打开目录逻辑。
- 前端偏好持久化用 `localStorage` + Pinia store(`src/stores/settings.ts`),启动时 invoke 同步给后端(参考 sound 偏好)。

## 架构

### 1. 后端日志层(Rust)

目录结构:

```
~/.cc-remote/logs/
  cc-remote.2026-06-25.log
  cc-remote.2026-06-24.log
  ...
```

新增模块 `src-tauri/src/logging.rs`,职责:

- 维护全局开关:`static LOG_ENABLED: AtomicBool`。
- 提供 `GatedWriter`:实现 `std::io::Write`,根据 `LOG_ENABLED` 决定写入按天滚动的文件 writer 还是丢弃(`std::io::sink()`)。
- 提供 `GatedMakeWriter`:实现 `tracing_subscriber::fmt::MakeWriter`,每次写日志返回上述 `GatedWriter`。
- 提供 `init(enabled_initial: bool, log_dir: PathBuf)`:
  - 用 `tracing_appender::rolling::Builder`(`rotation(Rotation::DAILY)`、`filename_prefix("cc-remote")`、`filename_suffix("log")`)生成按天滚动 writer,文件名形如 `cc-remote.2026-06-25.log`。用 `non_blocking` 包装,`WorkerGuard` 需存活整个进程生命周期(放进 `OnceCell` 或泄漏)。
  - 注册一个 subscriber,包含**两层**:
    - stdout 层:始终输出(保留现有控制台行为)。
    - gated 文件层:始终注册,实际是否落盘由 `LOG_ENABLED` 实时控制。
  - 设置 `LOG_ENABLED` 初值为 `enabled_initial`。
- 提供 `set_enabled(bool)`:翻转 `LOG_ENABLED`,实时生效。
- 提供 `log_dir() -> PathBuf`:返回 `~/.cc-remote/logs`。
- 提供 `cleanup_old_logs(retain_days: u64)`:扫描日志目录,删除文件名日期早于 `今天 - retain_days` 的 `cc-remote.*.log` 文件。`retain_days` 默认常量 `RETAIN_DAYS = 7`。

实时开关的关键:subscriber 只初始化一次,两层都常驻;切换开关仅翻转 `AtomicBool`,不重建 subscriber,避免 reload layer 的复杂度与出错风险。

### 2. Panic 捕获

在 `run()` 中、`logging::init(...)` 之后、其余初始化之前安装:

```rust
std::panic::set_hook(Box::new(|info| {
    tracing::error!("PANIC: {}", info);
    eprintln!("{}", info); // 保留默认 stderr 行为
}));
```

panic 信息通过 `tracing::error!` 进入 gated 文件层;开关开启时即写入当天日志。

### 3. 启动流程改动(`src-tauri/src/lib.rs`)

`run()` 开头替换 `tracing_subscriber::fmt::init()`:

```rust
let log_dir = logging::log_dir();
let _ = std::fs::create_dir_all(&log_dir);
let initial_enabled = false; // 后端默认关;前端启动时会 invoke set_log_to_file 同步真实值
logging::init(initial_enabled, log_dir.clone());
logging::cleanup_old_logs(logging::RETAIN_DAYS);
std::panic::set_hook(...);
```

后端默认 `false`,与前端默认一致;前端 `onMounted` 会把 localStorage 的真实值同步过来(与 sound 偏好同模式),避免「前端开了但后端没开」的不一致。

### 4. 新增 Tauri 命令(`src-tauri/src/commands.rs`)

| 命令 | 签名 | 作用 |
|------|------|------|
| `get_log_dir` | `() -> Result<String, String>` | 返回 `~/.cc-remote/logs` 绝对路径 |
| `open_log_dir` | `() -> Result<(), String>` | 打开日志目录(复用 `open_config_dir` 的 macOS/Linux 逻辑,目录不存在则创建) |
| `set_log_to_file` | `(enabled: bool) -> Result<(), String>` | 调用 `logging::set_enabled(enabled)`,实时生效 |

在 `lib.rs` 的 `generate_handler!` 中注册这三个命令。

### 5. 前端改动

**store(`src/stores/settings.ts`)**:

- 新增 `logToFile = ref<boolean>(localStorage.getItem("logToFile") === "true")`(默认 `false`,因为初始无该 key)。
- 新增 `setLogToFile(enabled: boolean)`:写 `localStorage`,并 `invoke("set_log_to_file", { enabled })`。
- 新增 `syncLogToFile()`:启动时把当前 `logToFile.value` invoke 给后端一次(对齐 sound 同步)。

**视图(`src/views/SettingsWindow.vue`)**:

- 新增响应式 `logDir = ref("")`;`onMounted` 时 `invoke("get_log_dir")`,并用现有 home 替换逻辑显示为 `~/.cc-remote/logs`。
- `onMounted` 调用 `settings.syncLogToFile()`。
- `activeTab` 类型扩展为 `"icon" | "sound" | "config" | "logs" | "about"`。
- 侧边栏 `nav` 在 Config 与 About 之间新增一项「Logs」。
- 新增独立的 Logs 面板(`v-else-if="activeTab === 'logs'"`):
  - 标题 `<h3>Logs</h3>`。
  - 路径行:复用 `config-header` / `config-path-text` 样式显示日志目录路径 + `Open` 按钮(`@click="openLogDir"`,invoke `open_log_dir`)。
  - 一个 toggle 开关:label「保存日志到文件」,绑定 `settings.logToFile`,`@change` 调 `settings.setLogToFile(...)`。toggle 用纯 CSS 实现(checkbox + slider),默认关。

## 数据流

```
[用户拨动 toggle]
  -> setLogToFile(enabled)
       -> localStorage.setItem("logToFile", ...)
       -> invoke("set_log_to_file", { enabled })
            -> logging::set_enabled(enabled)  // 翻转 AtomicBool
[此后每条 tracing 日志 / panic]
  -> GatedMakeWriter -> GatedWriter
       -> LOG_ENABLED == true ? 写当天文件 : 丢弃
```

## 错误处理

- 日志目录创建失败:`create_dir_all` 失败仅告警(stderr/stdout),不阻断 app 启动。
- `open_log_dir`:目录不存在先创建;打开命令失败返回 `Err` 字符串,前端 `.catch` 仅 `console.warn`(与现有 `open_config_dir` 一致)。
- `cleanup_old_logs`:解析失败或删除失败的单个文件跳过,不影响其余;整体不抛错。
- 前端 invoke 失败:`.catch` 静默或 `console.warn`,不阻断 UI。

## 测试

**Rust(`src-tauri/src/logging.rs` 内 `#[cfg(test)]`)**:

- `gated_writer_writes_when_enabled`:enabled=true,写一条日志,断言临时目录当天文件包含内容。
- `gated_writer_discards_when_disabled`:enabled=false,写一条日志,断言文件为空或不含该内容。
- `cleanup_removes_old_logs`:在临时目录造若干含不同日期的 `cc-remote.<date>.log`,调用 `cleanup_old_logs(7)`,断言早于阈值的被删、近 7 天的保留。

**前端(`src/stores/__tests__/settings.test.ts`)**:

- `logToFile` 默认为 `false`。
- `setLogToFile(true)` 写入 `localStorage("logToFile") === "true"` 且以 `{ enabled: true }` 调用 invoke。

## 新增依赖

- `tracing-appender = "0.2"`(按天滚动 + non-blocking)。

## 常量

- `RETAIN_DAYS: u64 = 7`(保留天数)。
- 日志文件前缀:`"cc-remote"`(`tracing_appender::rolling::daily` 生成 `cc-remote.YYYY-MM-DD.log`)。
- localStorage key:`"logToFile"`。
