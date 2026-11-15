//! 配置校验的错误类型与单条问题描述。

use std::fmt;

/// 单条配置校验问题，含「JSON Pointer + 字段名 + 期望 + 实际 + 建议」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    /// JSON Pointer，如 `/watcher/min_age_days`。
    pub pointer: String,
    /// 点分字段名，如 `watcher.min_age_days`。
    pub field: String,
    /// 期望的取值 / 类型描述。
    pub expected: String,
    /// 实际得到的值。
    pub actual: String,
    /// 一条改正建议。
    pub suggestion: String,
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "配置校验失败：字段 \"{}\"（路径 {}）期望 {}，实际得到 {}\n建议：{}",
            self.field, self.pointer, self.expected, self.actual, self.suggestion
        )
    }
}

/// 配置相关错误。
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// 配置文件不是合法 JSON。
    #[error("配置文件不是合法 JSON：{0}")]
    Syntax(#[from] serde_json::Error),

    /// 通过 JSON 语法、但未通过 Schema 校验（消息为多条问题拼接）。
    #[error("{0}")]
    Validation(String),
}
