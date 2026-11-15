//! 测试共享工具：基于 `tempfile` 的自动清理临时目录。

// 该模块被多个测试二进制分别 `mod common;` 引入，部分方法只在部分二进制中使用。
#![allow(dead_code)]

use std::path::{Path, PathBuf};

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
