//! Windows 托盘（tray-icon + muda）：刷子图标 + 固定四菜单，阻塞事件循环。

use std::sync::atomic::Ordering;
use std::time::Duration;

use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};

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

    let (w, h, rgba) = brush_icon_rgba();
    let icon =
        Icon::from_rgba(rgba, w, h).map_err(|e| Error::Msg(format!("托盘图标数据无效：{e}")))?;

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("课件整理助手")
        .build()
        .map_err(|e| Error::Msg(format!("构建托盘图标失败：{e}")))?;

    let autostart_id = autostart_item.id().clone();
    let organize_id = organize_item.id().clone();
    let config_id = config_item.id().clone();
    let quit_id = quit_item.id().clone();

    let mut last_checked = autostart.load(Ordering::SeqCst);
    loop {
        // 同步自启勾选态（由 tokio 侧在 set_autostart 后更新）。
        let cur = autostart.load(Ordering::SeqCst);
        if cur != last_checked {
            autostart_item.set_checked(cur);
            last_checked = cur;
        }

        if let Ok(event) = MenuEvent::receiver().recv_timeout(Duration::from_millis(200)) {
            let id = event.id();
            if id == &autostart_id {
                let _ = tx.blocking_send(TrayCommand::ToggleAutostart);
            } else if id == &organize_id {
                let _ = tx.blocking_send(TrayCommand::OrganizeNow);
            } else if id == &config_id {
                let _ = tx.blocking_send(TrayCommand::OpenConfigDir);
            } else if id == &quit_id {
                let _ = tx.blocking_send(TrayCommand::Quit);
                break;
            }
        }
    }

    drop(tray);
    Ok(())
}
