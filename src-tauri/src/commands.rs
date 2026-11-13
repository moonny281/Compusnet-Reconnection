//! `#[tauri::command]`s exposed to the settings/logs windows.
//! All business logic stays in Rust; the frontend is a pure view.

use crate::error::AppResult;
use crate::services::config::{delete_password, load_password, save_password};
use crate::state::Monitor;
use crate::types::{AppConfig, StatusPayload};
use tauri::{AppHandle, State, Wry};
use tauri_plugin_notification::NotificationExt;

#[tauri::command]
pub fn get_config(monitor: State<'_, Monitor>) -> AppResult<AppConfig> {
    Ok(monitor.config().get())
}

#[tauri::command]
pub fn save_config(
    app: AppHandle,
    monitor: State<'_, Monitor>,
    config: AppConfig,
    password: Option<String>,
    autostart: Option<bool>,
) -> AppResult<()> {
    if let Some(p) = &password {
        if !p.is_empty() {
            save_password(&config.username, p)?;
        }
    }
    // If the username changed, drop the old account's credential to avoid
    // orphaned entries with stale passwords.
    let old = monitor.config().get();
    if !old.username.is_empty() && old.username != config.username {
        let _ = delete_password(&old.username);
    }
    monitor.config().update(|c| *c = config.clone())?;
    monitor.reload_config();
    // Apply autostart when the UI sent an explicit value.
    if let Some(want) = autostart {
        let auto = app.autolaunch();
        let enabled = auto.is_enabled().unwrap_or(false);
        if want != enabled {
            let _ = if want { auto.enable() } else { auto.disable() };
        }
    }
    Ok(())
}

/// Send a test notification (lets the user verify notify works).
#[tauri::command]
pub fn test_notification(app: AppHandle) -> AppResult<()> {
    app.notification()
        .builder()
        .title("校园网重连工具")
        .body("这是一条测试通知")
        .show()
        .map_err(|e| crate::error::AppError::Tauri(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn get_status(monitor: State<'_, Monitor>) -> StatusPayload {
    monitor.current_status()
}

#[tauri::command]
pub fn get_logs(monitor: State<'_, Monitor>) -> Vec<crate::types::LogEntry> {
    monitor.log().snapshot()
}

#[tauri::command]
pub fn clear_logs(monitor: State<'_, Monitor>) {
    monitor.log().clear();
}

#[tauri::command]
pub fn set_paused(monitor: State<'_, Monitor>, paused: bool) {
    monitor.set_paused(paused);
}

#[tauri::command]
pub fn probe_now(monitor: State<'_, Monitor>) {
    monitor.request_probe_now();
}

/// Has the user saved a password for this account? (Never returns it.)
#[tauri::command]
pub fn has_password(monitor: State<'_, Monitor>, username: String) -> AppResult<bool> {
    let cfg_user = monitor.config().get().username;
    let user = if username.is_empty() {
        cfg_user
    } else {
        username
    };
    if user.is_empty() {
        return Ok(false); // first run: no account configured yet
    }
    Ok(load_password(&user)?.is_some())
}

/// Open the logs window (tray menu + settings "打开日志" button).
#[tauri::command]
pub fn open_logs_window(app: AppHandle) {
    crate::windows::open_logs(app);
}

/// Register all commands.
pub fn register(builder: tauri::Builder<Wry>) -> tauri::Builder<Wry> {
    builder.invoke_handler(tauri::generate_handler![
        get_config,
        save_config,
        get_status,
        get_logs,
        clear_logs,
        set_paused,
        probe_now,
        has_password,
        test_notification,
        open_logs_window,
    ])
}
