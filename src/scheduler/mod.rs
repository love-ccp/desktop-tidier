//! 后台编排器（M8）：周期整理 / 归档 / 授时 + 配置、词库热重载。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use notify_debouncer_full::notify::RecursiveMode;
use tokio::sync::watch;

use crate::archiver::Archiver;
use crate::classifier::Classifier;
use crate::config::{Config, ConfigStore, ReloadOutcome};
use crate::indexer::Indexer;
use crate::organizer::Organizer;
use crate::platform::PlatformAdapter;
use crate::timesync::{Clock, Timesync};
use crate::Result;

/// 后台编排器：持有可热重载的配置与分类器，编排各周期任务。
pub struct Scheduler {
    config: Arc<ConfigStore>,
    classifier: ArcSwap<Classifier>,
    platform: Arc<dyn PlatformAdapter>,
    clock: Arc<dyn Clock>,
    indexer: Arc<Indexer>,
    timesync: Arc<Timesync>,
}

impl Scheduler {
    pub fn new(
        config: Arc<ConfigStore>,
        platform: Arc<dyn PlatformAdapter>,
        clock: Arc<dyn Clock>,
        timesync: Arc<Timesync>,
    ) -> Result<Self> {
        let classifier = build_classifier(config.get())?;
        Ok(Self {
            config,
            classifier: ArcSwap::from_pointee(classifier),
            platform,
            clock,
            indexer: Arc::new(Indexer),
            timesync,
        })
    }

    /// 一次完整整理：organizer（扫描→分类→移动→记账）+ archiver（归档）。
    pub async fn organize_once(&self) -> Result<()> {
        let cfg = self.config.get();
        let organizer = Organizer::new(
            self.platform.clone(),
            self.classifier.load_full(),
            self.indexer.clone(),
            self.clock.clone(),
        );
        let report = organizer.run_once(&cfg).await?;
        tracing::info!(
            "整理完成：moved={} skipped={} failed={}",
            report.moved,
            report.skipped,
            report.failed
        );

        let archiver = Archiver::new(self.platform.clone(), self.clock.clone());
        let ar = archiver.run_once(&cfg).await?;
        tracing::info!(
            "归档完成：archived={} recycled={}",
            ar.archived,
            ar.recycled
        );
        Ok(())
    }

    /// 归档一次（独立周期，供 `archiver.interval_hours` 调用）。
    pub async fn archive_once(&self) -> Result<()> {
        let cfg = self.config.get();
        let archiver = Archiver::new(self.platform.clone(), self.clock.clone());
        let ar = archiver.run_once(&cfg).await?;
        tracing::info!(
            "归档完成：archived={} recycled={}",
            ar.archived,
            ar.recycled
        );
        Ok(())
    }

    /// 授时一次（失败不阻塞，沿用本地时钟）。
    pub async fn timesync_once(&self) {
        let cfg = self.config.get();
        if !cfg.timesync.enabled {
            return;
        }
        match self
            .timesync
            .sync_once(&cfg.timesync.servers, cfg.timesync.timeout_secs)
            .await
        {
            Ok(secs) => tracing::info!("授时成功：偏移 {secs}s"),
            Err(e) => tracing::warn!("授时失败（沿用本地时钟）：{e}"),
        }
    }

    /// 重新加载配置；成功则重建分类器（词库路径可能变化）。
    pub fn reload_config(&self) -> Result<ReloadOutcome> {
        let outcome = self.config.reload()?;
        if let ReloadOutcome::Reloaded(cfg) = &outcome {
            self.classifier
                .store(Arc::new(build_classifier(Arc::clone(cfg))?));
        }
        Ok(outcome)
    }

    /// 仅重建分类器（词库内容变化）。
    pub fn reload_lexicon(&self) -> Result<()> {
        let classifier = build_classifier(self.config.get())?;
        self.classifier.store(Arc::new(classifier));
        Ok(())
    }

    /// 启动所有周期任务与文件监听；`shutdown` 置 true 时优雅退出。
    pub async fn run_loops(self: Arc<Self>, mut shutdown: watch::Receiver<bool>) {
        let cfg0 = self.config.get();
        let scan = Duration::from_secs(cfg0.watcher.scan_interval_secs);
        let archive_iv = Duration::from_secs(cfg0.archiver.interval_hours as u64 * 3600);
        let timesync_iv = Duration::from_secs(cfg0.timesync.interval_hours as u64 * 3600);
        drop(cfg0);

        // 授时：启动立即一次，之后每 N 小时。
        {
            let this = Arc::clone(&self);
            let mut sh = shutdown.clone();
            tokio::spawn(async move {
                this.timesync_once().await;
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(timesync_iv) => this.timesync_once().await,
                        _ = sh.changed() => break,
                    }
                }
            });
        }

        // 周期全量整理（兜底）。
        {
            let this = Arc::clone(&self);
            let mut sh = shutdown.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(scan) => {
                            if let Err(e) = this.organize_once().await {
                                tracing::error!("周期整理失败：{e}");
                            }
                        }
                        _ = sh.changed() => break,
                    }
                }
            });
        }

        // 周期归档。
        {
            let this = Arc::clone(&self);
            let mut sh = shutdown.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(archive_iv) => {
                            if let Err(e) = this.archive_once().await {
                                tracing::error!("周期归档失败：{e}");
                            }
                        }
                        _ = sh.changed() => break,
                    }
                }
            });
        }

        // 文件监听：配置/词库 → 热重载；源目录 → 触发整理。
        let (watcher_tx, mut watcher_rx) = tokio::sync::mpsc::channel::<Vec<PathBuf>>(64);
        let watch_paths = self.watch_paths();
        let debounce = Duration::from_millis(self.config.get().watcher.debounce_ms);
        if let Err(e) = crate::watcher::spawn_debounced_watcher(watch_paths, debounce, watcher_tx) {
            tracing::warn!("文件监听不可用（热重载/自动整理将依赖周期兜底）：{e}");
        }

        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                maybe = watcher_rx.recv() => {
                    let Some(paths) = maybe else { break };
                    self.handle_fs_events(&paths).await;
                }
            }
        }
    }

    /// 计算需要监听的路径：配置文件、词库、源目录。
    fn watch_paths(&self) -> Vec<(PathBuf, RecursiveMode)> {
        let cfg = self.config.get();
        let mut out = vec![
            (
                self.config.path().to_path_buf(),
                RecursiveMode::NonRecursive,
            ),
            (
                PathBuf::from(&cfg.classifier.lexicon_path),
                RecursiveMode::NonRecursive,
            ),
        ];
        let source = if cfg.watcher.source_dir.is_empty() {
            self.platform.desktop_dir()
        } else {
            Some(PathBuf::from(&cfg.watcher.source_dir))
        };
        if let Some(src) = source {
            out.push((src, RecursiveMode::Recursive));
        }
        out
    }

    /// 按变化路径决定动作：配置/词库 → 重载；源目录 → 整理。
    async fn handle_fs_events(&self, paths: &[PathBuf]) {
        let cfg = self.config.get();
        let config_path = self.config.path().to_path_buf();
        let lexicon_path = PathBuf::from(&cfg.classifier.lexicon_path);
        let source = if cfg.watcher.source_dir.is_empty() {
            self.platform.desktop_dir()
        } else {
            Some(PathBuf::from(&cfg.watcher.source_dir))
        };

        let mut reload = false;
        let mut organize = false;
        for p in paths {
            if p == &config_path || p == &lexicon_path {
                reload = true;
            }
            if let Some(src) = &source {
                if p.starts_with(src) {
                    organize = true;
                }
            }
        }

        if reload {
            match self.reload_config() {
                Ok(ReloadOutcome::Reloaded(_)) => tracing::info!("配置已热重载"),
                Ok(ReloadOutcome::Rejected(msg)) => {
                    tracing::error!("配置重载被拒绝（保留旧值）：{msg}");
                }
                Err(e) => tracing::error!("配置重载失败：{e}"),
            }
        }
        if organize {
            if let Err(e) = self.organize_once().await {
                tracing::error!("整理失败：{e}");
            }
        }
    }
}

fn build_classifier(cfg: Arc<Config>) -> Result<Classifier> {
    Classifier::load(
        std::path::Path::new(&cfg.classifier.lexicon_path),
        &cfg.classifier.lexicon_encoding,
    )
}
