# desktop-tidier 总规格（AGENTS.md）

> 每个 agent 会话开始都必须完整读取本文件与 `config/schema.json`、`config/配置说明.md`。
> 本文件是防漂移核心；违反硬性约束的输出一律打回。

## 1. 目标与非目标

**目标**：单进程纯 Rust 的 Windows / Linux(KDE) 桌面课件定期整理后台工具，面向希沃白板长时间开机场景。

**托盘与菜单（固定，已锁定）**：
- 托盘图标：**刷子样式**（Windows 用 `.ico`，Linux 用 PNG/SVG，资源放 `assets/`）。
- 右键菜单固定 4 项，自上而下依次：`设置开机自启`、`立刻整理`、`打开配置文件目录`、`退出`。
- `设置开机自启`：勾选态切换，调 `PlatformAdapter::set_autostart`（Windows 写注册表 Run 键 / Linux 写 `~/.config/autostart/*.desktop`）。
- `立刻整理`：立即触发一次整理（扫描 → 分类 → 移动 → 记账），不等 `scan_interval_secs`；**不触发归档**（归档只按 `archiver.interval_hours` 独立周期执行）。
- `打开配置文件目录`：调 `PlatformAdapter::open_in_file_manager` 打开配置目录（explorer / xdg-open）。
- 无"重载配置/暂停"菜单项：配置与词库的重载走文件监听自动完成。

**非目标（明确不做）**：
- 不做 GUI 窗口（无 tao/winit/egui/iced）；托盘仅有菜单，无窗口。
- 不引入 Qt / GTK / WebView / Tauri（Linux 托盘用纯 Rust 的 `ksni`）。
- 不支持网络盘 / 云同步 / NAS 挂载路径（不主动排除，但不做特殊处理）。
- 不支持多实例（单实例锁直接拒绝第二个实例）。
- 不做文件内容解析（只按文件名/后缀/元数据操作）。

## 2. 硬性约束（任何模块都必须遵守）

1. `cargo clippy -- -D warnings` 必须零警告通过；`cargo fmt --check` 通过；`cargo test` 全绿。
2. 不得修改已冻结的接口签名（见 §5）。需要改接口 → 停下来说明理由，等人工批准。
3. 不得新增未经批准的依赖；依赖只允许 Cargo.toml 中已列出的 crate，新增需在 PR 描述中说明并等批准。
4. 所有时间获取一律走 `Clock` trait；禁止直接调 `std::time::SystemTime::now()`、`chrono::Utc::now()` 于业务路径。
5. 所有平台调用（锁/回收站/通知/桌面路径/默认配置目录/开机自启/打开目录）一律走 `PlatformAdapter`，业务层禁止出现 `#[cfg(windows)]` / `#[cfg(target_os)]`。
6. 业务路径禁止 `unwrap()` / `expect()` / `panic!()`；库层禁止 `std::process::exit`。
7. 禁止阻塞 main 线程或 tokio worker；阻塞 IO（目录扫描、压缩、trash）用 `spawn_blocking`。
8. 单实例锁必须在构建托盘前完成。

## 3. 技术栈与线程模型

已锁定依赖（M0 生成并提交 `Cargo.lock`）：
`tokio`、`tray-icon`+`muda`（仅 Windows）、`ksni`（仅 Linux）、`notify-rust`、`notify`+`notify-debouncer-full`、`jwalk`、`aho-corasick`、`trash`、`zip`、`chrono`、`serde`+`serde_json`+`jsonschema`、`tracing`+`tracing-subscriber`、`reqwest`（default-features=false + rustls-tls）、`chardetng`+`encoding_rs`、`dirs`、`thiserror`、`anyhow`、`rustix`、`libc`、`windows`（仅 Windows，按需 feature）。

线程模型：
```
main 线程：单实例锁 → Runtime::new() → rt.spawn 各服务 → 构建托盘
          → Windows: tray-icon 事件循环(阻塞) / Linux: ksni spawn 自带线程后 park 等待
tokio worker：scheduler / watcher / organizer / archiver / indexer / timesync
托盘菜单（设置开机自启 / 立刻整理 / 打开配置文件目录 / 退出）→ mpsc::Sender → tokio 侧处理
退出：托盘 quit → 各任务 graceful shutdown → drop runtime → 删除托盘图标
```
禁止用 `#[tokio::main]` 直接包 main；用 `Runtime::new()` 手动建。

## 4. 仓库结构

```
desktop-tidier/
├─ Cargo.toml            # 单 crate，lib + bin 分离（lib 可测，bin 薄）
├─ Cargo.lock            # 必须提交
├─ justfile              # lint/test/build/package 统一入口
├─ AGENTS.md             # 本文件
├─ config/
│  ├─ schema.json        # JSON Schema（draft-07）
│  ├─ config.example.json
│  ├─ 配置说明.md        # 逐字段中文编写说明
│  └─ lexicon.txt        # 外置词库：关键词<TAB>分组
├─ src/
│  ├─ main.rs            # 组装：runtime + tray + 各服务（薄）
│  ├─ config/            # Config 结构、加载、Schema 校验、示例生成、热重载
│  ├─ platform/          # PlatformAdapter trait + linux.rs + windows.rs
│  ├─ scheduler/         # 单调时钟定时、任务编排
│  ├─ watcher/           # notify 监听 + 防抖 + 周期兜底全量扫（默认两天）
│  ├─ organizer/         # 后缀/时间筛选 → 分类 → 移动（重试队列）
│  ├─ archiver/          # zip6 归档 + 重命名 + 回收站 + 回滚
│  ├─ indexer/           # MD 台账、编号顺延、编码探测
│  ├─ logger/            # 单文件 MakeWriter、格式
│  ├─ timesync/          # HTTP Date 授时 + 偏移落盘 + Clock 实现
│  ├─ classifier/        # aho-corasick 关键词分组
│  └─ error.rs
├─ tests/                # 集成测试（tempfile 沙盒 + 固定时钟）
├─ assets/               # 刷子样式托盘图标（Windows icon.ico / Linux icon.png）
└─ packaging/            # PKGBUILD / desktop / systemd / Inno Setup / CI
```

## 5. 模块边界与冻结接口

以下为冻结签名（业务层不得改动；实现内部自由）：

```rust
// platform/mod.rs —— 阻塞方法，业务层用 spawn_blocking 包裹
pub trait PlatformAdapter: Send + Sync {
    fn acquire_single_instance(&self) -> Result<SingleInstanceGuard>;
    fn trash(&self, paths: &[PathBuf]) -> Result<()>;
    fn notify(&self, title: &str, body: &str) -> Result<()>;
    fn desktop_dir(&self) -> Option<PathBuf>;
    fn default_config_dir(&self) -> PathBuf;
    fn created(&self, path: &Path) -> Result<std::time::SystemTime>;  // 创建时间（Windows 创建时间；Linux 无 btime 时回退 mtime）
    fn set_autostart(&self, enabled: bool) -> Result<bool>;   // 开机自启（注册表 Run / autostart .desktop）
    fn is_autostart_enabled(&self) -> Result<bool>;           // 读当前自启状态（菜单勾选态）
    fn open_in_file_manager(&self, path: &Path) -> Result<()>; // 打开配置目录（explorer / xdg-open）
}
pub struct SingleInstanceGuard { /* Drop 时释放锁 */ }

// timesync/clock.rs —— 唯一的时间来源
pub trait Clock: Send + Sync {
    fn now(&self) -> chrono::DateTime<chrono::Utc>;   // 授时校正后的墙钟
    fn now_mono(&self) -> tokio::time::Instant;       // 单调时钟（定时/间隔用）
}

// config/mod.rs
pub struct Config { /* 字段见 schema.json，加载后不可变 */ }
impl Config {
    pub fn load(path: &Path) -> Result<Self>;         // 含 schema 校验，失败返回带"路径+字段名+建议"的错误
    pub fn write_example(path: &Path) -> Result<()>;  // 生成 config.example.json
}
pub struct ConfigStore { /* ArcSwap<Arc<Config>> */ }
impl ConfigStore {
    pub fn get(&self) -> Arc<Config>;
    pub fn reload(&self) -> Result<ReloadOutcome>;    // 成功替换 / 失败保留旧值并返回错误
}
pub enum ReloadOutcome { Reloaded(Arc<Config>), Rejected(String) }

// classifier/mod.rs
pub struct Classifier { /* aho-corasick automaton: pattern_id -> group */ }
impl Classifier {
    pub fn classify(&self, filename: &str) -> Option<&str>;  // 命中返回分组，未命中 None
}

// organizer/mod.rs
pub struct OrganizeReport { pub moved: usize, pub skipped: usize, pub failed: usize }
pub struct Organizer { /* deps 注入 */ }
impl Organizer { pub async fn run_once(&self, cfg: &Config) -> Result<OrganizeReport>; }

// archiver/mod.rs
pub struct ArchiveReport { pub archived: usize, pub zipped_to: PathBuf, pub recycled: usize, pub purged: usize }
pub struct Archiver { /* deps 注入 */ }
impl Archiver { pub async fn run_once(&self, cfg: &Config) -> Result<ArchiveReport>; }

// indexer/mod.rs
pub struct Record { pub filename: String, pub group: String, pub date: chrono::NaiveDate }
pub struct Indexer { /* deps 注入 */ }
impl Indexer {
    pub fn max_existing_no(&self, md_path: &Path) -> Result<u64>;  // 扫描已有 MD 取最大编号
    pub fn record(&self, cfg: &Config, entry: &Record) -> Result<u64>; // 追加一行，返回编号；临时文件+rename 原子写
}

// logger/mod.rs
pub struct SingleFileMakeWriter { /* 截断 + 单 .bak */ }
impl tracing_subscriber::fmt::MakeWriter for SingleFileMakeWriter { /* 见 §8 */ }
```

## 6. 配置字段表（全量，详细见 config/配置说明.md）

| 路径 | 类型 | 默认 | 含义 |
|---|---|---|---|
| `watcher.source_dir` | string | "" | 源目录（桌面）；空 = 自动探测桌面 |
| `watcher.extensions` | string[] | 见示例 | 特定后缀（不带点，小写） |
| `watcher.min_age_days` | uint | 7 | 创建时间距今超过 N 天（默认 7；无创建时间时回退修改时间；拷贝/下载文件以「到电脑上」的时间为准） |
| `watcher.debounce_ms` | uint | 400 | 事件防抖 |
| `watcher.scan_interval_secs` | uint | 172800 | 扫描整理频率（秒），默认两天一次；按「至多每隔 N」宽松执行（启动时超期即追赶） |
| `storage.inbox_dir` | string | storage/待分类 | 步骤1移动目标（指定目录） |
| `storage.course_dir` | string | storage/课件库 | 步骤2分组根，文件进 `course_dir/{group}/` |
| `storage.archive_dir` | string | storage/归档 | 步骤3归档目标 |
| `storage.recycle_after_archive` | bool | true | 归档成功后清空 course_dir 至回收站 |
| `classifier.lexicon_path` | string | lexicon.txt | 词库路径（与 config.json 同目录，扁平布局） |
| `classifier.lexicon_encoding` | string | auto | auto/utf-8/gbk |
| `classifier.default_group` | string | 未分类 | 未命中关键词的分组 |
| `archiver.interval_hours` | uint | 168 | 归档间隔（小时），默认一周；归档与整理完全解耦，只按本周期执行（启动时超期即追赶） |
| `archiver.zip_level` | uint | 6 | 压缩等级（固定要求 6，仍可配） |
| `archiver.rename_pattern` | string | {date}_{seq}_desktop-tidier.zip | 重命名规则，占位符见说明 |
| `archiver.max_archives` | uint | 0 | 归档 zip 最大保留数量；0 = 不限制，超出时最旧的移入回收站 |
| `archiver.max_total_bytes` | uint | 0 | 归档 zip 最大总占用字节；0 = 不限制，超出时最旧的移入回收站 |
| `archiver.min_new_files` | uint | 1 | 归档前按台账比对的新增文件数下限；低于该值跳过本次归档；0 = 不限制 |
| `indexer.md_path` | string | storage/台账.md | 台账路径 |
| `indexer.md_encoding` | string | auto | 读取现有编码再顺延 |
| `indexer.entry_format` | string | 见示例 | 单条记录格式，占位符 {no}{filename}{group}{date} |
| `logger.file` | string | logs/desktop-tidier.log | 日志文件（单文件） |
| `logger.level` | string | info | trace/debug/info/warn/error |
| `logger.max_bytes` | uint | 1048576 | 超过则截断为 1 个 .bak + 新文件 |
| `timesync.enabled` | bool | true | 是否授时 |
| `timesync.servers` | string[] | 见示例 | HTTPS 授时 URL，读 `Date` 头 |
| `timesync.interval_hours` | uint | 1 | 启动时与每 N 小时储存 |
| `timesync.timeout_secs` | uint | 10 | 单次请求超时 |
| `notify.enabled` | bool | true | 系统通知开关 |
| `notify.app_name` | string | 课件整理助手 | 通知来源名 |
| `concurrency.max_parallel_ops` | uint | 4 | 并发文件操作数 |

> 运行状态文件 `storage/state.json`（上次整理/归档时间与台账水位）由程序自动维护，支撑「启动追赶」与「归档阈值比对」，用户不应手改。首次运行生成的配置目录为扁平布局：`config.json` 是唯一生效配置，`lexicon.txt` 是唯一生效词库，其余（schema.json / config.example.json / 配置说明.md）均为参考文件。

**错误提示格式（已锁定）**：
```
配置校验失败：字段 "watcher.min_age_days" 期望 非负整数(0~36500)，实际得到 "abc"
建议：请改为整数，例如 3
```
即 `JSON Pointer 路径 + 字段名 + 期望类型/取值 + 实际值 + 一条改正建议`，不做精确行号。

## 7. 词库格式（lexicon.txt）

- 每行一条：`关键词<TAB>分组`；`#` 开头为注释；空行跳过。
- 关键词大小写不敏感（ASCII），按整条文件名（含扩展名）匹配；一行含多个 TAB 或非法分组 → 该行跳过 + 系统通知 + 改正建议。
- 关键词重复：首个生效，后续重复记 warn 日志（不中断）。
- 编码：按 `classifier.lexicon_encoding`；auto 时用 chardetng 探测。
- 示例：
  ```
  # 关键词<TAB>分组
  二次函数	数学
  三角函数	数学
  牛顿定律	物理
  电磁感应	物理
  ```

## 8. 日志格式（单文件）

- 唯一日志文件 + 至多 1 个 `.bak`（写满 `logger.max_bytes` 后：旧文件改名 `.bak`，新建同名继续写）。禁止用 `tracing-appender`（会多文件）。
- 每行 JSON 或 `时间 [级别] 模块: 消息`，采用 `tracing-subscriber` fmt，自定义 `SingleFileMakeWriter`。
- 级别受 `logger.level` 控制；所有错误级别日志必须同时尝试系统通知（受 `notify.enabled`）。
- 禁止把文件内容/路径之外的敏感信息写进日志（本项目无账号密码，无需脱敏，但保留脱敏扩展点）。

## 9. MD 台账格式

- 追加式单文件；新写入前先读现有文件编码（chardetng），按相同编码继续追加（"读取现有的再顺延"）。
- 编号顺延：启动/首次写入时扫描现有行取最大 `{no}`，`+1` 继续。
- `indexer.entry_format` 占位符：`{no}` 编号、`{filename}` 文件名、`{group}` 分组、`{date}` 操作日期（授时校正后的本地日期，格式 `%Y-%m-%d`）。
- 写入用临时文件 + `rename` 原子替换。

## 10. 验收命令（每个 PR 必须通过）

```
just fmt    # cargo fmt --check
just lint   # cargo clippy -- -D warnings
just test   # cargo test
just build  # cargo build --release（Linux）；M11 起含 --target x86_64-pc-windows-msvc
```

## 11. 禁止事项

- 禁止新增 GUI / WebView / Qt / GTK 依赖。
- 禁止修改日志格式、配置结构、词库格式（M1/M2 冻结后）。
- 禁止在业务层使用 `#[cfg]` 平台分支（统一走 PlatformAdapter）。
- 禁止直接读写挂载点之外的非目标路径（仅操作 source_dir / storage.* 下内容）。
- 移动/回收站操作必须"先校验目标不存在 → 跨卷则 copy+delete+校验 → 失败回滚"，占用文件入重试队列（最多 3 次，指数退避），仍失败记日志并通知。

## 12. 里程碑（一个 PR 一个，冻结顺序 M1→M2→其余）

M0 骨架+justfile+CI+Cargo.lock → M1 Config+Schema → M2 logger → M3 platform(Linux) → M4 timesync+Clock → M5 organizer → M6 indexer → M7 archiver → M8 watcher+热重载 → M9 tray(刷子图标+四菜单) → M10 Linux 打包 → M11 Windows 适配+交叉编译+单文件 exe（首个交付物）。
