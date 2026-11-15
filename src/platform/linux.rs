//! Linux 平台实现（M3）：单实例 flock、回收站、通知、桌面/配置目录、
//! mtime、XDG autostart、xdg-open。

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::{Error, Result};

use super::{PlatformAdapter, SingleInstanceGuard};

/// 单实例锁：持有 `instance.lock` 文件 fd；Drop 关闭 fd 时内核自动释放 flock。
pub(crate) struct LockGuard(File);

impl Drop for LockGuard {
    fn drop(&mut self) {
        // 显式解锁（即便不调用，fd 关闭也会释放 flock；此处为语义清晰）。
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// Linux 平台适配器（无状态）。
#[derive(Clone, Copy, Default)]
pub struct LinuxPlatform;

impl LinuxPlatform {
    pub fn new() -> Self {
        Self
    }

    /// XDG autostart 的 `.desktop` 文件路径。
    fn autostart_path(&self) -> PathBuf {
        let base = config_home();
        base.join("autostart").join("course-organizer.desktop")
    }
}

impl PlatformAdapter for LinuxPlatform {
    fn acquire_single_instance(&self) -> Result<SingleInstanceGuard> {
        let dir = self.default_config_dir();
        std::fs::create_dir_all(&dir)?;
        let lock_path = dir.join("instance.lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;
        // 非阻塞排他锁：已被占用说明已有实例在运行。
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            return Err(Error::Msg(format!("已有实例在运行（{err}）")));
        }
        Ok(SingleInstanceGuard {
            inner: LockGuard(file),
        })
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
        config_home().join("course-organizer")
    }

    fn mtime(&self, path: &Path) -> Result<SystemTime> {
        Ok(std::fs::metadata(path)?.modified()?)
    }

    fn set_autostart(&self, enabled: bool) -> Result<bool> {
        let path = self.autostart_path();
        if enabled {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let exe = std::env::current_exe()
                .map_err(|e| Error::Msg(format!("无法确定可执行文件路径：{e}")))?;
            let content = format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=课件整理助手\n\
                 Exec={}\n\
                 X-GNOME-Autostart-enabled=true\n",
                exe.to_string_lossy()
            );
            std::fs::write(&path, content)?;
            Ok(true)
        } else {
            match std::fs::remove_file(&path) {
                Ok(()) => Ok(false),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e.into()),
            }
        }
    }

    fn is_autostart_enabled(&self) -> Result<bool> {
        Ok(self.autostart_path().exists())
    }

    fn open_in_file_manager(&self, path: &Path) -> Result<()> {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| Error::Msg(format!("无法打开目录：{e}")))?;
        Ok(())
    }
}

/// `$XDG_CONFIG_HOME`，回退 `~/.config`。
fn config_home() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string())).join(".config")
    })
}
