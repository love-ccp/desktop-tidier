//! 测试共享工具：标准库实现的临时目录（避免引入 tempfile 依赖）。

// 该模块被多个测试二进制分别 `mod common;` 引入，部分方法只在部分二进制中使用。
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 自动清理的临时目录。
pub struct TempDir(PathBuf);

impl TempDir {
    /// 在系统临时目录下创建一个唯一子目录。
    pub fn new() -> Self {
        let pid = std::process::id();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("course_organizer_test_{pid}_{n}"));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Default for TempDir {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
