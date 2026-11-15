//! M7 归档器测试：zip 内容、回收清空。

mod common;

use std::sync::Arc;

use course_organizer::archiver::Archiver;
use course_organizer::platform::PlatformAdapter;
use course_organizer::timesync::Clock;

#[tokio::test]
async fn archives_to_zip_and_recycles() {
    let dir = common::TempDir::new();

    let mut cfg = common::example_config(dir.path());
    cfg.storage.course_dir = dir.join("course").to_string_lossy().into_owned();
    cfg.storage.archive_dir = dir.join("archive").to_string_lossy().into_owned();
    cfg.archiver.rename_pattern = "{date}_{seq}_course.zip".to_string();
    cfg.storage.recycle_after_archive = true;

    let group = dir.join("course/数学");
    std::fs::create_dir_all(&group).unwrap();
    let file = group.join("二次函数课件.pptx");
    std::fs::write(&file, b"course data").unwrap();

    let platform: Arc<dyn PlatformAdapter> =
        Arc::new(common::FakePlatform::new(dir.join("Desktop")));
    let clock: Arc<dyn Clock> = Arc::new(common::FixedClock(chrono::Utc::now()));
    let report = Archiver::new(platform, clock).run_once(&cfg).await.unwrap();

    assert_eq!(report.archived, 1);
    assert_eq!(report.recycled, 1);
    assert!(report.zipped_to.exists(), "zip 应已生成");
    assert!(!file.exists(), "course_dir 应已清空");

    // 校验 zip 内容与分组目录结构。
    let f = std::fs::File::open(&report.zipped_to).unwrap();
    let mut archive = zip::ZipArchive::new(f).unwrap();
    assert_eq!(archive.len(), 1);
    let name = archive.by_index(0).unwrap().name().unwrap().into_owned();
    assert_eq!(name, "数学/二次函数课件.pptx");
}

#[tokio::test]
async fn empty_course_dir_skips() {
    let dir = common::TempDir::new();
    let mut cfg = common::example_config(dir.path());
    cfg.storage.course_dir = dir.join("course").to_string_lossy().into_owned();
    cfg.storage.archive_dir = dir.join("archive").to_string_lossy().into_owned();

    let platform: Arc<dyn PlatformAdapter> =
        Arc::new(common::FakePlatform::new(dir.join("Desktop")));
    let clock: Arc<dyn Clock> = Arc::new(common::FixedClock(chrono::Utc::now()));
    let report = Archiver::new(platform, clock).run_once(&cfg).await.unwrap();

    assert_eq!(report.archived, 0);
    assert_eq!(report.recycled, 0);
    assert!(!report.zipped_to.exists(), "空目录不应生成 zip");
}
