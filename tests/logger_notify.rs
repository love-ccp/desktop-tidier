//! M2 日志通知测试：错误级日志触发系统通知（受 notify.enabled 控制）。

mod common;

use std::sync::Arc;

use course_organizer::config::ConfigStore;
use course_organizer::logger::NotifyLayer;
use course_organizer::platform::PlatformAdapter;
use tracing_subscriber::layer::SubscriberExt;

#[test]
fn error_log_notifies_when_enabled() {
    let dir = common::TempDir::new();
    let cfg_path = dir.join("config.json");
    course_organizer::config::Config::write_example(&cfg_path).unwrap();
    let config_store = Arc::new(ConfigStore::load(&cfg_path).unwrap());

    let fake = Arc::new(common::FakePlatform::new(dir.join("Desktop")));
    let platform: Arc<dyn PlatformAdapter> = fake.clone();

    let subscriber = tracing_subscriber::registry().with(NotifyLayer::new(platform, config_store));
    tracing::subscriber::with_default(subscriber, || {
        tracing::error!("测试错误 {}", 42);
    });

    let notified = fake.notified.lock().expect("lock");
    assert!(
        notified.iter().any(|s| s.contains("测试错误 42")),
        "{notified:?}"
    );
}

#[test]
fn info_log_does_not_notify() {
    let dir = common::TempDir::new();
    let cfg_path = dir.join("config.json");
    course_organizer::config::Config::write_example(&cfg_path).unwrap();
    let config_store = Arc::new(ConfigStore::load(&cfg_path).unwrap());

    let fake = Arc::new(common::FakePlatform::new(dir.join("Desktop")));
    let platform: Arc<dyn PlatformAdapter> = fake.clone();

    let subscriber = tracing_subscriber::registry().with(NotifyLayer::new(platform, config_store));
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("普通信息");
    });

    assert!(fake.notified.lock().expect("lock").is_empty());
}
