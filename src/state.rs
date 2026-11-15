//! 持久化运行状态（storage/state.json）：上次整理时间、上次归档时间与台账水位。
//!
//! 用途：
//! - 班级电脑并非全天开机：启动时若距上次整理/归档已超过配置周期，则"追赶"执行一次，
//!   让 `watcher.scan_interval_secs` / `archiver.interval_hours` 变成"至多每隔 N"的宽松间隔。
//! - 归档前按台账比对新增文件数：`last_archive_record_no` 记录上次归档时台账的最大编号，
//!   本次新增 = 当前最大编号 − 上次编号，低于 `archiver.min_new_files` 则跳过本次归档。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::timesync::Clock;
use crate::Result;

/// 落盘结构（JSON；字段缺失/损坏时回退默认值，不因状态文件问题中断运行）。
/// 时间为 Unix 秒（i64），避免依赖 chrono 的 serde feature。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Persisted {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_organize_utc: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_archive_utc: Option<i64>,
    #[serde(default)]
    last_archive_record_no: u64,
}

/// 运行状态存储（互斥锁保护，写入用临时文件 + rename 原子替换）。
pub struct StateStore {
    path: PathBuf,
    inner: Mutex<Persisted>,
}

impl StateStore {
    /// 从 `path` 加载；文件缺失或解析失败时使用空状态（记 warn，不中断）。
    pub fn load(path: &Path) -> Self {
        let persisted = match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Persisted>(&bytes) {
                Ok(state) => state,
                Err(e) => {
                    tracing::warn!("状态文件解析失败（回退空状态）：{e}");
                    Persisted::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Persisted::default(),
            Err(e) => {
                tracing::warn!("状态文件读取失败（回退空状态）：{e}");
                Persisted::default()
            }
        };
        Self {
            path: path.to_path_buf(),
            inner: Mutex::new(persisted),
        }
    }

    /// 互斥锁（中毒时取回内部值继续用，状态文件损坏不应拖垮程序）。
    fn lock(&self) -> MutexGuard<'_, Persisted> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn persist(&self, state: &Persisted) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(state)?;
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.tmp_path();
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    fn tmp_path(&self) -> PathBuf {
        let mut os = self.path.as_os_str().to_os_string();
        os.push(format!(".tmp{}", std::process::id()));
        PathBuf::from(os)
    }

    /// 距上次整理是否已超过 `interval_secs`（从未整理过 → true）。
    pub fn organize_due(&self, interval_secs: u64, clock: &dyn Clock) -> bool {
        let state = self.lock();
        match state.last_organize_utc {
            None => true,
            Some(t) => clock.now().timestamp() - t >= interval_secs as i64,
        }
    }

    /// 记录一次整理完成时间。
    pub fn mark_organize(&self, clock: &dyn Clock) -> Result<()> {
        let mut state = self.lock();
        state.last_organize_utc = Some(clock.now().timestamp());
        self.persist(&state)
    }

    /// 距上次归档（含被跳过的归档）是否已超过 `interval_hours` 小时。
    pub fn archive_due(&self, interval_hours: u32, clock: &dyn Clock) -> bool {
        let state = self.lock();
        match state.last_archive_utc {
            None => true,
            Some(t) => (clock.now().timestamp() - t) / 3600 >= interval_hours as i64,
        }
    }

    /// 上次归档时的台账最大编号（首次为 0）。
    pub fn last_archive_record_no(&self) -> u64 {
        self.lock().last_archive_record_no
    }

    /// 归档成功：推进台账水位与归档时间。
    pub fn mark_archive(&self, max_record_no: u64, clock: &dyn Clock) -> Result<()> {
        let mut state = self.lock();
        state.last_archive_utc = Some(clock.now().timestamp());
        state.last_archive_record_no = max_record_no;
        self.persist(&state)
    }

    /// 归档被跳过（新增不足阈值）：只推进归档时间，水位不变（下次继续累计）。
    pub fn mark_archive_skipped(&self, clock: &dyn Clock) -> Result<()> {
        let mut state = self.lock();
        state.last_archive_utc = Some(clock.now().timestamp());
        self.persist(&state)
    }
}
