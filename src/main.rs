// Windows release 构建：隐藏控制台窗口（托盘程序双击运行不弹命令行窗口）。
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

fn main() -> anyhow::Result<()> {
    desktop_tidier::run()
}
