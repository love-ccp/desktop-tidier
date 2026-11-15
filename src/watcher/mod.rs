//! 文件监听（notify + 防抖）：配置/词库热重载 + 源目录变化触发整理。

use std::path::PathBuf;
use std::time::Duration;

use notify_debouncer_full::new_debouncer;
use notify_debouncer_full::notify::RecursiveMode;

use crate::Result;

/// 启动一个防抖监听线程，把「防抖后的事件路径」转发到 tokio 通道。
///
/// 内部线程持有 debouncer（保持存活）并阻塞接收；进程退出时随主线程终止。
/// 当接收端（tokio 通道）关闭时线程自行结束。
pub fn spawn_debounced_watcher(
    paths: Vec<(PathBuf, RecursiveMode)>,
    debounce: Duration,
    tx: tokio::sync::mpsc::Sender<Vec<PathBuf>>,
) -> Result<()> {
    let (raw_tx, raw_rx) = std::sync::mpsc::channel();
    let mut debouncer = new_debouncer(debounce, None, raw_tx)
        .map_err(|e| crate::Error::Msg(format!("创建文件监听器失败：{e}")))?;
    for (path, mode) in &paths {
        debouncer
            .watch(path, *mode)
            .map_err(|e| crate::Error::Msg(format!("监听 {path:?} 失败：{e}")))?;
    }
    std::thread::spawn(move || {
        let _keep_alive = debouncer;
        for result in raw_rx {
            let Ok(events) = result else { continue };
            let mut changed = Vec::new();
            for event in events {
                changed.extend(event.event.paths);
            }
            if !changed.is_empty() && tx.blocking_send(changed).is_err() {
                break;
            }
        }
    });
    Ok(())
}
