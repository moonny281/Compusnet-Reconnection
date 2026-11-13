//! Application entry: tray-only startup, monitor loop, status watcher.

pub mod commands;
pub mod error;
pub mod services;
pub mod state;
pub mod tray;
pub mod types;
pub mod windows;

use state::Monitor;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let log = Arc::new(services::logging::RingLog::new(500));

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ));

    let builder = commands::register(builder);

    builder
        .setup(move |app| {
            let handle = app.handle().clone();
            let cfg = Arc::new(
                services::config::ConfigStore::new().expect("failed to init config store"),
            );
            let monitor = Monitor::new(handle.clone(), cfg.clone(), log.clone())
                .expect("failed to init monitor");

            // Manage BEFORE anything (tray handlers / spawned tasks) can
            // call state::<Monitor>() — Tauri manages it internally as an Arc.
            app.manage(monitor);

            // Build the tray with its menu + click handlers. The trayIcon
            // entry in tauri.conf.json is intentionally absent: a config tray
            // would have no menu handlers wired to Rust.
            tray::setup_tray(&handle)?;

            // Spawn the monitor loop.
            {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let m = handle.state::<Monitor>();
                    m.run().await;
                });
            }

            // Tray reacts to state changes via the watch channel — no polling.
            {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let mut rx = handle.state::<Monitor>().subscribe();
                    let mut last: Option<(bool, types::AppState)> = None;
                    loop {
                        if rx.changed().await.is_err() {
                            return; // app shutting down
                        }
                        let (paused, st) = {
                            let s = rx.borrow();
                            (s.paused, s.state)
                        };
                        if last != Some((paused, st)) {
                            last = Some((paused, st));
                            let _ = tray::refresh_tray_menu(&handle, paused, st);
                            let _ = tray::set_tray_icon(&handle, st, paused);
                        }
                    }
                });
            }

            let _ = log.info("应用启动完成（托盘模式）"); // pre-window: no listener yet
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, _event| {});
}
