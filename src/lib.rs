//! desktop-tidier 库入口。
//!
//! 单进程纯 Rust 桌面课件整理后台工具。业务层统一返回 [`Result`]，
//! 禁止使用 `unwrap`/`expect`/`panic`。

use std::sync::Arc;

pub mod archiver;
pub mod classifier;
pub mod config;
pub mod encoding;
pub mod error;
pub mod indexer;
pub mod logger;
pub mod organizer;
pub mod platform;
pub mod scheduler;
pub mod state;
pub mod timesync;
pub mod tray;
pub mod watcher;

pub use error::{Error, Result};

/// 版本号（来自 Cargo.toml）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 早期失败兜底：日志/通知体系可能尚未就绪，把错误写进
/// `<config_dir>/logs/startup-error.log` 并尝试系统通知（隐藏控制台后无 stdout 可看）。
fn fail_early(
    platform: &Arc<dyn platform::PlatformAdapter>,
    config_dir: &std::path::Path,
    message: &str,
) -> anyhow::Error {
    let log_dir = config_dir.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let _ = std::fs::write(log_dir.join("startup-error.log"), format!("{message}\n"));
    let _ = platform.notify("desktop-tidier", message);
    anyhow::anyhow!("{message}")
}

/// 程序入口：单实例锁 → 配置/日志 → 运行时 + 各服务 → 托盘 → 退出。
pub fn run() -> anyhow::Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};

    let platform = platform::adapter();
    let config_dir = platform.default_config_dir();

    // 单实例锁（必须在构建托盘前完成）。
    let instance_guard = match platform.acquire_single_instance() {
        Ok(guard) => guard,
        Err(e) => {
            return Err(fail_early(
                &platform,
                &config_dir,
                &format!("启动失败：{e}"),
            ))
        }
    };

    // 配置目录与配置存储（不存在则生成示例）。
    let config_path = config_dir.join("config.json");
    let config_store = match config::ConfigStore::load(&config_path) {
        Ok(store) => Arc::new(store),
        Err(e) => {
            return Err(fail_early(
                &platform,
                &config_dir,
                &format!("配置加载失败：{e}"),
            ))
        }
    };

    // 单文件日志（依赖配置中的日志级别与路径；错误级日志同时触发系统通知）。
    logger::init(&config_store.get(), platform.clone(), config_store.clone())?;

    // 运行状态（上次整理/归档时间与台账水位，支撑启动追赶与归档阈值）。
    let state_path = {
        let cfg0 = config_store.get();
        std::path::Path::new(&cfg0.storage.archive_dir)
            .parent()
            .map(|p| p.join("state.json"))
            .unwrap_or_else(|| config_dir.join("state.json"))
    };
    let state = Arc::new(state::StateStore::load(&state_path));

    // 授时时钟（启动时从落盘偏移恢复）。
    let offset_path = config_dir.join("timesync.offset");
    let clock = timesync::SyncedClock::shared(timesync::Timesync::load_offset(&offset_path));
    let timesync = Arc::new(timesync::Timesync::new(clock.clone(), offset_path)?);

    // 后台编排器（整理 / 归档 / 授时 / 热重载）。
    let scheduler = Arc::new(scheduler::Scheduler::new(
        config_store.clone(),
        platform.clone(),
        clock.clone(),
        timesync,
        state,
    )?);

    // 手动建 runtime（不用 #[tokio::main]）。
    let rt = tokio::runtime::Runtime::new()?;

    // 托盘命令通道 + 优雅退出信号。
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<tray::TrayCommand>(16);

    // 自启勾选态（初始从 OS 读取，供托盘显示）。
    let autostart = Arc::new(AtomicBool::new(
        platform.is_autostart_enabled().unwrap_or(false),
    ));

    // 菜单命令消费任务：菜单 → 这里（tokio 侧）处理。
    {
        let platform = platform.clone();
        let scheduler = scheduler.clone();
        let config_dir = config_dir.clone();
        let autostart = autostart.clone();
        let shutdown_tx = shutdown_tx.clone();
        rt.spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    tray::TrayCommand::ToggleAutostart => {
                        let cur = platform.is_autostart_enabled().unwrap_or(false);
                        let next = !cur;
                        match platform.set_autostart(next) {
                            Ok(enabled) => {
                                autostart.store(enabled, Ordering::SeqCst);
                                tracing::info!(
                                    "开机自启：{}",
                                    if enabled { "已开启" } else { "已关闭" }
                                );
                            }
                            Err(e) => tracing::error!("设置开机自启失败：{e}"),
                        }
                    }
                    tray::TrayCommand::OrganizeNow => {
                        if let Err(e) = scheduler.organize_once().await {
                            tracing::error!("立刻整理失败：{e}");
                        }
                    }
                    tray::TrayCommand::OpenConfigDir => {
                        if let Err(e) = platform.open_in_file_manager(&config_dir) {
                            tracing::error!("打开配置文件目录失败：{e}");
                        }
                    }
                    tray::TrayCommand::Quit => {
                        let _ = shutdown_tx.send(true);
                        break;
                    }
                }
            }
        });
    }

    // 后台周期任务 + 文件监听（周期整理 / 周期归档 / 授时 / 热重载 / 启动追赶）。
    rt.spawn(scheduler.clone().run_loops(shutdown_rx.clone()));

    // 托盘：Linux 在运行时内 spawn SNI；Windows 阻塞 tray-icon 事件循环。
    #[cfg(target_os = "linux")]
    let tray_handle = {
        let tx = cmd_tx.clone();
        let autostart = autostart.clone();
        rt.block_on(tray::linux::spawn(tx, autostart))?
    };

    #[cfg(target_os = "windows")]
    {
        if let Err(e) = tray::windows::run(cmd_tx.clone(), autostart.clone()) {
            tracing::error!("托盘启动失败：{e}");
            return Err(e.into());
        }
    }

    // 等待退出信号（由「退出」菜单触发）。
    while !*shutdown_rx.borrow() {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // 移除托盘图标（Linux：关闭 SNI 服务）。
    #[cfg(target_os = "linux")]
    {
        rt.block_on(tray_handle.shutdown());
    }

    // 释放单实例锁、drop runtime（各任务被 graceful 终止）。
    drop(instance_guard);
    tracing::info!("desktop-tidier 已退出");
    Ok(())
}
