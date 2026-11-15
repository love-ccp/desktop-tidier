//! course-organizer 库入口。
//!
//! 单进程纯 Rust 桌面课件整理后台工具。业务层统一返回 [`Result`]，
//! 禁止使用 `unwrap`/`expect`/`panic`。

pub mod config;
pub mod error;
pub mod logger;
pub mod platform;
pub mod timesync;

pub use error::{Error, Result};

/// 版本号（来自 Cargo.toml）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 程序入口（薄组装层；M0~M2 阶段为空实现）。
pub fn run() -> anyhow::Result<()> {
    println!("course-organizer v{VERSION}");
    Ok(())
}
