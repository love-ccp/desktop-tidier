//! 时钟抽象：业务路径唯一的时间来源（禁止直接调 `Utc::now()`）。

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};

/// 时间来源（授时校正后的墙钟 + 单调时钟）。
pub trait Clock: Send + Sync {
    /// 授时校正后的墙钟（UTC）。
    fn now(&self) -> DateTime<Utc>;
    /// 单调时钟（定时 / 间隔用，不受授时影响）。
    fn now_mono(&self) -> tokio::time::Instant;
}

/// 无校正的系统时钟（授时关闭时的回退实现）。
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn now_mono(&self) -> tokio::time::Instant {
        tokio::time::Instant::now()
    }
}

/// 授时校正时钟：`now() = 本地墙钟 + 偏移秒`。
///
/// 偏移 = 服务器时间 - 本地时间；用 `AtomicI64` 无锁共享给所有业务。
pub struct SyncedClock {
    offset_secs: AtomicI64,
}

impl SyncedClock {
    /// 以初始偏移（秒）构造。
    pub fn new(offset_secs: i64) -> Self {
        Self {
            offset_secs: AtomicI64::new(offset_secs),
        }
    }

    /// 以初始偏移构造并包入 `Arc`（供业务层与授时服务共享）。
    pub fn shared(offset_secs: i64) -> Arc<Self> {
        Arc::new(Self::new(offset_secs))
    }

    /// 更新偏移（授时服务成功同步后调用）。
    pub fn set_offset(&self, secs: i64) {
        self.offset_secs.store(secs, Ordering::Relaxed);
    }

    /// 当前偏移（秒）。
    pub fn offset(&self) -> i64 {
        self.offset_secs.load(Ordering::Relaxed)
    }
}

impl Clock for SyncedClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now() + chrono::Duration::seconds(self.offset())
    }

    fn now_mono(&self) -> tokio::time::Instant {
        tokio::time::Instant::now()
    }
}
