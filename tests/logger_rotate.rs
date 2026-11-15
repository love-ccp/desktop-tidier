//! M2 单文件日志 writer 测试：轮转（1 个日志 + 1 个 .bak）与写入内容。

mod common;

use course_organizer::logger::SingleFileMakeWriter;

#[test]
fn rotates_keeping_single_bak() {
    let dir = common::TempDir::new();
    let log = dir.join("organizer.log");

    let writer = SingleFileMakeWriter::new(&log, 512);
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(writer)
        .with_ansi(false)
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        for i in 0..80 {
            tracing::info!("第 {} 条日志", i);
        }
    });

    let bak = dir.join("organizer.log.bak");
    assert!(log.exists(), "日志文件应存在");
    assert!(bak.exists(), "应存在 1 个 .bak");

    // .bak 是轮转前的满日志（轮转在下次写入时惰性触发，故当前日志可能略超阈值）。
    let bak_size = std::fs::metadata(&bak).unwrap().len();
    assert!(bak_size >= 512, ".bak 应为轮转前的满日志，实际 {bak_size}");

    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        entries.len(),
        2,
        "应只有 1 个日志 + 1 个 .bak，实际 {entries:?}"
    );
}

#[test]
fn writes_events_to_file() {
    let dir = common::TempDir::new();
    let log = dir.join("organizer.log");

    let writer = SingleFileMakeWriter::new(&log, 1024 * 1024);
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(writer)
        .with_ansi(false)
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("你好，课件整理");
    });

    let content = std::fs::read_to_string(&log).unwrap();
    assert!(content.contains("你好，课件整理"), "content = {content}");
}
