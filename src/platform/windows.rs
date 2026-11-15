//! Windows 平台实现：M11 完整实现；此处为可编译的最小桩。
//!
//! 跨平台能力（回收站 / 通知 / 目录 / mtime）已直接实现，仅 Windows 特有
//! 能力（命名互斥体单实例、注册表自启、explorer 打开目录）待 M11 补齐。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::{Error, Result};

use super::{PlatformAdapter, SingleInstanceGuard};

/// 单实例锁句柄占位（M11 用 Win32 命名互斥体实现）。
#[allow(dead_code)]
pub(crate) struct LockGuard;

/// Windows 平台适配器（无状态）。
#[derive(Clone, Copy, Default)]
pub struct WindowsPlatform;

impl WindowsPlatform {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformAdapter for WindowsPlatform {
    fn acquire_single_instance(&self) -> Result<SingleInstanceGuard> {
        // M11：CreateMutexW 命名互斥体，已存在则拒绝第二个实例。
        Err(Error::Msg("Windows 单实例锁待 M11 实现".to_string()))
    }

    fn trash(&self, paths: &[PathBuf]) -> Result<()> {
        trash::delete_all(paths.iter()).map_err(|e| Error::Msg(format!("移入回收站失败：{e}")))
    }

    fn notify(&self, title: &str, body: &str) -> Result<()> {
        notify_rust::Notification::new()
            .appname("course-organizer")
            .summary(title)
            .body(body)
            .show()
            .map_err(|e| Error::Msg(format!("发送系统通知失败：{e}")))?;
        Ok(())
    }

    fn desktop_dir(&self) -> Option<PathBuf> {
        dirs::desktop_dir()
    }

    fn default_config_dir(&self) -> PathBuf {
        dirs::config_dir()
            .unwrap_or_default()
            .join("course-organizer")
    }

    fn mtime(&self, path: &Path) -> Result<SystemTime> {
        Ok(std::fs::metadata(path)?.modified()?)
    }

    fn set_autostart(&self, _enabled: bool) -> Result<bool> {
        // M11：写注册表 HKCU\...\CurrentVersion\Run 键。
        Err(Error::Msg("Windows 开机自启待 M11 实现".to_string()))
    }

    fn is_autostart_enabled(&self) -> Result<bool> {
        // M11：读注册表 Run 键。
        Err(Error::Msg("Windows 开机自启状态待 M11 实现".to_string()))
    }

    fn open_in_file_manager(&self, _path: &Path) -> Result<()> {
        // M11：explorer /select,<path>。
        Err(Error::Msg("Windows 打开目录待 M11 实现".to_string()))
    }
}
