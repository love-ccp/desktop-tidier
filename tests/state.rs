//! 运行状态测试：宽松间隔判定、台账水位推进、损坏文件回退。

mod common;

use std::sync::{Arc, Mutex};

use desktop_tidier::state::StateStore;
use desktop_tidier::timesync::Clock;

/// 可调时钟：`now()` 由外部设置（模拟时间流逝）。
#[derive(Clone)]
struct AdjustableClock(Arc<Mutex<chrono::DateTime<chrono::Utc>>>);

impl AdjustableClock {
    fn new(t: chrono::DateTime<chrono::Utc>) -> Self {
        Self(Arc::new(Mutex::new(t)))
    }

    fn set(&self, t: chrono::DateTime<chrono::Utc>) {
        *self.0.lock().unwrap() = t;
    }
}

impl Clock for AdjustableClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        *self.0.lock().unwrap()
    }

    fn now_mono(&self) -> tokio::time::Instant {
        tokio::time::Instant::now()
    }
}

#[test]
fn organize_due_and_mark_organize() {
    let dir = common::TempDir::new();
    let path = dir.join("storage/state.json");
    let store = StateStore::load(&path);
    let clock = AdjustableClock::new(chrono::Utc::now());

    // 首次：无记录 → 判定超期（启动追赶会补一次整理）。
    assert!(store.organize_due(172_800, &clock));
    store.mark_organize(&clock).unwrap();
    // 刚整理完：不超期。
    assert!(!store.organize_due(172_800, &clock));

    // 落盘后可恢复（同一时刻：不超期）。
    let reloaded = StateStore::load(&path);
    assert!(!reloaded.organize_due(172_800, &clock));

    // 时间前进 3 天：超期（宽松间隔 =「至多每隔 N」，超过即追）。
    let later = clock.now() + chrono::Duration::days(3);
    clock.set(later);
    assert!(store.organize_due(172_800, &clock));
    assert!(reloaded.organize_due(172_800, &clock));
}

#[test]
fn archive_due_and_watermark() {
    let dir = common::TempDir::new();
    let path = dir.join("storage/state.json");
    let store = StateStore::load(&path);
    let clock = AdjustableClock::new(chrono::Utc::now());

    assert!(store.archive_due(168, &clock));
    assert_eq!(store.last_archive_record_no(), 0);

    // 跳过归档：只推进归档时间，台账水位不变（下次继续累计新增）。
    store.mark_archive_skipped(&clock).unwrap();
    assert!(!store.archive_due(168, &clock));
    assert_eq!(store.last_archive_record_no(), 0, "跳过不应推进水位");

    // 成功归档：推进水位并落盘。
    store.mark_archive(42, &clock).unwrap();
    assert_eq!(store.last_archive_record_no(), 42);
    let reloaded = StateStore::load(&path);
    assert_eq!(reloaded.last_archive_record_no(), 42);
}

#[test]
fn corrupt_state_falls_back_to_empty() {
    let dir = common::TempDir::new();
    let path = dir.join("storage/state.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{ not valid json").unwrap();

    let store = StateStore::load(&path);
    let clock = AdjustableClock::new(chrono::Utc::now());
    // 损坏文件回退为空状态：不中断运行，按首次运行处理。
    assert!(store.organize_due(1, &clock));
    assert!(store.archive_due(1, &clock));
    assert_eq!(store.last_archive_record_no(), 0);
}
