//! M6 台账测试：编号顺延、追加、编码、原子写。

mod common;

use chrono::NaiveDate;

use desktop_tidier::indexer::{Indexer, Record};

fn record(no_expected: u64) -> Record {
    Record {
        filename: format!("第{no_expected}个课件.pptx"),
        group: "数学".to_string(),
        date: NaiveDate::from_ymd_opt(2026, 11, 15).expect("date"),
    }
}

#[test]
fn records_with_incrementing_no() {
    let dir = common::TempDir::new();
    let mut cfg = common::example_config(dir.path());
    cfg.indexer.md_path = dir.join("台账.md").to_string_lossy().into_owned();

    let idx = Indexer;
    assert_eq!(idx.record(&cfg, &record(1)).unwrap(), 1);
    assert_eq!(idx.record(&cfg, &record(2)).unwrap(), 2);

    let text = std::fs::read_to_string(&cfg.indexer.md_path).unwrap();
    assert!(
        text.contains("1. 第1个课件.pptx - 数学 - 2026-11-15"),
        "{text}"
    );
    assert!(
        text.contains("2. 第2个课件.pptx - 数学 - 2026-11-15"),
        "{text}"
    );
}

#[test]
fn max_existing_no_reads_prior() {
    let dir = common::TempDir::new();
    let md = dir.join("台账.md");
    std::fs::write(&md, "1. a.pptx\n42. b.pptx\n99. c.pptx\n").unwrap();
    assert_eq!(Indexer.max_existing_no(&md).unwrap(), 99);
}

#[test]
fn max_existing_no_zero_when_missing() {
    let dir = common::TempDir::new();
    assert_eq!(Indexer.max_existing_no(&dir.join("不存在.md")).unwrap(), 0);
}
