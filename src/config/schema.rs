//! 内置 JSON Schema 与示例配置：加载、编译校验器、生成中文错误消息。

use std::sync::OnceLock;

use jsonschema::error::ValidationErrorKind;
use jsonschema::{ValidationError, Validator};
use serde_json::Value;

use super::ValidationIssue;

const SCHEMA_STR: &str = include_str!("../../config/schema.json");
const EXAMPLE_STR: &str = include_str!("../../config/config.example.json");
const LEXICON_STR: &str = include_str!("../../config/lexicon.txt");
const DOC_STR: &str = include_str!("../../config/配置说明.md");

/// 首次运行时把 schema / 示例 / 词库 / 说明写入配置目录（不覆盖已有文件）。
///
/// 布局：`schema.json` 平铺在配置根目录（对应示例 `"$schema": "./schema.json"` 编辑器提示），
/// 其余文件放在 `config/` 子目录（对应 `lexicon_path: "config/lexicon.txt"` 与仓库布局）。
pub fn seed_support_files(config_dir: &std::path::Path) -> crate::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    // 供 config.json 的 `$schema` 编辑器提示定位（相对 config.json 所在目录）。
    write_if_missing(&config_dir.join("schema.json"), SCHEMA_STR)?;

    let dir = config_dir.join("config");
    std::fs::create_dir_all(&dir)?;
    write_if_missing(&dir.join("schema.json"), SCHEMA_STR)?;
    write_if_missing(&dir.join("config.example.json"), EXAMPLE_STR)?;
    write_if_missing(&dir.join("lexicon.txt"), LEXICON_STR)?;
    write_if_missing(&dir.join("配置说明.md"), DOC_STR)?;
    Ok(())
}

fn write_if_missing(path: &std::path::Path, content: &str) -> crate::Result<()> {
    if !path.exists() {
        std::fs::write(path, content)?;
    }
    Ok(())
}

/// 内置 schema（draft-07）。
///
/// 该常量为源码内嵌，解析失败即程序 bug，故此处用 `expect`（非用户输入）。
pub fn schema_value() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        serde_json::from_str(SCHEMA_STR).expect("内置 config/schema.json 必须是合法 JSON")
    })
}

/// 编译好的校验器（进程内复用，避免每次加载重复编译）。
pub fn validator() -> &'static Validator {
    static VALIDATOR: OnceLock<Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        jsonschema::validator_for(schema_value())
            .expect("内置 config/schema.json 必须是合法 JSON Schema")
    })
}

/// 内置示例配置原文（含 `$schema` 编辑器提示键）。
pub fn example_str() -> &'static str {
    EXAMPLE_STR
}

/// 校验（`value` 需已去掉 `$schema`）。通过返回 `Ok(())`，否则返回中文错误消息。
pub fn validate(value: &Value) -> std::result::Result<(), String> {
    let issues: Vec<ValidationIssue> = validator()
        .iter_errors(value)
        .map(|err| build_issue(&err))
        .collect();
    if issues.is_empty() {
        Ok(())
    } else {
        let message = issues
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        Err(message)
    }
}

/// 由一条 jsonschema 错误生成中文问题描述。
fn build_issue(err: &ValidationError<'_>) -> ValidationIssue {
    let pointer = err.instance_path().as_str().to_string();
    match err.kind() {
        ValidationErrorKind::Required { property } => {
            let prop = property.as_str().unwrap_or("?").to_string();
            let full = join_pointer(&pointer, &prop);
            ValidationIssue {
                pointer: full.clone(),
                field: dotted(&full),
                expected: "必填字段".to_string(),
                actual: "缺失".to_string(),
                suggestion: format!("请补充 \"{prop}\" 字段"),
            }
        }
        ValidationErrorKind::AdditionalProperties { unexpected } => {
            let name = unexpected.first().cloned().unwrap_or_default();
            let full = join_pointer(&pointer, &name);
            ValidationIssue {
                pointer: full.clone(),
                field: dotted(&full),
                expected: "不允许出现的字段".to_string(),
                actual: format!("\"{name}\""),
                suggestion: "请删除多余字段".to_string(),
            }
        }
        _ => {
            let node = lookup_node(schema_value(), &pointer);
            let actual = serde_json::to_string(err.instance().as_ref()).unwrap_or_default();
            let (expected, suggestion) = describe(node, err.kind());
            ValidationIssue {
                pointer: pointer.clone(),
                field: dotted(&pointer),
                expected,
                actual,
                suggestion,
            }
        }
    }
}

/// 依据 schema 节点与错误类型生成「期望描述 + 改正建议」。
fn describe(node: Option<&Value>, kind: &ValidationErrorKind) -> (String, String) {
    let node = node.unwrap_or(&Value::Null);
    match kind {
        ValidationErrorKind::Type { .. } => {
            let expected = type_desc(node);
            let ty = node.get("type").and_then(Value::as_str).unwrap_or("值");
            let suggestion = match ty {
                "integer" => format!("请改为整数，例如 {}", example_for(node)),
                "string" => format!("请改为字符串，例如 {}", example_for(node)),
                "boolean" => "请改为 true 或 false".to_string(),
                "array" => "请改为数组，例如 [...]".to_string(),
                "object" => "请改为对象".to_string(),
                _ => "请检查该字段的类型".to_string(),
            };
            (expected, suggestion)
        }
        ValidationErrorKind::Enum { options } => {
            let strs = enum_strs(options);
            let example = strs.first().copied().unwrap_or("");
            (
                format!("枚举值 {}", strs.join("/")),
                format!("请从枚举值中选择，例如 {example}"),
            )
        }
        ValidationErrorKind::Pattern { pattern } => (
            format!("匹配正则 {pattern} 的字符串"),
            format!("请改为匹配 {pattern} 的字符串"),
        ),
        ValidationErrorKind::Minimum { limit } => {
            let l = value_str(limit);
            (format!("≥ {l}"), format!("请改为 ≥ {l} 的整数"))
        }
        ValidationErrorKind::Maximum { limit } => {
            let l = value_str(limit);
            (format!("≤ {l}"), format!("请改为 ≤ {l} 的整数"))
        }
        ValidationErrorKind::MinItems { limit } => {
            (format!("至少 {limit} 项"), format!("请提供至少 {limit} 项"))
        }
        ValidationErrorKind::Format { format } => (
            format!("格式 {format}"),
            format!("请改为符合 {format} 格式的值"),
        ),
        _ => (type_desc(node), "请检查该字段的取值".to_string()),
    }
}

/// 由 schema 节点生成中文类型描述，如「非负整数(0~36500)」。
fn type_desc(node: &Value) -> String {
    match node.get("type").and_then(Value::as_str) {
        Some("integer") => {
            let min = node.get("minimum").and_then(Value::as_u64);
            let max = node.get("maximum").and_then(Value::as_u64);
            match (min, max) {
                (Some(0), Some(mx)) => format!("非负整数(0~{mx})"),
                (Some(mn), Some(mx)) => format!("整数({mn}~{mx})"),
                (Some(0), None) => "非负整数".to_string(),
                (Some(mn), None) => format!("≥{mn} 的整数"),
                (None, Some(mx)) => format!("≤{mx} 的整数"),
                (None, None) => "整数".to_string(),
            }
        }
        Some("string") => "字符串".to_string(),
        Some("boolean") => "布尔值(true/false)".to_string(),
        Some("array") => "数组".to_string(),
        Some("object") => "对象".to_string(),
        Some(other) => other.to_string(),
        None => "值".to_string(),
    }
}

/// 由 schema 节点生成一个合法示例值（用于改正建议）。
fn example_for(node: &Value) -> String {
    match node.get("type").and_then(Value::as_str) {
        Some("integer") => {
            let min = node.get("minimum").and_then(Value::as_u64).unwrap_or(0);
            let max = node
                .get("maximum")
                .and_then(Value::as_u64)
                .unwrap_or(u64::MAX);
            min.max(1).min(max).to_string()
        }
        Some("string") => enum_strs(node.get("enum").unwrap_or(&Value::Null))
            .first()
            .map(|s| (*s).to_string())
            .unwrap_or_else(|| "\"示例字符串\"".to_string()),
        Some("boolean") => "true".to_string(),
        Some("array") => "[\"...\"]".to_string(),
        _ => "…".to_string(),
    }
}

/// 提取 `enum` 数组里的字符串项。
fn enum_strs(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// 将 JSON 值序列化为紧凑字符串（用于「实际得到」与数值上下限）。
fn value_str(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// 在 JSON Pointer 上追加一个段。
fn join_pointer(pointer: &str, segment: &str) -> String {
    if segment.is_empty() {
        return pointer.to_string();
    }
    if pointer.is_empty() {
        return format!("/{segment}");
    }
    format!("{pointer}/{segment}")
}

/// JSON Pointer → 点分字段名，如 `/watcher/min_age_days` → `watcher.min_age_days`。
fn dotted(pointer: &str) -> String {
    if pointer.is_empty() {
        return "根节点".to_string();
    }
    pointer.trim_start_matches('/').replace('/', ".")
}

/// 由 JSON Pointer 在 schema 树上定位对应节点（处理嵌套 `properties` 与数组 `items`）。
fn lookup_node<'a>(schema: &'a Value, pointer: &str) -> Option<&'a Value> {
    let mut node = schema;
    if pointer.is_empty() {
        return Some(node);
    }
    for segment in pointer.split('/').filter(|s| !s.is_empty()) {
        if segment.parse::<usize>().is_ok() {
            node = node.get("items")?;
        } else {
            node = node.get("properties")?.get(segment)?;
        }
    }
    Some(node)
}
