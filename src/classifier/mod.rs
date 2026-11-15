//! 关键词分类器（M5）：aho-corasick 多模式匹配，关键词 → 分组。

use std::collections::HashSet;
use std::path::Path;

use aho_corasick::AhoCorasick;

use crate::Result;

/// 关键词分组器：`pattern_id -> group`。
pub struct Classifier {
    /// 无关键词时为空（`classify` 恒返回 `None`）。
    ac: Option<AhoCorasick>,
    groups: Vec<String>,
}

impl Classifier {
    /// 从词库文件加载（按 `encoding` 解码）。
    pub fn load(path: &Path, encoding: &str) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        let text = crate::encoding::decode(&bytes, encoding)?;
        Self::from_lexicon(&text)
    }

    /// 解析词库文本。单行格式错误 / 空字段 / 重复关键词仅跳过该行并记日志，
    /// 不中断整体加载（符合「每行独立解析」）。
    pub fn from_lexicon(text: &str) -> Result<Self> {
        let mut patterns: Vec<String> = Vec::new();
        let mut groups: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() != 2 {
                tracing::warn!("词库第 {} 行格式错误（应恰好 1 个 TAB）：{line:?}", i + 1);
                continue;
            }
            let kw = parts[0].trim();
            let group = parts[1].trim();
            if kw.is_empty() || group.is_empty() {
                tracing::warn!("词库第 {} 行关键字或分组为空：{line:?}", i + 1);
                continue;
            }
            if !seen.insert(kw.to_string()) {
                tracing::warn!("词库第 {} 行关键字重复（首个生效）：{kw:?}", i + 1);
                continue;
            }
            patterns.push(kw.to_string());
            groups.push(group.to_string());
        }

        if patterns.is_empty() {
            return Ok(Self { ac: None, groups });
        }

        let ac = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(&patterns)
            .map_err(|e| crate::Error::Msg(format!("构建关键词自动机失败：{e}")))?;

        Ok(Self {
            ac: Some(ac),
            groups,
        })
    }

    /// 命中返回分组，未命中返回 `None`（左起首个命中）。
    pub fn classify(&self, filename: &str) -> Option<&str> {
        let ac = self.ac.as_ref()?;
        ac.find(filename)
            .map(|m| self.groups[m.pattern().as_usize()].as_str())
    }
}
