//! M5 分类器测试：关键词命中、大小写、重复、格式错误行。

use desktop_tidier::classifier::Classifier;

#[test]
fn classifies_by_keyword() {
    let c = Classifier::from_lexicon("二次函数\t数学\n牛顿定律\t物理\n# 注释\n").unwrap();
    assert_eq!(c.classify("二次函数课件.pptx"), Some("数学"));
    assert_eq!(c.classify("牛顿定律实验.docx"), Some("物理"));
    assert_eq!(c.classify("无关键词.pdf"), None);
}

#[test]
fn ascii_case_insensitive() {
    let c = Classifier::from_lexicon("PPT\t演示\n").unwrap();
    assert_eq!(c.classify("my.ppt"), Some("演示"));
    assert_eq!(c.classify("MY.PPT"), Some("演示"));
}

#[test]
fn duplicate_keyword_first_wins() {
    let c = Classifier::from_lexicon("力学\t物理\n力学\t化学\n").unwrap();
    assert_eq!(c.classify("力学题.docx"), Some("物理"));
}

#[test]
fn malformed_line_skipped() {
    let c = Classifier::from_lexicon("a\tb\n没有tab的行\nc\td\n").unwrap();
    assert_eq!(c.classify("a.txt"), Some("b"));
    assert_eq!(c.classify("c.txt"), Some("d"));
}

#[test]
fn empty_lexicon_matches_nothing() {
    let c = Classifier::from_lexicon("# 只有注释\n").unwrap();
    assert_eq!(c.classify("任意.pptx"), None);
}
