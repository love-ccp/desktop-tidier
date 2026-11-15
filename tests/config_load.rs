//! M1 配置加载测试：示例加载、自动生成、字段值与相对路径解析。

mod common;

use desktop_tidier::config::{example_str, Config};

#[test]
fn example_loads_with_correct_values() {
    let dir = common::TempDir::new();
    let path = dir.join("config.json");
    Config::write_example(&path).unwrap();

    let config = Config::load(&path).unwrap();
    let base = path.parent().unwrap().to_path_buf();

    assert_eq!(config.watcher.min_age_days, 7);
    assert_eq!(config.watcher.debounce_ms, 400);
    assert_eq!(config.watcher.scan_interval_secs, 172_800);
    assert_eq!(config.watcher.source_dir, "");
    assert_eq!(config.watcher.extensions.len(), 10);
    assert!(config.watcher.extensions.contains(&"enbx".to_string()));

    assert_eq!(config.archiver.zip_level, 6);
    assert_eq!(config.archiver.interval_hours, 168);
    assert_eq!(config.archiver.max_archives, 0);
    assert_eq!(config.archiver.max_total_bytes, 0);
    assert_eq!(config.archiver.min_new_files, 1);
    assert_eq!(config.logger.max_bytes, 1_048_576);
    assert_eq!(config.logger.level, "info");
    assert_eq!(config.concurrency.max_parallel_ops, 4);
    assert_eq!(config.notify.app_name, "课件整理助手");
    assert!(config.notify.enabled);
    assert!(config.storage.recycle_after_archive);

    // 相对路径已在加载时解析为基于配置目录的绝对路径。
    assert_eq!(
        config.storage.course_dir,
        base.join("storage/课件库").to_string_lossy().into_owned()
    );
    assert_eq!(
        config.logger.file,
        base.join("logs/desktop-tidier.log")
            .to_string_lossy()
            .into_owned()
    );
    assert_eq!(
        config.classifier.lexicon_path,
        base.join("lexicon.txt").to_string_lossy().into_owned()
    );
}

#[test]
fn missing_file_generates_example() {
    let dir = common::TempDir::new();
    let path = dir.join("config.json");
    assert!(!path.exists());

    Config::load(&path).unwrap();

    let generated = std::fs::read_to_string(&path).unwrap();
    assert_eq!(generated, example_str());
}

#[test]
fn first_run_seeds_support_files() {
    let dir = common::TempDir::new();
    Config::load(&dir.join("config.json")).unwrap();

    // 扁平布局（无歧义）：config.json 是唯一生效配置，其余为参考/数据文件，全部平铺。
    assert!(dir.join("config.json").exists());
    assert!(dir.join("schema.json").exists());
    assert!(dir.join("config.example.json").exists());
    assert!(dir.join("lexicon.txt").exists());
    assert!(dir.join("配置说明.md").exists());
    // 不再生成容易与生效配置混淆的 config/ 子目录。
    assert!(!dir.join("config/lexicon.txt").exists());
}

#[test]
fn resolve_paths_is_idempotent() {
    let dir = common::TempDir::new();
    let path = dir.join("config.json");
    Config::write_example(&path).unwrap();

    let mut config = Config::load(&path).unwrap();
    let once = config.storage.course_dir.clone();
    config.resolve_paths(path.parent().unwrap());
    assert_eq!(config.storage.course_dir, once, "重复解析不应二次拼接");
}

#[test]
fn schema_hint_key_is_accepted() {
    // `$schema` 是可选编辑器提示键：schema 已声明该属性，加载时不再当作多余字段。
    let dir = common::TempDir::new();
    let path = dir.join("config.json");
    let mut value: serde_json::Value = serde_json::from_str(example_str()).unwrap();
    value["$schema"] = serde_json::json!("https://example.local/desktop-tidier/schema.json");
    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

    let config = Config::load(&path).unwrap();
    assert_eq!(config.watcher.min_age_days, 7);
}
