//! M4 授时测试：Date 头解析、偏移校正、偏移落盘读取。

use chrono::{Datelike, Timelike, Utc};

use course_organizer::timesync::{parse_http_date, Clock, SyncedClock, Timesync};

#[test]
fn parses_rfc2822_date_header() {
    let dt = parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT").unwrap();
    assert_eq!(dt.year(), 1994);
    assert_eq!(dt.month(), 11);
    assert_eq!(dt.day(), 6);
    assert_eq!(dt.hour(), 8);
    assert_eq!(dt.minute(), 49);
    assert_eq!(dt.second(), 37);
}

#[test]
fn synced_clock_applies_offset() {
    let clock = SyncedClock::new(3600);
    let before = Utc::now();
    let now = clock.now();
    let after = Utc::now();
    // now 应约为本地墙钟 + 1 小时（夹在前后两次 Utc::now 之间）。
    let low = before + chrono::Duration::seconds(3600);
    let high = after + chrono::Duration::seconds(3600);
    assert!(now >= low && now <= high, "now={now} low={low} high={high}");
}

#[test]
fn load_offset_returns_zero_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("offset.json");
    assert_eq!(Timesync::load_offset(&path), 0);
}

#[test]
fn load_offset_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("offset.json");
    std::fs::write(
        &path,
        r#"{"offset_secs":-7,"updated_at":"2026-11-15T06:00:00Z"}"#,
    )
    .unwrap();
    assert_eq!(Timesync::load_offset(&path), -7);
}
