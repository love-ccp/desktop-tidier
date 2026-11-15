//! M8 编排器测试：整理与归档解耦、台账阈值跳过归档、状态落盘。

mod common;

use std::sync::Arc;

use desktop_tidier::config::ConfigStore;
use desktop_tidier::platform::PlatformAdapter;
use desktop_tidier::scheduler::Scheduler;
use desktop_tidier::state::StateStore;
use desktop_tidier::timesync::{Clock, SyncedClock, Timesync};

fn write_config(dir: &common::TempDir, mutate: impl FnOnce(&mut serde_json::Value)) {
    let cfg_path = dir.join("config.json");
    let mut value: serde_json::Value =
        serde_json::from_str(desktop_tidier::config::example_str()).unwrap();
    mutate(&mut value);
    std::fs::write(&cfg_path, serde_json::to_string(&value).unwrap()).unwrap();
}

fn setup(
    dir: &common::TempDir,
    lexicon: &str,
) -> (Arc<ConfigStore>, Arc<dyn Clock>, std::path::PathBuf) {
    write_config(dir, |_| {});
    // 词库平铺在配置目录（示例 lexicon_path: "lexicon.txt"）。
    std::fs::write(dir.join("lexicon.txt"), lexicon).unwrap();

    let desktop = dir.join("Desktop");
    std::fs::create_dir_all(&desktop).unwrap();

    let config_store = Arc::new(ConfigStore::load(&dir.join("config.json")).unwrap());
    // 固定时钟拨快 10 天：文件创建时间 ≈ 现在，等效于「文件已存在 10 天」。
    let clock: Arc<dyn Clock> = Arc::new(common::clock_days_from_now(10));
    (config_store, clock, desktop)
}

fn build_scheduler(
    dir: &common::TempDir,
    config_store: Arc<ConfigStore>,
    clock: Arc<dyn Clock>,
    desktop: std::path::PathBuf,
) -> Scheduler {
    let platform: Arc<dyn PlatformAdapter> = Arc::new(common::FakePlatform::new(desktop));
    // 授时服务需要具体 SyncedClock（更新偏移），与测试用的 FixedClock 分离。
    let timesync =
        Arc::new(Timesync::new(SyncedClock::shared(0), dir.join("offset.json")).unwrap());
    let state = Arc::new(StateStore::load(&dir.join("storage/state.json")));
    Scheduler::new(config_store, platform, clock, timesync, state).unwrap()
}

/// 在桌面放一个刚创建的课件文件；配合拨快 10 天的固定时钟模拟「10 天前创建」
/// （满足默认 min_age_days=7）。
fn put_course_file(desktop: &std::path::Path) {
    let src = desktop.join("二次函数课件.pptx");
    std::fs::write(&src, b"course data").unwrap();
}

#[tokio::test]
async fn organize_once_organizes_but_does_not_archive() {
    let dir = common::TempDir::new();
    let (config_store, clock, desktop) = setup(&dir, "二次函数\t数学\n");
    put_course_file(&desktop);

    build_scheduler(&dir, config_store.clone(), clock, desktop.clone())
        .organize_once()
        .await
        .unwrap();

    // 整理：源文件进入课件库分组 + 记账。
    assert!(
        !desktop.join("二次函数课件.pptx").exists(),
        "源文件应被移走"
    );
    assert!(
        dir.join("storage/课件库/数学/二次函数课件.pptx").exists(),
        "文件应进课件库分组"
    );
    let md = std::fs::read_to_string(dir.join("storage/台账.md")).unwrap();
    assert!(md.contains("1. 二次函数课件.pptx - 数学"), "{md}");

    // 归档与整理完全解耦：「立刻整理」不触发归档。
    let zip_count = std::fs::read_dir(dir.join("storage/归档"))
        .map(|rd| rd.count())
        .unwrap_or(0);
    assert_eq!(zip_count, 0, "立刻整理不应触发归档");
}

#[tokio::test]
async fn archive_once_archives_when_threshold_met() {
    let dir = common::TempDir::new();
    let (config_store, clock, desktop) = setup(&dir, "二次函数\t数学\n");
    put_course_file(&desktop);

    let scheduler = build_scheduler(&dir, config_store.clone(), clock, desktop.clone());
    scheduler.organize_once().await.unwrap();
    // min_new_files 默认 1：台账新增 1 ≥ 1 → 归档应执行。
    scheduler.archive_once().await.unwrap();

    let archives: Vec<_> = std::fs::read_dir(dir.join("storage/归档"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(archives.len(), 1, "{archives:?}");
    assert!(archives[0].ends_with(".zip"), "{archives:?}");
    // recycle_after_archive 默认 true：课件库被清空。
    assert!(!dir.join("storage/课件库/数学/二次函数课件.pptx").exists());

    // 台账水位已推进并落盘。
    let state = std::fs::read_to_string(dir.join("storage/state.json")).unwrap();
    assert!(state.contains("\"last_archive_record_no\": 1"), "{state}");
}

#[tokio::test]
async fn archive_skipped_when_below_min_new_files() {
    let dir = common::TempDir::new();
    write_config(&dir, |v| {
        v["archiver"]["min_new_files"] = serde_json::json!(5);
    });
    std::fs::write(dir.join("lexicon.txt"), "二次函数\t数学\n").unwrap();
    let desktop = dir.join("Desktop");
    std::fs::create_dir_all(&desktop).unwrap();
    let config_store = Arc::new(ConfigStore::load(&dir.join("config.json")).unwrap());
    // 固定时钟拨快 10 天：文件创建时间 ≈ 现在，等效于「文件已存在 10 天」。
    let clock: Arc<dyn Clock> = Arc::new(common::clock_days_from_now(10));

    put_course_file(&desktop);
    let scheduler = build_scheduler(&dir, config_store.clone(), clock, desktop.clone());
    scheduler.organize_once().await.unwrap();
    scheduler.archive_once().await.unwrap();

    // 新增 1 < 阈值 5：跳过本次归档 → 无 zip、课件库文件保留、水位不推进。
    let zip_count = std::fs::read_dir(dir.join("storage/归档"))
        .map(|rd| rd.count())
        .unwrap_or(0);
    assert_eq!(zip_count, 0, "低于阈值不应产生归档");
    assert!(
        dir.join("storage/课件库/数学/二次函数课件.pptx").exists(),
        "跳过归档时课件库文件应保留"
    );
    let state = std::fs::read_to_string(dir.join("storage/state.json")).unwrap();
    assert!(state.contains("\"last_archive_record_no\": 0"), "{state}");
}

#[tokio::test]
async fn reload_lexicon_updates_classification() {
    let dir = common::TempDir::new();
    let (config_store, clock, desktop) = setup(&dir, "二次函数\t数学\n");

    let scheduler = build_scheduler(&dir, config_store.clone(), clock, desktop.clone());

    // 词库改为「二次函数 → 物理」后热重载（词库平铺在配置目录）。
    std::fs::write(dir.join("lexicon.txt"), "二次函数\t物理\n").unwrap();
    scheduler.reload_lexicon().unwrap();

    put_course_file(&desktop);
    scheduler.organize_once().await.unwrap();

    let md = std::fs::read_to_string(dir.join("storage/台账.md")).unwrap();
    assert!(md.contains("1. 二次函数课件.pptx - 物理"), "{md}");
}
