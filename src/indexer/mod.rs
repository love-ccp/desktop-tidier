//! MD 台账（M6）：追加式单文件，编号顺延，编码顺延，临时文件 + rename 原子写。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::NaiveDate;

use crate::Result;

/// 单条台账记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub filename: String,
    pub group: String,
    pub date: NaiveDate,
}

/// 台账写入器（无状态；由上层注入到 Organizer 等）。
#[derive(Debug, Clone, Copy, Default)]
pub struct Indexer;

impl Indexer {
    /// 扫描已有 MD 取最大编号（编号 = 每行行首连续数字；无文件 → 0）。
    pub fn max_existing_no(&self, md_path: &Path) -> Result<u64> {
        let bytes = match std::fs::read(md_path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        let mut max = 0u64;
        for line in bytes.split(|&b| b == b'\n') {
            if let Some(no) = parse_leading_no(line) {
                max = max.max(no);
            }
        }
        Ok(max)
    }

    /// 追加一行；返回分配的编号。读取现有编码后按相同编码顺延追加，
    /// 用临时文件 + `rename` 原子替换。
    pub fn record(&self, cfg: &crate::config::Config, entry: &Record) -> Result<u64> {
        let path = Path::new(&cfg.indexer.md_path);
        let no = self.max_existing_no(path)? + 1;

        let existing = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        let encoding = crate::encoding::append_encoding(&existing, &cfg.indexer.md_encoding)?;
        let line = render_entry(&cfg.indexer.entry_format, no, entry);

        let mut content = existing;
        if !content.is_empty() && !content.ends_with(b"\n") {
            content.push(b'\n');
        }
        content.extend_from_slice(&crate::encoding::encode(&format!("{line}\n"), encoding));

        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(path, &content)?;
        Ok(no)
    }
}

/// 解析行首连续 ASCII 数字（对应默认 `{no}. …` 的编号前置约定）。
fn parse_leading_no(line: &[u8]) -> Option<u64> {
    let mut num = 0u64;
    let mut any = false;
    for &b in line {
        if b.is_ascii_digit() {
            num = num.saturating_mul(10).saturating_add((b - b'0') as u64);
            any = true;
        } else {
            break;
        }
    }
    any.then_some(num)
}

/// 用 `entry_format` 渲染一行（占位符 `{no}` `{filename}` `{group}` `{date}`）。
fn render_entry(format: &str, no: u64, entry: &Record) -> String {
    format
        .replace("{no}", &no.to_string())
        .replace("{filename}", &entry.filename)
        .replace("{group}", &entry.group)
        .replace("{date}", &entry.date.format("%Y-%m-%d").to_string())
}

/// 临时文件 + rename 原子写（进程内唯一临时名，崩溃遗留 `.tmp*` 无副作用）。
fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let tmp = tmp_path(path);
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_path(path: &Path) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut os = path.as_os_str().to_os_string();
    os.push(format!(".tmp{}_{}", std::process::id(), n));
    PathBuf::from(os)
}
