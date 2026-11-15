//! M5 整理器测试：扫描 → 分类 → 移动 → 记账，及 age 过滤。

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use desktop_tidier::classifier::Classifier;
use desktop_tidier::indexer::Indexer;
use desktop_tidier::organizer::Organizer;
use desktop_tidier::platform::PlatformAdapter;
use desktop_tidier::timesync::Clock;

fn build(cfg: &desktop_tidier::config::Config, desktop: &Path) -> Organizer {
    let classifier = Arc::new(
        Classifier::load(Path::new(&cfg.classifier.lexicon_path), "utf-8")
            .expect("classifier load"),
    );
    let platform: Arc<dyn PlatformAdapter> =
        Arc::new(common::FakePlatform::new(desktop.to_path_buf()));
    let indexer = Arc::new(Indexer);
    let clock: Arc<dyn Clock> = Arc::new(common::FixedClock(chrono::Utc::now()));
    Organizer::new(platform, classifier, indexer, clock)
}

#[tokio::test]
async fn moves_due_file_to_group_and_records() {
    let dir = common::TempDir::new();
    let desktop = dir.join("Desktop");
    std::fs::create_dir_all(&desktop).unwrap();

    let mut cfg = common::example_config(dir.path());
    cfg.watcher.source_dir = desktop.to_string_lossy().into_owned();
    cfg.watcher.min_age_days = 3;
    cfg.watcher.extensions = vec!["pptx".to_string()];
    cfg.storage.inbox_dir = dir.join("inbox").to_string_lossy().into_owned();
    cfg.storage.course_dir = dir.join("course").to_string_lossy().into_owned();
    cfg.storage.archive_dir = dir.join("archive").to_string_lossy().into_owned();
    cfg.indexer.md_path = dir.join("台账.md").to_string_lossy().into_owned();
    cfg.classifier.lexicon_path = dir.join("lexicon.txt").to_string_lossy().into_owned();
    cfg.notify.enabled = false;

    std::fs::write(&cfg.classifier.lexicon_path, "二次函数\t数学\n").unwrap();

    let src = desktop.join("二次函数课件.pptx");
    std::fs::write(&src, b"fake").unwrap();
    common::set_mtime(&src, SystemTime::now() - Duration::from_secs(10 * 86400));

    let report = build(&cfg, &desktop).run_once(&cfg).await.unwrap();

    assert_eq!(report.moved, 1);
    assert_eq!(report.failed, 0);
    assert!(!src.exists(), "源文件应被移走");
    assert!(dir.join("course/数学/二次函数课件.pptx").exists());
    let md = std::fs::read_to_string(&cfg.indexer.md_path).unwrap();
    assert!(md.contains("1. 二次函数课件.pptx - 数学"), "{md}");
}

#[tokio::test]
async fn skips_too_recent_file() {
    let dir = common::TempDir::new();
    let desktop = dir.join("Desktop");
    std::fs::create_dir_all(&desktop).unwrap();

    let mut cfg = common::example_config(dir.path());
    cfg.watcher.source_dir = desktop.to_string_lossy().into_owned();
    cfg.watcher.min_age_days = 30; // 要求 30 天
    cfg.watcher.extensions = vec!["pptx".to_string()];
    cfg.storage.inbox_dir = dir.join("inbox").to_string_lossy().into_owned();
    cfg.storage.course_dir = dir.join("course").to_string_lossy().into_owned();
    cfg.storage.archive_dir = dir.join("archive").to_string_lossy().into_owned();
    cfg.indexer.md_path = dir.join("台账.md").to_string_lossy().into_owned();
    cfg.classifier.lexicon_path = dir.join("lexicon.txt").to_string_lossy().into_owned();
    cfg.notify.enabled = false;
    std::fs::write(&cfg.classifier.lexicon_path, "二次函数\t数学\n").unwrap();

    let src = desktop.join("二次函数课件.pptx");
    std::fs::write(&src, b"fake").unwrap();
    common::set_mtime(&src, SystemTime::now() - Duration::from_secs(86400)); // 1 天

    let report = build(&cfg, &desktop).run_once(&cfg).await.unwrap();

    assert_eq!(report.skipped, 1);
    assert_eq!(report.moved, 0);
    assert!(src.exists(), "未到期的文件应保留");
}
