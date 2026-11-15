//! 库级错误类型与 `Result` 别名。
//!
//! 约定：业务路径统一返回 [`Result`]；禁止在业务路径使用
//! `unwrap`/`expect`/`panic!`。

use thiserror::Error;

/// 库级统一错误。
#[derive(Debug, Error)]
pub enum Error {
    /// 底层 IO 错误。
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),

    /// JSON 解析 / 序列化错误。
    #[error("JSON 错误：{0}")]
    Json(#[from] serde_json::Error),

    /// 配置加载 / 校验错误（直接透出 ConfigError 的展示文本）。
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),

    /// 其它运行时错误。
    #[error("{0}")]
    Msg(String),
}

/// 库级 `Result` 别名。
pub type Result<T> = std::result::Result<T, Error>;
