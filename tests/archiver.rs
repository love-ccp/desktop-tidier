//! M7 归档器测试：zip 内容、回收清空。

mod common;

use std::sync::Arc;

use desktop_tidier::archiver::Archiver;
use desktop_tidier::platform::PlatformAdapter;
use desktop_tidier::timesync::Clock;

#[tokio::test]
async fn archives_to_zip_and_recycles() {
    let dir = common::TempDir::new();

    let mut cfg = common::example_config(dir.path());
    cfg.storage.course_dir = dir.join("course").to_string_lossy().into_owned();
    cfg.storage.archive_dir = dir.join("archive").to_string_lossy().into_owned();
    cfg.archiver.rename_pattern = "{date}_{seq}_desktop-tidier.zip".to_string();
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

#[tokio::test]
async fn purges_oldest_when_over_max_archives() {
    let dir = common::TempDir::new();

    let mut cfg = common::example_config(dir.path());
    cfg.storage.course_dir = dir.join("course").to_string_lossy().into_owned();
    cfg.storage.archive_dir = dir.join("archive").to_string_lossy().into_owned();
    cfg.storage.recycle_after_archive = false;
    cfg.archiver.max_archives = 2;

    // 预置两个旧 zip（字典序更旧）。
    let archive = dir.join("archive");
    std::fs::create_dir_all(&archive).unwrap();
    let old1 = archive.join("20200101_1_desktop-tidier.zip");
    let old2 = archive.join("20200102_1_desktop-tidier.zip");
    std::fs::write(&old1, b"old1").unwrap();
    std::fs::write(&old2, b"old2").unwrap();

    // 一个新文件待归档。
    let group = dir.join("course/数学");
    std::fs::create_dir_all(&group).unwrap();
    std::fs::write(group.join("二次函数课件.pptx"), b"course data").unwrap();

    let platform: Arc<dyn PlatformAdapter> =
        Arc::new(common::FakePlatform::new(dir.join("Desktop")));
    let clock: Arc<dyn Clock> = Arc::new(common::FixedClock(chrono::Utc::now()));
    let report = Archiver::new(platform, clock).run_once(&cfg).await.unwrap();

    assert_eq!(report.archived, 1);
    assert_eq!(report.purged, 1, "3 个 zip > 上限 2，应清理最旧的 1 个");
    assert!(!old1.exists(), "最旧的应被清理");
    assert!(old2.exists(), "次新的应保留");
    assert!(report.zipped_to.exists(), "本次归档应保留");
}

#[tokio::test]
async fn purges_oldest_when_over_max_total_bytes() {
    let dir = common::TempDir::new();

    let mut cfg = common::example_config(dir.path());
    cfg.storage.course_dir = dir.join("course").to_string_lossy().into_owned();
    cfg.storage.archive_dir = dir.join("archive").to_string_lossy().into_owned();
    cfg.storage.recycle_after_archive = false;
    cfg.archiver.max_total_bytes = 20;

    let archive = dir.join("archive");
    std::fs::create_dir_all(&archive).unwrap();
    let old1 = archive.join("20200101_1_desktop-tidier.zip");
    let old2 = archive.join("20200102_1_desktop-tidier.zip");
    std::fs::write(&old1, b"1234567").unwrap(); // 7 字节
    std::fs::write(&old2, b"1234567").unwrap(); // 7 字节

    let group = dir.join("course/数学");
    std::fs::create_dir_all(&group).unwrap();
    std::fs::write(group.join("二次函数课件.pptx"), b"course data").unwrap();

    let platform: Arc<dyn PlatformAdapter> =
        Arc::new(common::FakePlatform::new(dir.join("Desktop")));
    let clock: Arc<dyn Clock> = Arc::new(common::FixedClock(chrono::Utc::now()));
    let report = Archiver::new(platform, clock).run_once(&cfg).await.unwrap();

    // 新 zip 本身即超过 20 字节上限：旧 zip 全部清理，但绝不删除本次产物。
    assert_eq!(report.archived, 1);
    assert_eq!(report.purged, 2);
    assert!(!old1.exists());
    assert!(!old2.exists());
    assert!(report.zipped_to.exists(), "本次归档产物不得被上限管理删除");
}
