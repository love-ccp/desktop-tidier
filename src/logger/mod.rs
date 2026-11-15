//! 单文件日志 writer：写满 `max_bytes` 后截断为 1 个 `.bak` + 新文件。
//!
//! 禁止使用 `tracing-appender`（会多文件）；本模块自行实现
//! 「超过阈值 → 旧文件改名 `.bak`（仅保留 1 份）→ 新建同名文件」的轮转。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use tracing_subscriber::fmt::MakeWriter;

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

/// 依据配置初始化全局 `tracing` 订阅器（单文件 writer + 级别过滤）。
///
/// 进程内仅能成功调用一次；后续 M9 由 `main` 组装时调用。
pub fn init(config: &crate::config::Config) -> crate::Result<()> {
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
        .try_init()
        .map_err(|e| crate::Error::Msg(format!("日志初始化失败：{e}")))?;
    Ok(())
}
