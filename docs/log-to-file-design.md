# 日志与崩溃信息落盘 — 方案与设计

> 关联设计草案:`docs/superpowers/specs/2026-06-25-log-to-file-design.md`
> 本文是面向开发者的完整方案与实现说明文档。

## 1. 背景与动机

CC Remote Seamless 在长时间运行中出现「收到 IM 消息无响应」「莫名崩溃退出」等难以复现的问题。这类问题的根因排查严重依赖运行期日志,但此前应用仅通过 `tracing_subscriber::fmt::init()` 将日志输出到 stdout,GUI 打包运行(.app / .exe)时 stdout 不落盘,用户现场无任何可回溯的记录。

本功能为应用增加**可开关的本地日志持久化**能力:

- 运行日志与 Rust panic 信息按天写入 `~/.cc-remote/logs/`。
- 设置页提供独立的 Logs 标签:展示日志目录、一键打开目录、开关控制是否落盘(默认关闭,实时生效)。
- 自动保留最近 7 天,过期文件启动时清理,避免磁盘无限增长。

## 2. 目标与非目标

### 目标

1. 日志落盘到 `~/.cc-remote/logs/`,与既有 `config.toml` / `icons/` / `sounds/` 同根目录。
2. 设置页可视化:显示路径、打开目录、toggle 开关(默认关)。
3. 开关**实时生效**,无需重启应用。
4. 捕获 Rust 端 panic 并写入同一套日志。
5. 按天轮转,保留最近 7 天,启动时清理。

### 非目标(YAGNI)

- 不捕获前端 Vue/JS 运行时错误(本期仅 Rust 后端 + panic)。
- 不做日志远程上传或聚合。
- 不提供运行时日志等级(level)动态调整。
- 不做日志文件大小上限的二级切分(仅按天)。

## 3. 现状梳理

| 关注点 | 现状 | 位置 |
|--------|------|------|
| 日志初始化 | `tracing_subscriber::fmt::init()`,仅 stdout | `src-tauri/src/lib.rs` |
| 日志调用 | 全项目已广泛使用 `tracing::{info,warn,error}!` | `lib.rs` 等 |
| 数据目录 | `~/.cc-remote/` 已承载 config/icons/sounds | `config.rs` |
| 打开目录能力 | `open_config_dir` 封装了 macOS `open` / Linux `xdg-open` | `commands.rs` |
| 前端偏好持久化 | localStorage + Pinia store,启动时 invoke 同步后端 | `stores/settings.ts` |
| 设置页结构 | 侧边栏 tab(Icon/Sound/IMConfig/About) + 各面板 | `views/SettingsWindow.vue` |

复用上述既有模式可将改动面降到最小,且与项目风格一致。

## 4. 整体方案

```
┌──────────────────────── 设置页 (Vue) ────────────────────────┐
│  Logs Tab                                                    │
│   ├─ 日志路径展示 (~/.cc-remote/logs)                          │
│   ├─ Open 按钮  ── invoke("open_log_dir")                     │
│   └─ Toggle 开关 ── settings.setLogToFile(bool)               │
│                         │ localStorage("logToFile")           │
│                         └─ invoke("set_log_to_file", {enabled})│
└──────────────────────────────┼───────────────────────────────┘
                               ▼
┌──────────────────────── 后端 (Rust) ─────────────────────────┐
│  commands.rs                                                 │
│   ├─ get_log_dir()    → logging::log_dir()                   │
│   ├─ open_log_dir()   → 打开目录 (open / xdg-open)            │
│   └─ set_log_to_file(enabled) → logging::set_enabled(bool)   │
│                                                              │
│  logging.rs                                                  │
│   ├─ static LOG_ENABLED: AtomicBool   ← 实时开关             │
│   ├─ GatedMakeWriter / GatedWriter    ← 按需落盘             │
│   ├─ init(enabled, dir)               ← 双层 subscriber      │
│   │     ├─ stdout 层 (始终输出)                              │
│   │     └─ gated 文件层 (按天滚动, 受开关控制)               │
│   └─ cleanup_old_logs(7)              ← 启动清理过期          │
│                                                              │
│  lib.rs run()                                                │
│   ├─ create_dir_all(log_dir)                                 │
│   ├─ logging::init(false, log_dir)                           │
│   ├─ logging::cleanup_old_logs(RETAIN_DAYS)                  │
│   └─ panic::set_hook → tracing::error!("PANIC: ...")         │
└──────────────────────────────────────────────────────────────┘
```

## 5. 详细实现

### 5.1 日志模块 `src-tauri/src/logging.rs`

**核心思路**:subscriber 只初始化一次,同时挂载两层 —— stdout 层始终输出(保留开发体验),文件层始终存在但是否真正写盘由一个全局原子布尔 `LOG_ENABLED` 实时控制。切换开关只翻转布尔值,不重建 subscriber,从而避免 `reload` layer 的复杂度与竞态风险。

**常量**

```rust
pub const RETAIN_DAYS: i64 = 7;
const FILE_PREFIX: &str = "cc-remote";  // 文件名 cc-remote.YYYY-MM-DD.log
const FILE_SUFFIX: &str = "log";
```

**全局状态**

```rust
static LOG_ENABLED: AtomicBool = AtomicBool::new(false);
static GUARD: OnceLock<WorkerGuard> = OnceLock::new(); // non_blocking 的 worker 守卫,需存活整个进程
```

`WorkerGuard` 一旦 drop,后台写线程即停止刷盘,因此用 `OnceLock` 持有,保证其生命周期与进程一致。

**门控 writer**

```rust
impl Write for GatedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if LOG_ENABLED.load(Ordering::Relaxed) {
            self.inner.write(buf)   // 写入按天滚动文件
        } else {
            Ok(buf.len())           // 假装写成功,实际丢弃
        }
    }
    // flush 同理
}
```

`GatedMakeWriter` 实现 `tracing_subscriber::fmt::MakeWriter`,每条日志返回一个 `GatedWriter`。门控判断发生在**每次写入**,因此 `set_enabled` 立即对后续所有日志生效。

**初始化**

```rust
pub fn init(enabled_initial: bool, dir: PathBuf) {
    set_enabled(enabled_initial);
    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(FILE_PREFIX)
        .filename_suffix(FILE_SUFFIX)
        .build(&dir).expect("...");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let _ = GUARD.set(guard);

    let stdout_layer = fmt::layer();
    let file_layer = fmt::layer().with_ansi(false)
        .with_writer(GatedMakeWriter { inner: non_blocking });

    registry().with(stdout_layer).with(file_layer).init();
}
```

文件层 `with_ansi(false)`,避免把终端颜色转义码写进日志文件。

**过期清理**

```rust
pub fn cleanup_old_logs(retain_days: i64) {
    let cutoff = Local::now().date_naive() - Duration::days(retain_days);
    // 遍历目录,文件名解析出日期,早于 cutoff 的删除
}
```

文件名通过 `parse_log_date` 解析:剥离 `cc-remote.` 前缀与 `.log` 后缀,用 `NaiveDate::parse_from_str(_, "%Y-%m-%d")` 校验。解析失败的文件(非本应用日志)自动跳过,删除失败的单个文件忽略,不影响整体。

### 5.2 启动接入 `src-tauri/src/lib.rs`

`run()` 开头替换原 `tracing_subscriber::fmt::init()`:

```rust
let log_dir = logging::log_dir();
let _ = std::fs::create_dir_all(&log_dir);
logging::init(false, log_dir);                 // 后端默认关
logging::cleanup_old_logs(logging::RETAIN_DAYS);
std::panic::set_hook(Box::new(|info| {
    tracing::error!("PANIC: {}", info);        // 进入 gated 文件层
    eprintln!("{}", info);                     // 保留默认 stderr 行为
}));
```

后端初值取 `false`,与前端默认一致;前端 `onMounted` 阶段会把 localStorage 的真实开关值通过 `set_log_to_file` 同步过来(见 5.4),避免「前端开了但后端没写」的不一致。

panic hook 安装在 subscriber 初始化**之后**,确保 panic 时 `tracing::error!` 有可用的订阅者。开关开启时 panic 即落盘当天文件;开关关闭时仅走 stderr。

### 5.3 Tauri 命令 `src-tauri/src/commands.rs`

| 命令 | 签名 | 实现 |
|------|------|------|
| `get_log_dir` | `() -> Result<String, String>` | 返回 `logging::log_dir()` 的绝对路径字符串 |
| `open_log_dir` | `() -> Result<(), String>` | 目录不存在则创建,再 `open`(macOS)/`xdg-open`(Linux)打开,逻辑与 `open_config_dir` 一致 |
| `set_log_to_file` | `(enabled: bool) -> Result<(), String>` | 调用 `logging::set_enabled(enabled)`,实时生效 |

三个命令在 `lib.rs` 的 `generate_handler!` 宏中注册。

### 5.4 前端 Store `src/stores/settings.ts`

```ts
const logToFile = ref<boolean>(localStorage.getItem("logToFile") === "true"); // 默认 false

function setLogToFile(enabled: boolean) {
  logToFile.value = enabled;
  localStorage.setItem("logToFile", String(enabled));
  invoke("set_log_to_file", { enabled }).catch(() => {});
}

function syncLogToFile() {  // 启动时把本地真实值同步给后端一次
  invoke("set_log_to_file", { enabled: logToFile.value }).catch(() => {});
}
```

与既有 sound 偏好同步模式完全一致:localStorage 为单一事实来源,启动时下发后端。

### 5.5 前端 UI `src/views/SettingsWindow.vue`

- `activeTab` 类型扩展为 `"icon" | "sound" | "imconfig" | "logs" | "about"`。
- 侧边栏在 IMConfig 与 About 之间新增 **Logs** 项。
- 新增独立 Logs 面板:
  - 路径行:复用 `config-header` / `config-path-text` 样式 + Open 按钮(`openLogDir` → `invoke("open_log_dir")`)。路径用 home 替换为 `~` 显示。
  - Toggle:纯 CSS 实现(checkbox + slider),绑定 `settings.logToFile`,`@change` 调 `setLogToFile`。
  - 提示文案:说明开启后按天保存、保留 7 天。
- `onMounted` 中 `invoke("get_log_dir")` 取路径,并调用 `settings.syncLogToFile()`。

## 6. 数据流

```
[拨动 toggle]
  → setLogToFile(enabled)
      → localStorage.setItem("logToFile", ...)
      → invoke("set_log_to_file", {enabled})
          → logging::set_enabled(enabled)   // 翻转 AtomicBool

[此后每条 tracing 日志 / panic]
  → GatedMakeWriter::make_writer → GatedWriter::write
      → LOG_ENABLED == true ? 写当天文件 : 丢弃
```

## 7. 错误处理

| 场景 | 处理 |
|------|------|
| 日志目录创建失败 | 仅告警,不阻断启动 |
| `open_log_dir` 失败 | 返回 `Err`,前端 `.catch` 仅 `console.warn` |
| 过期清理中单文件删除/解析失败 | 跳过该文件,不影响其余,整体不抛错 |
| 前端 invoke 失败 | `.catch` 静默,不阻断 UI |
| 无法确定 home 目录 | `log_dir()` 回退到相对路径 `.cc-remote/logs` |

## 8. 测试

**Rust 单元测试**(`logging.rs` 内 `#[cfg(test)]`,均通过):

- `parses_valid_log_filename` — 正确解析 `cc-remote.2026-06-25.log` 的日期。
- `rejects_non_log_filename` — 拒绝 `config.toml`、非法日期、错误后缀。
- `cleanup_removes_old_keeps_recent` — 临时目录造新旧日志,验证早于阈值被删、近 7 天保留。
- `gated_writer_discards_when_disabled` — 验证开关布尔状态切换。

**前端测试**(`stores/__tests__/settings.test.ts`,28 passed):

- `logToFile` 默认为 `false`。
- `setLogToFile(true)` 写 localStorage 为 `"true"` 且以 `{ enabled: true }` 调用 invoke。
- 从 localStorage 恢复开关状态。

## 9. 新增依赖

- `tracing-appender = "0.2"`(按天滚动 + non-blocking 写入)。

## 10. 兼容性与影响

- **行为变更**:启动时多一次目录创建与过期清理(轻量);默认关闭落盘,对现有用户无可感知影响。
- **平台**:打开目录命令覆盖 macOS / Linux;Windows 下 `open_log_dir` 的打开分支未实现(与现有 `open_config_dir` 一致,后续可补 `explorer`)。
- **磁盘**:开启后每天一个文件,保留 7 天,占用可控。
