//! 归档器（M7）：把 course_dir/** 压缩为 zip（等级 6）到 archive_dir，
//! 成功后按 `recycle_after_archive` 将 course_dir 内文件清空至回收站；
//! 并按 `archiver.max_archives` / `archiver.max_total_bytes` 管理归档总量
//! （超限时最旧的 zip 移入回收站，0 = 不限制）。

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::config::Config;
use crate::platform::PlatformAdapter;
use crate::timesync::Clock;
use crate::{Error, Result};

/// 单次归档结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveReport {
    pub archived: usize,
    pub zipped_to: PathBuf,
    pub recycled: usize,
    /// 因超出 `max_archives`/`max_total_bytes` 上限被移入回收站的旧归档数。
    pub purged: usize,
}

/// 归档器（deps 注入）。
pub struct Archiver {
    platform: Arc<dyn PlatformAdapter>,
    clock: Arc<dyn Clock>,
}

impl Archiver {
    pub fn new(platform: Arc<dyn PlatformAdapter>, clock: Arc<dyn Clock>) -> Self {
        Self { platform, clock }
    }

    /// 执行一次归档（阻塞 IO 放入 `spawn_blocking`）。
    pub async fn run_once(&self, cfg: &Config) -> Result<ArchiveReport> {
        let cfg = cfg.clone();
        let platform = Arc::clone(&self.platform);
        let clock = Arc::clone(&self.clock);
        tokio::task::spawn_blocking(move || {
            let archiver = Archiver { platform, clock };
            archiver.run_sync(&cfg)
        })
        .await
        .map_err(|e| Error::Msg(format!("归档任务异常退出：{e}")))?
    }

    fn run_sync(&self, cfg: &Config) -> Result<ArchiveReport> {
        let course = PathBuf::from(&cfg.storage.course_dir);
        let archive = PathBuf::from(&cfg.storage.archive_dir);
        let files = collect_files(&course)?;

        let target = self.next_archive_path(cfg, &archive);
        let mut report = ArchiveReport {
            archived: 0,
            zipped_to: target.clone(),
            recycled: 0,
            purged: 0,
        };

        if files.is_empty() {
            tracing::info!("course_dir 为空，跳过归档");
            return Ok(report);
        }

        std::fs::create_dir_all(&archive)?;
        // 先压缩到临时文件，成功后再 rename 到最终名，避免半成品 zip。
        let tmp = tmp_path(&target);
        self.build_zip(&course, &files, &tmp, cfg.archiver.zip_level as i64)?;
        std::fs::rename(&tmp, &target)?;
        report.archived = files.len();

        if cfg.storage.recycle_after_archive {
            report.recycled = self.recycle(&files)?;
        }
        report.purged = self.enforce_limits(cfg, &archive, &target);
        Ok(report)
    }

    /// 压缩 `files`（相对 `base` 保留分组目录结构）到 `out`。
    fn build_zip(&self, base: &Path, files: &[PathBuf], out: &Path, level: i64) -> Result<()> {
        let file = File::create(out)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(level));
        for path in files {
            let name = zip_entry_name(base, path)?;
            let data = std::fs::read(path)?;
            zip.start_file(name, options)
                .map_err(|e| Error::Msg(format!("写入 zip 条目失败：{e}")))?;
            zip.write_all(&data)?;
        }
        zip.finish()
            .map_err(|e| Error::Msg(format!("完成 zip 写入失败：{e}")))?;
        Ok(())
    }

    /// 计算下一个归档文件名（占位符 {date}/{time}/{seq}，seq 递增避免重名）。
    fn next_archive_path(&self, cfg: &Config, archive: &Path) -> PathBuf {
        let now = self.clock.now().with_timezone(&chrono::Local);
        let date = now.format("%Y%m%d").to_string();
        let time = now.format("%H%M%S").to_string();
        for seq in 1..=9999u32 {
            let name = render_name(&cfg.archiver.rename_pattern, &date, &time, seq);
            let target = archive.join(name);
            if !target.exists() {
                return target;
            }
        }
        archive.join(render_name(
            &cfg.archiver.rename_pattern,
            &date,
            &time,
            10_000,
        ))
    }

    /// 清空 course_dir 文件至回收站，返回回收数量。
    fn recycle(&self, files: &[PathBuf]) -> Result<usize> {
        if files.is_empty() {
            return Ok(0);
        }
        self.platform
            .trash(files)
            .map_err(|e| Error::Msg(format!("清空 course_dir 至回收站失败：{e}")))?;
        Ok(files.len())
    }

    /// 归档上限管理：仅统计 `archive_dir` 顶层的 `*.zip`；超出
    /// `max_archives`（数量）或 `max_total_bytes`（总字节）时，从最旧开始把多余的
    /// zip 移入回收站（0 = 不限制）。绝不删除本次刚生成的 `target`。
    fn enforce_limits(&self, cfg: &Config, archive: &Path, target: &Path) -> usize {
        if cfg.archiver.max_archives == 0 && cfg.archiver.max_total_bytes == 0 {
            return 0;
        }
        let mut zips = match list_archive_zips(archive) {
            Ok(z) => z,
            Err(e) => {
                tracing::warn!("扫描归档目录失败（跳过上限管理）：{e}");
                return 0;
            }
        };
        // 默认命名以日期开头，字典序即时间序，最新的在末尾。
        zips.sort();

        let mut purged = 0usize;
        loop {
            let count = zips.len() as u32;
            let total: u64 = zips
                .iter()
                .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
                .sum();
            let over_count = cfg.archiver.max_archives > 0 && count > cfg.archiver.max_archives;
            let over_size =
                cfg.archiver.max_total_bytes > 0 && total > cfg.archiver.max_total_bytes;
            if !over_count && !over_size {
                break;
            }
            // 从最旧（且不是本次产物）的开始清理。
            let Some(idx) = zips.iter().position(|p| p != target) else {
                break; // 只剩本次产物本身：即使仍超限也不再删除（保护数据）。
            };
            let victim = zips.remove(idx);
            match self.platform.trash(std::slice::from_ref(&victim)) {
                Ok(()) => {
                    purged += 1;
                    tracing::info!("归档超出上限，已将 {victim:?} 移入回收站");
                }
                Err(e) => {
                    tracing::warn!("移入回收站失败（停止上限管理）：{e}");
                    break;
                }
            }
        }
        purged
    }
}

/// 列出 `archive` 顶层的 `*.zip` 文件。
fn list_archive_zips(archive: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(archive)? {
        let entry = entry?;
        let path = entry.path();
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        let is_zip = path.extension().and_then(|e| e.to_str()) == Some("zip");
        if is_file && is_zip {
            out.push(path);
        }
    }
    Ok(out)
}

/// 递归收集目录下所有文件。
fn collect_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk_files(dir, &mut out)?;
    Ok(out)
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let ft = entry.file_type()?;
                if ft.is_dir() {
                    walk_files(&entry.path(), out)?;
                } else if ft.is_file() {
                    out.push(entry.path());
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

/// zip 内条目名：相对 `base` 的路径，统一 `/` 分隔（保留分组目录）。
fn zip_entry_name(base: &Path, path: &Path) -> Result<String> {
    let rel = path
        .strip_prefix(base)
        .map_err(|e| Error::Msg(format!("路径 {path:?} 不在归档根 {base:?} 下：{e}")))?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

fn render_name(pattern: &str, date: &str, time: &str, seq: u32) -> String {
    pattern
        .replace("{date}", date)
        .replace("{time}", time)
        .replace("{seq}", &seq.to_string())
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_path(path: &Path) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut os = path.as_os_str().to_os_string();
    os.push(format!(".tmp{}_{}", std::process::id(), n));
    PathBuf::from(os)
}
