//! 授时（M4）：HTTPS 读响应 `Date` 头计算偏移，落盘持久化，提供校正时钟。
//!
//! 偏移 = 服务器时间 - 本地时间（秒）。启动时从落盘文件恢复，之后由
//! [`Timesync::sync_once`] 按 `timesync.interval_hours` 周期更新。

mod clock;

pub use clock::{Clock, SyncedClock, SystemClock};

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Result;

/// 落盘的偏移记录。
#[derive(Debug, Serialize, Deserialize)]
struct OffsetFile {
    /// 偏移秒（服务器 - 本地）。
    offset_secs: i64,
    /// 上次成功同步时间（RFC 3339）。
    updated_at: String,
}

/// 授时服务：用 `reqwest`（rustls）读响应 `Date` 头，更新 [`SyncedClock`] 并落盘。
pub struct Timesync {
    client: reqwest::Client,
    clock: Arc<SyncedClock>,
    offset_path: PathBuf,
}

impl Timesync {
    /// 新建授时服务；`offset_path` 为偏移持久化文件。
    pub fn new(clock: Arc<SyncedClock>, offset_path: PathBuf) -> Result<Self> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| crate::Error::Msg(format!("构建 HTTP 客户端失败：{e}")))?;
        Ok(Self {
            client,
            clock,
            offset_path,
        })
    }

    /// 读取落盘偏移；文件不存在或损坏时返回 0（不回退失败）。
    pub fn load_offset(path: &Path) -> i64 {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str::<OffsetFile>(&text)
                .map(|o| o.offset_secs)
                .unwrap_or(0),
            Err(_) => 0,
        }
    }

    /// 依次尝试服务器，读首个成功的 `Date` 头；成功则更新时钟并落盘。
    ///
    /// 返回最终偏移秒（可为负，表示本地时钟偏快）。
    pub async fn sync_once(&self, servers: &[String], timeout_secs: u32) -> Result<i64> {
        let mut last_err: Option<String> = None;
        for server in servers {
            match self.try_server(server, timeout_secs).await {
                Ok(offset) => {
                    let secs = offset.num_seconds();
                    self.clock.set_offset(secs);
                    self.persist(offset)?;
                    return Ok(secs);
                }
                Err(e) => last_err = Some(format!("{server}: {e}")),
            }
        }
        Err(crate::Error::Msg(format!(
            "授时失败：{}",
            last_err.unwrap_or_else(|| "无可用服务器".to_string())
        )))
    }

    /// 请求单个服务器并解析 `Date` 头。
    async fn try_server(&self, url: &str, timeout_secs: u32) -> Result<chrono::Duration> {
        let resp = self
            .client
            .get(url)
            .timeout(Duration::from_secs(timeout_secs as u64))
            .send()
            .await
            .map_err(|e| crate::Error::Msg(format!("请求 {url} 失败：{e}")))?;
        let header = resp
            .headers()
            .get(reqwest::header::DATE)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| crate::Error::Msg(format!("{url} 响应缺少 Date 头")))?;
        let server_time = parse_http_date(header)?;
        let local = Utc::now();
        Ok(server_time.signed_duration_since(local))
    }

    /// 将偏移落盘（原子性由「写临时文件 + rename」保障不在此处处理；
    /// 偏移为小文件且损坏可回退为 0，故直接写）。
    fn persist(&self, offset: chrono::Duration) -> Result<()> {
        let record = OffsetFile {
            offset_secs: offset.num_seconds(),
            updated_at: Utc::now().to_rfc3339(),
        };
        if let Some(parent) = self.offset_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.offset_path, serde_json::to_string(&record)?)?;
        Ok(())
    }
}

/// 解析 HTTP `Date` 头（RFC 7231 IMF-fixdate，如 `Sun, 06 Nov 1994 08:49:37 GMT`）。
pub fn parse_http_date(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc2822(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| crate::Error::Msg(format!("无法解析 Date 头 {s:?}：{e}")))
}
