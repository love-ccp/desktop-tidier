//! 单文件日志 writer：写满 `max_bytes` 后截断为 1 个 `.bak` + 新文件。
//!
//! 禁止使用 `tracing-appender`（会多文件）；本模块自行实现
//! 「超过阈值 → 旧文件改名 `.bak`（仅保留 1 份）→ 新建同名文件」的轮转。

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

/// 单文件日志 writer：始终只有 1 个日志文件 + 至多 1 个 `.bak`。
pub struct SingleFileMakeWriter {
    path: PathBuf,
    max_bytes: u64,
    rotate_lock: Mutex<()>,
}

impl SingleFileMakeWriter {
    /// 新建 writer；`max_bytes` 为触发轮转的字节阈值。
    pub fn new(path: impl Into<PathBuf>, max_bytes: u64) -> Self {
        Self {
            path: path.into(),
            max_bytes,
            rotate_lock: Mutex::new(()),
        }
    }

    /// 打开日志文件（必要时先轮转）。任何失败退回 [`FileWriter::sink`] 而非 panic。
    fn open(&self) -> FileWriter {
        let _guard = self
            .rotate_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.rotate_if_needed().is_err() {
            return FileWriter::sink();
        }
        // 确保日志文件所在目录存在（首次写入时创建）。
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            if std::fs::create_dir_all(parent).is_err() {
                return FileWriter::sink();
            }
        }
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            Ok(file) => FileWriter::file(file),
            Err(_) => FileWriter::sink(),
        }
    }

    /// 当前文件达到阈值时，将其改名 `.bak`（覆盖旧的 `.bak`）。
    fn rotate_if_needed(&self) -> io::Result<()> {
        let meta = match fs::metadata(&self.path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        if meta.len() >= self.max_bytes {
            let bak = self.bak_path();
            let _ = fs::remove_file(&bak);
            fs::rename(&self.path, &bak)?;
        }
        Ok(())
    }

    /// `.bak` 路径：`{log_path}.bak`。
    fn bak_path(&self) -> PathBuf {
        let mut os = self.path.as_os_str().to_os_string();
        os.push(".bak");
        PathBuf::from(os)
    }
}

impl<'a> MakeWriter<'a> for SingleFileMakeWriter {
    type Writer = FileWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.open()
    }
}

/// 写入句柄：优先写文件，失败时静默丢弃（禁止 panic）。
#[derive(Debug)]
pub enum FileWriter {
    File(File),
    Sink,
}

impl FileWriter {
    fn file(file: File) -> Self {
        FileWriter::File(file)
    }

    fn sink() -> Self {
        FileWriter::Sink
    }
}

impl Write for FileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            FileWriter::File(file) => file.write(buf),
            FileWriter::Sink => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            FileWriter::File(file) => file.flush(),
            FileWriter::Sink => Ok(()),
        }
    }
}

/// 初始化订阅器：单文件 writer + 级别过滤 + 「错误级日志 → 系统通知」Layer
/// （是否静默由 `notify.enabled` 决定，热重载即时生效）。
///
/// 进程内仅能成功调用一次。
pub fn init(
    config: &crate::config::Config,
    platform: Arc<dyn crate::platform::PlatformAdapter>,
    config_store: Arc<crate::config::ConfigStore>,
) -> crate::Result<()> {
    let level: tracing::Level = config
        .logger
        .level
        .parse()
        .map_err(|_| crate::Error::Msg(format!("非法日志级别：{}", config.logger.level)))?;
    let writer = SingleFileMakeWriter::new(&config.logger.file, config.logger.max_bytes);
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(writer)
        .with_ansi(false)
        .finish()
        .with(NotifyLayer::new(platform, config_store))
        .try_init()
        .map_err(|e| crate::Error::Msg(format!("日志初始化失败：{e}")))?;
    Ok(())
}

/// 错误级日志 → 系统通知 的 tracing Layer。
///
/// 每次 `error!`（及更严重）日志都会尝试系统通知，是否静默由当前配置
/// `notify.enabled` 决定（热重载即时生效）。
pub struct NotifyLayer {
    platform: Arc<dyn crate::platform::PlatformAdapter>,
    config: Arc<crate::config::ConfigStore>,
}

impl NotifyLayer {
    pub fn new(
        platform: Arc<dyn crate::platform::PlatformAdapter>,
        config: Arc<crate::config::ConfigStore>,
    ) -> Self {
        Self { platform, config }
    }
}

impl<S: Subscriber> Layer<S> for NotifyLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() > tracing::Level::ERROR {
            return;
        }
        let cfg = self.config.get();
        if !cfg.notify.enabled {
            return;
        }
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let message = visitor
            .message
            .unwrap_or_else(|| event.metadata().target().to_string());
        let _ = self.platform.notify(&cfg.notify.app_name, &message);
    }
}

/// 只抓取 `message` 字段的访问器（通知正文用）。
#[derive(Default)]
struct MessageVisitor {
    message: Option<String>,
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        }
    }
}
