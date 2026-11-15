//! Windows 平台实现（M11）：命名互斥体单实例、注册表 Run 键自启、explorer 打开目录。
//!
//! 跨平台能力（回收站 / 通知 / 目录 / mtime）与 Linux 侧一致，直接调用跨平台 crate。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::{Error, Result};

use super::{PlatformAdapter, SingleInstanceGuard};

const MUTEX_NAME: &str = "desktop-tidier-single-instance";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "desktop-tidier";

/// 单实例锁：持有命名互斥体句柄；Drop 时 CloseHandle 释放。
pub(crate) struct LockGuard(HANDLE);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

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
        let name = wide_null(MUTEX_NAME);
        let handle = unsafe { CreateMutexW(None, false, PCWSTR::from_raw(name.as_ptr())) }
            .map_err(|e| Error::Msg(format!("创建单实例互斥体失败：{e}")))?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            let _ = unsafe { CloseHandle(handle) };
            return Err(Error::Msg("已有实例在运行".to_string()));
        }
        Ok(SingleInstanceGuard {
            inner: LockGuard(handle),
        })
    }

    fn trash(&self, paths: &[PathBuf]) -> Result<()> {
        trash::delete_all(paths.iter()).map_err(|e| Error::Msg(format!("移入回收站失败：{e}")))
    }

    fn notify(&self, title: &str, body: &str) -> Result<()> {
        notify_rust::Notification::new()
            .appname("desktop-tidier")
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
            .join("desktop-tidier")
    }

    fn mtime(&self, path: &Path) -> Result<SystemTime> {
        Ok(std::fs::metadata(path)?.modified()?)
    }

    fn set_autostart(&self, enabled: bool) -> Result<bool> {
        let run_key = wide_null(RUN_KEY);
        let value_name = wide_null(RUN_VALUE);
        let mut hkey = HKEY::default();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR::from_raw(run_key.as_ptr()),
                None,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                &mut hkey,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(Error::Msg(format!("打开注册表 Run 键失败：{status:?}")));
        }

        if enabled {
            let exe = std::env::current_exe()?;
            let data = wide_null(&exe.to_string_lossy());
            let bytes = wide_bytes(&data);
            let status = unsafe {
                RegSetValueExW(
                    hkey,
                    PCWSTR::from_raw(value_name.as_ptr()),
                    None,
                    REG_SZ,
                    Some(&bytes),
                )
            };
            let _ = unsafe { RegCloseKey(hkey) };
            if status != ERROR_SUCCESS {
                return Err(Error::Msg(format!("写入自启注册表失败：{status:?}")));
            }
            Ok(true)
        } else {
            let status = unsafe { RegDeleteValueW(hkey, PCWSTR::from_raw(value_name.as_ptr())) };
            let _ = unsafe { RegCloseKey(hkey) };
            // ERROR_FILE_NOT_FOUND(2) 表示本就未设置，视为成功。
            if status != ERROR_SUCCESS && status.0 != 2 {
                return Err(Error::Msg(format!("删除自启注册表失败：{status:?}")));
            }
            Ok(false)
        }
    }

    fn is_autostart_enabled(&self) -> Result<bool> {
        let run_key = wide_null(RUN_KEY);
        let value_name = wide_null(RUN_VALUE);
        let mut hkey = HKEY::default();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR::from_raw(run_key.as_ptr()),
                None,
                KEY_QUERY_VALUE,
                &mut hkey,
            )
        };
        if status != ERROR_SUCCESS {
            return Ok(false);
        }
        let status = unsafe {
            RegQueryValueExW(
                hkey,
                PCWSTR::from_raw(value_name.as_ptr()),
                None,
                None,
                None,
                None,
            )
        };
        let _ = unsafe { RegCloseKey(hkey) };
        Ok(status == ERROR_SUCCESS)
    }

    fn open_in_file_manager(&self, path: &Path) -> Result<()> {
        let op = wide_null("open");
        let target = wide_null(&path.to_string_lossy());
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR::from_raw(op.as_ptr()),
                PCWSTR::from_raw(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // ShellExecuteW 返回 HINSTANCE；> 32 表示成功。
        if result.0 as isize > 32 {
            Ok(())
        } else {
            Err(Error::Msg(format!(
                "打开目录失败（ShellExecuteW 返回 {result:?}）"
            )))
        }
    }
}

/// 把 UTF-8 字符串编码为带 `\0` 结尾的 UTF-16（LE）宽字符序列。
fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 宽字符序列的字节视图（UTF-16LE，供注册表 REG_SZ 使用）。
fn wide_bytes(s: &[u16]) -> Vec<u8> {
    s.iter().flat_map(|c| c.to_le_bytes()).collect()
}
