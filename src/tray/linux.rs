//! Linux 托盘（ksni / SNI）：刷子图标 + 固定四菜单。

use std::sync::atomic::Ordering;

use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::TrayMethods;

use super::{brush_icon_rgba, AutostartFlag, TrayCommand};
use crate::Result;

/// SNI 托盘对象。
pub struct SniTray {
    tx: tokio::sync::mpsc::Sender<TrayCommand>,
    autostart: AutostartFlag,
}

impl ksni::Tray for SniTray {
    fn id(&self) -> String {
        "course-organizer".into()
    }

    fn title(&self) -> String {
        "课件整理助手".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let (w, h, rgba) = brush_icon_rgba();
        // ksni 需要 ARGB32（网络字节序 = [A,R,G,B]），由 RGBA 转换而来。
        let (pixels, _) = rgba.as_chunks::<4>();
        let data: Vec<u8> = pixels
            .iter()
            .flat_map(|p| [p[3], p[0], p[1], p[2]])
            .collect();
        vec![ksni::Icon {
            width: w as i32,
            height: h as i32,
            data,
        }]
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let checked = self.autostart.load(Ordering::SeqCst);
        vec![
            CheckmarkItem {
                label: "设置开机自启".into(),
                checked,
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.tx.try_send(TrayCommand::ToggleAutostart);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "立刻整理".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.tx.try_send(TrayCommand::OrganizeNow);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "打开配置文件目录".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.tx.try_send(TrayCommand::OpenConfigDir);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "退出".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.tx.try_send(TrayCommand::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// 在 tokio 运行时内 spawn SNI 服务，返回句柄（Drop 前应 `shutdown`）。
pub async fn spawn(
    tx: tokio::sync::mpsc::Sender<TrayCommand>,
    autostart: AutostartFlag,
) -> Result<ksni::Handle<SniTray>> {
    let tray = SniTray { tx, autostart };
    tray.spawn()
        .await
        .map_err(|e| crate::Error::Msg(format!("托盘启动失败：{e}")))
}
