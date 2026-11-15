//! Windows 托盘（tray-icon + muda）：刷子图标 + 固定四菜单，自带 Win32 消息泵。
//!
//! tray-icon 0.26 在 Windows 上把「点击 → TrackPopupMenu 弹菜单」放在隐藏窗口的
//! WndProc 里（见 vendored `src/platform_impl/windows/mod.rs` 的 WM_USER_TRAYICON 分支），
//! 图标经 NIM_ADD 同步注册即可见，但**只有所属线程泵送 Win32 消息时 WndProc 才会被调用**。
//! 本项目无 GUI 窗口（无 tao/winit），因此本循环必须自己用
//! `MsgWaitForMultipleObjects + PeekMessageW/DispatchMessageW` 泵消息，否则图标可见、
//! 左右键却永远不弹菜单。

use std::sync::atomic::Ordering;

use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    QS_ALLINPUT, WM_QUIT,
};

use super::{brush_icon_rgba, AutostartFlag, TrayCommand};
use crate::{Error, Result};

/// 阻塞运行 tray-icon 事件循环，直到「退出」菜单被点击。
pub fn run(tx: tokio::sync::mpsc::Sender<TrayCommand>, autostart: AutostartFlag) -> Result<()> {
    let menu = Menu::new();
    let autostart_item =
        CheckMenuItem::new("设置开机自启", true, autostart.load(Ordering::SeqCst), None);
    let organize_item = MenuItem::new("立刻整理", true, None);
    let config_item = MenuItem::new("打开配置文件目录", true, None);
    let quit_item = MenuItem::new("退出", true, None);
    menu.append_items(&[&autostart_item, &organize_item, &config_item, &quit_item])
        .map_err(|e| Error::Msg(format!("构建托盘菜单失败：{e}")))?;

    let autostart_id = autostart_item.id().clone();
    let organize_id = organize_item.id().clone();
    let config_id = config_item.id().clone();
    let quit_id = quit_item.id().clone();

    // 事件接收器在 build 前取一次（muda 0.21 是全局惰性通道，取用无副作用）。
    let menu_rx = MenuEvent::receiver();
    let tray_events = TrayIconEvent::receiver();

    // 建图标（显式化左右键弹菜单行为，与 tray-icon 默认值一致）。
    let (w, h, rgba) = brush_icon_rgba();
    let icon =
        Icon::from_rgba(rgba, w, h).map_err(|e| Error::Msg(format!("托盘图标数据无效：{e}")))?;

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("课件整理助手")
        .with_menu_on_right_click(true)
        .with_menu_on_left_click(true)
        .build()
        .map_err(|e| Error::Msg(format!("构建托盘图标失败：{e}")))?;

    let mut last_checked = autostart.load(Ordering::SeqCst);
    let mut msg = MSG::default();
    'outer: loop {
        // 非阻塞排空菜单点击事件。
        while let Ok(event) = menu_rx.try_recv() {
            let id = event.id();
            if id == &autostart_id {
                let _ = tx.blocking_send(TrayCommand::ToggleAutostart);
            } else if id == &organize_id {
                let _ = tx.blocking_send(TrayCommand::OrganizeNow);
            } else if id == &config_id {
                let _ = tx.blocking_send(TrayCommand::OpenConfigDir);
            } else if id == &quit_id {
                let _ = tx.blocking_send(TrayCommand::Quit);
                break 'outer;
            }
        }

        // 同步自启勾选态（由 tokio 侧在 set_autostart 后更新）。
        let cur = autostart.load(Ordering::SeqCst);
        if cur != last_checked {
            autostart_item.set_checked(cur);
            last_checked = cur;
        }

        // 排空托盘点击事件（本项目不消费，但必须清空防止无界增长）。
        while tray_events.try_recv().is_ok() {}

        // 关键：泵送 Win32 消息。等待消息到达或 200ms 超时（超时用于勾选态轮询；
        // 消息到达立即唤醒，无忙等）。
        unsafe {
            let _ = MsgWaitForMultipleObjects(None, false, 200, QS_ALLINPUT);
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).0 != 0 {
                if msg.message == WM_QUIT {
                    break 'outer;
                }
                TranslateMessage(&msg);
                // DispatchMessageW 可能进入 TrackPopupMenu 的阻塞模态循环
                // （正是菜单弹出的路径），属预期行为。
                DispatchMessageW(&msg);
            }
        }
    }

    // 仍在创建线程上 drop：Drop 内 DestroyWindow 必须由窗口属主线程调用。
    drop(tray);
    Ok(())
}
