//! M1 配置校验错误测试：坏 JSON 的字段路径、期望、建议与热重载降级。

mod common;

use course_organizer::config::{example_str, Config, ConfigStore, ReloadOutcome};

/// 读内置示例为 Value，逐字段破坏后写回 `config.json`，返回临时目录。
fn write_bad(mut mutate: impl FnMut(&mut serde_json::Value)) -> common::TempDir {
    let dir = common::TempDir::new();
    let path = dir.join("config.json");
    let mut value: serde_json::Value = serde_json::from_str(example_str()).unwrap();
    mutate(&mut value);
    std::fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();
    dir
}

#[test]
fn wrong_type_reports_pointer_and_suggestion() {
    let dir = write_bad(|v| {
        v["watcher"]["min_age_days"] = serde_json::json!("abc");
    });
    let err = Config::load(&dir.join("config.json")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("/watcher/min_age_days"), "msg = {msg}");
    assert!(msg.contains("min_age_days"), "msg = {msg}");
    assert!(msg.contains("非负整数"), "msg = {msg}");
    assert!(msg.contains("建议"), "msg = {msg}");
}

#[test]
fn missing_required_reports_field() {
    let dir = write_bad(|v| {
        v.as_object_mut().unwrap().remove("storage");
    });
    let err = Config::load(&dir.join("config.json")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("storage"), "msg = {msg}");
    assert!(msg.contains("必填"), "msg = {msg}");
    assert!(msg.contains("建议"), "msg = {msg}");
}

#[test]
fn unknown_field_reports_suggestion() {
    let dir = write_bad(|v| {
        v["watcher"]["extra_field"] = serde_json::json!(1);
    });
    let err = Config::load(&dir.join("config.json")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("watcher.extra_field"), "msg = {msg}");
    assert!(msg.contains("不允许"), "msg = {msg}");
    assert!(msg.contains("建议"), "msg = {msg}");
}

#[test]
fn reload_rejected_keeps_old_value() {
    let dir = common::TempDir::new();
    let path = dir.join("config.json");
    Config::write_example(&path).unwrap();
    let store = ConfigStore::load(&path).unwrap();
    let before = store.get().watcher.min_age_days;

    let mut value: serde_json::Value = serde_json::from_str(example_str()).unwrap();
    value["watcher"]["min_age_days"] = serde_json::json!("abc");
    std::fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();

    match store.reload().unwrap() {
        ReloadOutcome::Rejected(_) => {}
        ReloadOutcome::Reloaded(_) => panic!("坏配置不应重载成功"),
    }
    assert_eq!(store.get().watcher.min_age_days, before);
}

#[test]
fn reload_success_replaces_value() {
    let dir = common::TempDir::new();
    let path = dir.join("config.json");
    Config::write_example(&path).unwrap();
    let store = ConfigStore::load(&path).unwrap();

    let mut value: serde_json::Value = serde_json::from_str(example_str()).unwrap();
    value["watcher"]["min_age_days"] = serde_json::json!(7);
    std::fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();

    match store.reload().unwrap() {
        ReloadOutcome::Reloaded(cfg) => assert_eq!(cfg.watcher.min_age_days, 7),
        ReloadOutcome::Rejected(_) => panic!("合法配置应重载成功"),
    }
    assert_eq!(store.get().watcher.min_age_days, 7);
}
