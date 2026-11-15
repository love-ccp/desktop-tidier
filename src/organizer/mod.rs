//! 整理器（M5）：扫描 → 分类 → 移动（source → inbox → course_dir/{分组}）→ 记账。
//!
//! 移动失败自动重试（最多 3 次，指数退避）；跨卷回退 copy+delete+校验；
//! 目标已存在则跳过（绝不覆盖）。阻塞 IO 全部放入 `spawn_blocking`。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use crate::classifier::Classifier;
use crate::config::Config;
use crate::indexer::{Indexer, Record};
use crate::platform::PlatformAdapter;
use crate::timesync::Clock;
use crate::{Error, Result};

const MAX_RETRIES: u32 = 3;

/// 单次整理结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrganizeReport {
    pub moved: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// 整理器（deps 注入）。
pub struct Organizer {
    platform: Arc<dyn PlatformAdapter>,
    classifier: Arc<Classifier>,
    indexer: Arc<Indexer>,
    clock: Arc<dyn Clock>,
}

impl Organizer {
    pub fn new(
        platform: Arc<dyn PlatformAdapter>,
        classifier: Arc<Classifier>,
        indexer: Arc<Indexer>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            platform,
            classifier,
            indexer,
            clock,
        }
    }

    /// 执行一次完整整理（扫描 + 分类 + 移动 + 记账）。
    pub async fn run_once(&self, cfg: &Config) -> Result<OrganizeReport> {
        let cfg = cfg.clone();
        let platform = Arc::clone(&self.platform);
        let classifier = Arc::clone(&self.classifier);
        let indexer = Arc::clone(&self.indexer);
        let clock = Arc::clone(&self.clock);
        tokio::task::spawn_blocking(move || {
            let organizer = Organizer {
                platform,
                classifier,
                indexer,
                clock,
            };
            organizer.run_sync(&cfg)
        })
        .await
        .map_err(|e| Error::Msg(format!("整理任务异常退出：{e}")))?
    }

    fn run_sync(&self, cfg: &Config) -> Result<OrganizeReport> {
        let source = self.resolve_source(cfg)?;
        let inbox = PathBuf::from(&cfg.storage.inbox_dir);
        let course = PathBuf::from(&cfg.storage.course_dir);
        let archive = PathBuf::from(&cfg.storage.archive_dir);

        let extensions: HashSet<String> = cfg.watcher.extensions.iter().cloned().collect();
        let excluded: [&Path; 3] = [inbox.as_path(), course.as_path(), archive.as_path()];

        let mut report = OrganizeReport::default();

        // Step 1：源目录 → inbox（只收集扩展名匹配的文件）。
        let mut due: Vec<PathBuf> = Vec::new();
        self.collect_source(&source, &extensions, &excluded, &mut due)?;
        for src in due {
            if !self.is_due(&src, cfg)? {
                report.skipped += 1;
                continue;
            }
            match self.move_into_inbox(&src, &inbox) {
                Ok(Some(_)) => {} // 待 Step 2 处理
                Ok(None) => report.skipped += 1,
                Err(e) => {
                    report.failed += 1;
                    tracing::error!("移入 inbox 失败：{} → {e}", src.display());
                    self.notify(cfg, "整理移动失败", &e.to_string());
                }
            }
        }

        // Step 2：inbox → course_dir/{分组}，并记账。
        for src in list_files(&inbox)? {
            match self.classify_and_move(&src, cfg, &course) {
                Ok(Some((group, dst))) => {
                    report.moved += 1;
                    if let Err(e) = self.record(&dst, &group, cfg) {
                        tracing::error!("记账失败（不影响移动）：{e}");
                        self.notify(cfg, "台账写入失败", &e.to_string());
                    }
                }
                Ok(None) => report.skipped += 1,
                Err(e) => {
                    report.failed += 1;
                    tracing::error!("移动失败：{} → {e}", src.display());
                    self.notify(cfg, "整理移动失败", &e.to_string());
                }
            }
        }

        Ok(report)
    }

    /// 源目录：空 → 桌面；相对 → 基于当前工作目录。
    fn resolve_source(&self, cfg: &Config) -> Result<PathBuf> {
        if !cfg.watcher.source_dir.is_empty() {
            let p = PathBuf::from(&cfg.watcher.source_dir);
            return if p.is_absolute() {
                Ok(p)
            } else {
                Ok(std::env::current_dir()?.join(p))
            };
        }
        self.platform.desktop_dir().ok_or_else(|| {
            Error::Msg("无法探测桌面目录，请在配置中显式指定 watcher.source_dir".to_string())
        })
    }

    /// 递归收集扩展名匹配的文件，跳过我们自己的输出目录。
    fn collect_source(
        &self,
        dir: &Path,
        extensions: &HashSet<String>,
        excluded: &[&Path],
        out: &mut Vec<PathBuf>,
    ) -> Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let ft = entry.file_type()?;
            if ft.is_dir() {
                if excluded.iter().any(|ex| path.starts_with(ex)) {
                    continue;
                }
                self.collect_source(&path, extensions, excluded, out)?;
            } else if ft.is_file() && self.has_extension(&path, extensions) {
                out.push(path);
            }
        }
        Ok(())
    }

    fn has_extension(&self, path: &Path, extensions: &HashSet<String>) -> bool {
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => extensions.contains(&ext.to_lowercase()),
            None => false,
        }
    }

    /// mtime 距今是否超过 `min_age_days` 天。
    fn is_due(&self, path: &Path, cfg: &Config) -> Result<bool> {
        let mtime = self.platform.mtime(path)?;
        let mtime_dt: chrono::DateTime<chrono::Utc> = mtime.into();
        let age = self.clock.now() - mtime_dt;
        Ok(age.num_days() >= cfg.watcher.min_age_days as i64)
    }

    fn move_into_inbox(&self, src: &Path, inbox: &Path) -> Result<Option<PathBuf>> {
        std::fs::create_dir_all(inbox)?;
        let filename = file_name(src)?;
        let dst = inbox.join(&filename);
        if dst.exists() {
            tracing::warn!("inbox 已存在同名文件，跳过：{}", dst.display());
            return Ok(None);
        }
        move_with_retry(src, &dst)?;
        Ok(Some(dst))
    }

    fn classify_and_move(
        &self,
        src: &Path,
        cfg: &Config,
        course: &Path,
    ) -> Result<Option<(String, PathBuf)>> {
        let filename = file_name(src)?;
        let group = self
            .classifier
            .classify(&filename)
            .unwrap_or(&cfg.classifier.default_group)
            .to_string();
        let group_dir = course.join(&group);
        std::fs::create_dir_all(&group_dir)?;
        let dst = group_dir.join(&filename);
        if dst.exists() {
            tracing::warn!("目标分组已存在同名文件，跳过：{}", dst.display());
            return Ok(None);
        }
        move_with_retry(src, &dst)?;
        Ok(Some((group, dst)))
    }

    fn record(&self, dst: &Path, group: &str, cfg: &Config) -> Result<()> {
        let filename = dst
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let date = self.clock.now().with_timezone(&chrono::Local).date_naive();
        let entry = Record {
            filename,
            group: group.to_string(),
            date,
        };
        self.indexer.record(cfg, &entry)?;
        Ok(())
    }

    fn notify(&self, cfg: &Config, title: &str, body: &str) {
        if cfg.notify.enabled {
            let _ = self.platform.notify(title, body);
        }
    }
}

/// 列出目录下的顶层文件（inbox 非递归）。
fn list_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    out.push(entry.path());
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(out)
}

fn file_name(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .ok_or_else(|| Error::Msg(format!("路径无有效文件名：{}", path.display())))
}

/// 移动：最多 3 次，指数退避。
fn move_with_retry(src: &Path, dst: &Path) -> Result<()> {
    let mut attempt = 0u32;
    loop {
        match move_file(src, dst) {
            Ok(()) => return Ok(()),
            Err(e) => {
                attempt += 1;
                if attempt >= MAX_RETRIES {
                    return Err(e);
                }
                let backoff = StdDuration::from_millis(100u64 << (attempt - 1).min(6));
                std::thread::sleep(backoff);
            }
        }
    }
}

/// 移动：优先 rename；跨卷回退 copy+delete+校验（失败回滚）。
fn move_file(src: &Path, dst: &Path) -> Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => copy_fallback(src, dst),
        Err(e) => Err(e.into()),
    }
}

fn copy_fallback(src: &Path, dst: &Path) -> Result<()> {
    let copied = std::fs::copy(src, dst)?;
    let src_len = std::fs::metadata(src)?.len();
    if copied != src_len {
        let _ = std::fs::remove_file(dst); // 回滚
        return Err(Error::Msg(format!(
            "跨卷复制校验失败：大小 {copied} != {src_len}，已回滚"
        )));
    }
    std::fs::remove_file(src)?;
    Ok(())
}
