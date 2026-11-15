//! 配置模块：结构定义、加载、Schema 校验、示例生成、热重载存储。
//!
//! 配置文件为严格 JSON（不支持注释），先经 JSON Schema（draft-07）校验，
//! 再反序列化。校验失败返回「字段 JSON Pointer + 字段名 + 期望 + 实际 + 建议」。

mod error;
mod schema;

pub use error::{ConfigError, ValidationIssue};
pub use schema::example_str;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;

/// 顶层配置（加载后不可变；相对路径已在加载时解析为绝对路径）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub watcher: Watcher,
    pub storage: Storage,
    pub classifier: Classifier,
    pub archiver: Archiver,
    pub indexer: Indexer,
    pub logger: Logger,
    pub timesync: Timesync,
    pub notify: Notify,
    pub concurrency: Concurrency,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Watcher {
    /// 源目录（空字符串 = 自动探测桌面）。
    pub source_dir: String,
    /// 需要处理的后缀（不带点、小写）。
    pub extensions: Vec<String>,
    /// 修改时间超过 N 天（mtime）。
    pub min_age_days: u32,
    /// 文件事件防抖（毫秒）。
    pub debounce_ms: u64,
    /// 扫描整理频率（秒），默认 172800（两天一次）。
    pub scan_interval_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    /// 步骤 1 移动目标。
    pub inbox_dir: String,
    /// 步骤 2 分组根，文件进 `course_dir/{group}/`。
    pub course_dir: String,
    /// 步骤 3 归档目标。
    pub archive_dir: String,
    /// 归档成功后清空 course_dir 至回收站。
    pub recycle_after_archive: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Classifier {
    pub lexicon_path: String,
    pub lexicon_encoding: String,
    pub default_group: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archiver {
    pub interval_hours: u32,
    pub zip_level: u32,
    pub rename_pattern: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Indexer {
    pub md_path: String,
    pub md_encoding: String,
    pub entry_format: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logger {
    pub file: String,
    pub level: String,
    pub max_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timesync {
    pub enabled: bool,
    pub servers: Vec<String>,
    pub interval_hours: u32,
    pub timeout_secs: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notify {
    pub enabled: bool,
    pub app_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Concurrency {
    pub max_parallel_ops: u32,
}

impl Config {
    /// 加载并校验配置；文件不存在时先生成示例。
    ///
    /// 返回的配置中相对路径已解析为基于配置文件所在目录的绝对路径。
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            Self::write_example(path)?;
        }
        let text = std::fs::read_to_string(path)?;
        let mut value: Value = serde_json::from_str(&text).map_err(ConfigError::Syntax)?;
        // `$schema` 是编辑器提示键，不属于配置字段（schema 设置了 additionalProperties:false）。
        if let Some(obj) = value.as_object_mut() {
            obj.remove("$schema");
        }
        if let Err(message) = schema::validate(&value) {
            return Err(ConfigError::Validation(message).into());
        }
        let mut config: Config = serde_json::from_value(value)?;
        if let Some(base) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            config.resolve_paths(base);
        }
        Ok(config)
    }

    /// 写出内置示例配置（内容与 `config/config.example.json` 一致）。
    pub fn write_example(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, schema::example_str())?;
        Ok(())
    }

    /// 把 storage.* / logger.file / indexer.md_path / classifier.lexicon_path 等
    /// 相对路径拼接到 `base`（配置文件所在目录）。已为绝对路径的保持不变。
    pub fn resolve_paths(&mut self, base: &Path) {
        for slot in [
            &mut self.storage.inbox_dir,
            &mut self.storage.course_dir,
            &mut self.storage.archive_dir,
            &mut self.logger.file,
            &mut self.indexer.md_path,
            &mut self.classifier.lexicon_path,
        ] {
            if slot.is_empty() {
                continue;
            }
            let p = Path::new(slot.as_str());
            if p.is_relative() {
                *slot = base.join(p).to_string_lossy().into_owned();
            }
        }
    }
}

/// 热重载存储：`RwLock<Arc<Config>>`（等价于规格中的 ArcSwap 语义，
/// 用标准库实现以避免额外依赖）。
pub struct ConfigStore {
    path: PathBuf,
    inner: RwLock<Arc<Config>>,
}

impl ConfigStore {
    /// 从 `path` 加载初始配置。
    pub fn load(path: &Path) -> Result<Self> {
        let config = Config::load(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            inner: RwLock::new(Arc::new(config)),
        })
    }

    /// 当前配置快照。
    pub fn get(&self) -> Arc<Config> {
        read_lock(&self.inner).clone()
    }

    /// 重新加载；成功返回 [`ReloadOutcome::Reloaded`] 并原子替换，
    /// 失败返回 [`ReloadOutcome::Rejected`] 且保留旧值。
    pub fn reload(&self) -> Result<ReloadOutcome> {
        match Config::load(&self.path) {
            Ok(new_config) => {
                let arc = Arc::new(new_config);
                *write_lock(&self.inner) = Arc::clone(&arc);
                Ok(ReloadOutcome::Reloaded(arc))
            }
            Err(e) => Ok(ReloadOutcome::Rejected(e.to_string())),
        }
    }
}

/// 热重载结果。
#[derive(Debug)]
pub enum ReloadOutcome {
    Reloaded(Arc<Config>),
    Rejected(String),
}

/// 读取锁（中毒时恢复，避免 panic）。
fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 写锁（中毒时恢复，避免 panic）。
fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
