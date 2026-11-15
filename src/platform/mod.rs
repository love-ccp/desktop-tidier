//! 平台抽象层：统一 Windows / Linux 差异。
//!
//! 业务层禁止出现 `#[cfg(windows)]` / `#[cfg(target_os)]` 分支，一律走
//! [`PlatformAdapter`]。所有方法为阻塞方法，业务层用 `spawn_blocking` 包裹。

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

use std::path::{Path, PathBuf};

use crate::Result;

/// 平台能力抽象（`Send + Sync`，可在 tokio worker 中共享）。
pub trait PlatformAdapter: Send + Sync {
    /// 获取单实例锁；已存在实例则返回错误。Drop 返回的 guard 即释放锁。
    fn acquire_single_instance(&self) -> Result<SingleInstanceGuard>;
    /// 将文件移入回收站。
    fn trash(&self, paths: &[PathBuf]) -> Result<()>;
    /// 发送系统通知（是否静默由业务层按 `notify.enabled` 决定）。
    fn notify(&self, title: &str, body: &str) -> Result<()>;
    /// 桌面目录（`None` 表示无法探测）。
    fn desktop_dir(&self) -> Option<PathBuf>;
    /// 默认配置目录（业务层配置不存在时在此生成）。
    fn default_config_dir(&self) -> PathBuf;
    /// 文件最后修改时间（mtime）。
    fn mtime(&self, path: &Path) -> Result<std::time::SystemTime>;
    /// 设置开机自启；返回设置后的启用状态。
    fn set_autostart(&self, enabled: bool) -> Result<bool>;
    /// 当前是否已启用开机自启（托盘菜单勾选态）。
    fn is_autostart_enabled(&self) -> Result<bool>;
    /// 在系统文件管理器中打开 `path`（explorer / xdg-open）。
    fn open_in_file_manager(&self, path: &Path) -> Result<()>;
}

/// 单实例锁句柄；Drop 时释放（关闭锁 fd / 释放命名互斥体）。
pub struct SingleInstanceGuard {
    /// 平台锁句柄；纯 RAII 持有，仅靠 Drop 释放，字段本身不直接读取。
    #[cfg(target_os = "linux")]
    #[allow(dead_code)]
    inner: linux::LockGuard,
    #[cfg(target_os = "windows")]
    #[allow(dead_code)]
    inner: windows::LockGuard,
}

/// 返回当前平台的适配器实现。
pub fn adapter() -> Box<dyn PlatformAdapter> {
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxPlatform::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsPlatform::new())
    }
}
