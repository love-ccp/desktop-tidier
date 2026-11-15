//! M0 冒烟测试：入口函数可正常返回。

#[test]
fn run_returns_ok() {
    course_organizer::run().unwrap();
}
