//! M0 冒烟测试：库可正常链接、版本号有效。
//! （`run()` 在 M9 起需要桌面环境，故不在此单元测试中调用。）

#[test]
fn version_is_set() {
    assert!(!course_organizer::VERSION.is_empty());
    assert!(course_organizer::VERSION.starts_with("0."));
}
