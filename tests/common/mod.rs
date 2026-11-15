//! 测试共享工具：临时目录、固定时钟、假平台、示例配置。

// 该模块被多个测试二进制分别 `mod common;` 引入，部分方法只在部分二进制中使用。
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use desktop_tidier::platform::{PlatformAdapter, SingleInstanceGuard};
use desktop_tidier::timesync::Clock;
use desktop_tidier::{Error, Result};

/// 自动清理的临时目录（封装 `tempfile::TempDir`）。
pub struct TempDir(tempfile::TempDir);

impl TempDir {
    /// 在系统临时目录下创建唯一子目录，Drop 时自动删除。
    pub fn new() -> Self {
        Self(tempfile::tempdir().expect("create temp dir"))
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.path().join(name)
    }
}

impl Default for TempDir {
    fn default() -> Self {
        Self::new()
    }
}

/// 固定时钟（测试用）：始终返回构造时的时间。
pub struct FixedClock(pub chrono::DateTime<chrono::Utc>);

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.0
    }

    fn now_mono(&self) -> tokio::time::Instant {
        tokio::time::Instant::now()
    }
}

/// 假平台（测试用）：回收站直接删文件、通知记录到内存、桌面为指定目录。
pub struct FakePlatform {
    pub desktop: PathBuf,
    pub notified: Mutex<Vec<String>>,
    autostart: std::sync::atomic::AtomicBool,
}

impl FakePlatform {
    pub fn new(desktop: PathBuf) -> Self {
        Self {
            desktop,
            notified: Mutex::new(Vec::new()),
            autostart: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl PlatformAdapter for FakePlatform {
    fn acquire_single_instance(&self) -> Result<SingleInstanceGuard> {
        Err(Error::Msg("测试平台不支持单实例锁".to_string()))
    }

    fn trash(&self, paths: &[PathBuf]) -> Result<()> {
        for p in paths {
            let _ = std::fs::remove_file(p);
        }
        Ok(())
    }

    fn notify(&self, title: &str, body: &str) -> Result<()> {
        self.notified
            .lock()
            .expect("notify lock")
            .push(format!("{title}: {body}"));
        Ok(())
    }

    fn desktop_dir(&self) -> Option<PathBuf> {
        Some(self.desktop.clone())
    }

    fn default_config_dir(&self) -> PathBuf {
        self.desktop.join("config")
    }

    fn created(&self, path: &Path) -> Result<SystemTime> {
        let meta = std::fs::metadata(path)?;
        match meta.created() {
            Ok(t) => Ok(t),
            Err(_) => meta.modified().map_err(Into::into),
        }
    }

    fn set_autostart(&self, enabled: bool) -> Result<bool> {
        self.autostart
            .store(enabled, std::sync::atomic::Ordering::SeqCst);
        Ok(enabled)
    }

    fn is_autostart_enabled(&self) -> Result<bool> {
        Ok(self.autostart.load(std::sync::atomic::Ordering::SeqCst))
    }

    fn open_in_file_manager(&self, _path: &Path) -> Result<()> {
        Ok(())
    }
}

/// 在 `dir` 下生成示例配置并加载（路径已相对 `dir` 解析）。
pub fn example_config(dir: &Path) -> desktop_tidier::config::Config {
    let path = dir.join("config.json");
    desktop_tidier::config::Config::write_example(&path).expect("write example");
    desktop_tidier::config::Config::load(&path).expect("load example")
}

/// 拨快 N 天的固定时钟。文件「创建时间」无法跨平台回写，
/// 测试用「文件刚创建 + 时钟超前 N 天」来模拟「N 天前创建」。
pub fn clock_days_from_now(days: i64) -> FixedClock {
    FixedClock(chrono::Utc::now() + chrono::Duration::days(days))
}
