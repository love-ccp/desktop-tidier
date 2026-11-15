//! 系统托盘（M9）：刷子图标 + 固定四菜单（设置开机自启 / 立刻整理 / 打开配置文件目录 / 退出）。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "windows")]
pub(crate) mod windows;

/// 托盘菜单命令（菜单 → tokio 侧处理）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    ToggleAutostart,
    OrganizeNow,
    OpenConfigDir,
    Quit,
}

/// 托盘图标的「自启勾选态」由 tokio 侧更新、托盘侧同步显示。
pub type AutostartFlag = Arc<AtomicBool>;

/// 刷子样式托盘图标（32×32 RGBA），assets/ 后续可替换为正式图标。
///
/// 简单示意：一根棕色斜向手柄 + 浅色刷毛 + 蓝色颜料点。
pub fn brush_icon_rgba() -> (u32, u32, Vec<u8>) {
    const W: i32 = 32;
    const H: i32 = 32;
    let mut buf = vec![0u8; (W * H * 4) as usize];

    let set = |buf: &mut Vec<u8>, x: i32, y: i32, r: u8, g: u8, b: u8, a: u8| {
        if (0..W).contains(&x) && (0..H).contains(&y) {
            let i = ((y * W + x) * 4) as usize;
            buf[i] = r;
            buf[i + 1] = g;
            buf[i + 2] = b;
            buf[i + 3] = a;
        }
    };

    // 手柄：从 (26,6) 到 (9,23) 的粗斜线（棕色）。
    for t in 0..=28 {
        let x = 26 - t * 17 / 28;
        let y = 6 + t * 17 / 28;
        for dx in -2..=2 {
            for dy in -2..=2 {
                set(&mut buf, x + dx, y + dy, 139, 90, 43, 255);
            }
        }
    }
    // 刷毛（浅色）在斜线末端。
    for dx in -3..=3 {
        for dy in -3..=3 {
            if dx * dx + dy * dy <= 10 {
                set(&mut buf, 9 + dx, 23 + dy, 210, 180, 140, 255);
            }
        }
    }
    // 颜料点（蓝色）。
    for dx in -2..=2 {
        for dy in -2..=2 {
            if dx * dx + dy * dy <= 5 {
                set(&mut buf, 6 + dx, 26 + dy, 74, 144, 226, 255);
            }
        }
    }

    (W as u32, H as u32, buf)
}
