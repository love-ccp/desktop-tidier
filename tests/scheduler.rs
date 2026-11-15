//! M8 编排器测试：完整流水线（整理+归档+记账）端到端。

mod common;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use course_organizer::config::{Config, ConfigStore};
use course_organizer::platform::PlatformAdapter;
use course_organizer::scheduler::Scheduler;
use course_organizer::timesync::{Clock, SyncedClock, Timesync};

fn setup(
    dir: &common::TempDir,
    lexicon: &str,
) -> (Arc<ConfigStore>, Arc<dyn Clock>, std::path::PathBuf) {
    let cfg_path = dir.join("config.json");
    Config::write_example(&cfg_path).unwrap();
    let config_dir = dir.join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("lexicon.txt"), lexicon).unwrap();

    let desktop = dir.join("Desktop");
    std::fs::create_dir_all(&desktop).unwrap();

    let config_store = Arc::new(ConfigStore::load(&cfg_path).unwrap());
    let clock: Arc<dyn Clock> = Arc::new(common::FixedClock(chrono::Utc::now()));
    (config_store, clock, desktop)
}

fn build_scheduler(
    config_store: Arc<ConfigStore>,
    clock: Arc<dyn Clock>,
    desktop: std::path::PathBuf,
    dir: &common::TempDir,
) -> Scheduler {
    let platform: Arc<dyn PlatformAdapter> = Arc::new(common::FakePlatform::new(desktop));
    // 授时服务需要具体 SyncedClock（更新偏移），与测试用的 FixedClock 分离。
    let timesync =
        Arc::new(Timesync::new(SyncedClock::shared(0), dir.join("offset.json")).unwrap());
    Scheduler::new(config_store, platform, clock, timesync).unwrap()
}

#[tokio::test]
async fn organize_once_runs_full_pipeline() {
    let dir = common::TempDir::new();
    let (config_store, clock, desktop) = setup(&dir, "二次函数\t数学\n");

    let src = desktop.join("二次函数课件.pptx");
    std::fs::write(&src, b"course data").unwrap();
    common::set_mtime(&src, SystemTime::now() - Duration::from_secs(10 * 86400));

    build_scheduler(config_store.clone(), clock, desktop, &dir)
        .organize_once()
        .await
        .unwrap();

    assert!(!src.exists(), "源文件应被移走");
    let md = std::fs::read_to_string(dir.join("storage/台账.md")).unwrap();
    assert!(md.contains("1. 二次函数课件.pptx - 数学"), "{md}");

    let archives: Vec<_> = std::fs::read_dir(dir.join("storage/归档"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(archives.len(), 1, "{archives:?}");
    assert!(archives[0].ends_with(".zip"));
}

#[tokio::test]
async fn reload_lexicon_updates_classification() {
    let dir = common::TempDir::new();
    let (config_store, clock, desktop) = setup(&dir, "二次函数\t数学\n");

    let scheduler = build_scheduler(config_store.clone(), clock, desktop.clone(), &dir);

    // 词库改为「二次函数 → 物理」后热重载。
    std::fs::write(dir.join("config/lexicon.txt"), "二次函数\t物理\n").unwrap();
    scheduler.reload_lexicon().unwrap();

    let src = desktop.join("二次函数课件.pptx");
    std::fs::write(&src, b"course data").unwrap();
    common::set_mtime(&src, SystemTime::now() - Duration::from_secs(10 * 86400));

    scheduler.organize_once().await.unwrap();

    let md = std::fs::read_to_string(dir.join("storage/台账.md")).unwrap();
    assert!(md.contains("1. 二次函数课件.pptx - 物理"), "{md}");
}
