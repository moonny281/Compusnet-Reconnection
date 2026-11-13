//! Tray icon + menu. The tray IS the app: no window exists at startup.

use crate::types::AppState;
use tauri::{
    AppHandle, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let tray = TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "probe_now" => {
                let m = app.state::<crate::state::Monitor>();
                m.request_probe_now();
            }
            "pause" => {
                let m = app.state::<crate::state::Monitor>();
                let paused = !m.is_paused();
                m.set_paused(paused);
            }
            "settings" => crate::windows::open_settings(app.clone()),
            "logs" => crate::windows::open_logs(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left click: open settings.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::windows::open_settings(tray.app_handle().clone());
            }
        })
        .build(app)?;

    let menu = build_menu(app, false, AppState::Online)?;
    tray.set_menu(Some(menu))?;
    Ok(())
}

fn build_menu(app: &AppHandle, paused: bool, state: AppState) -> tauri::Result<Menu<tauri::Wry>> {
    let label = match state {
        AppState::Online => "状态: 已联网",
        AppState::Offline => "状态: 未联网",
        AppState::Authing => "状态: 正在认证…",
        AppState::Backoff => "状态: 认证重试中…",
        AppState::GaveUp => "状态: 自动登录失败(已停止)",
        AppState::Paused => "状态: 已暂停",
    };
    let status = MenuItem::with_id(app, "status", label, false, None::<&str>)?;
    let probe_now = MenuItem::with_id(app, "probe_now", "立即检测", true, None::<&str>)?;
    let pause = MenuItem::with_id(
        app,
        "pause",
        if paused { "恢复监控" } else { "暂停监控" },
        true,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let logs = MenuItem::with_id(app, "logs", "日志…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &probe_now,
            &pause,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &logs,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}

/// Reflect current state in the menu text (rebuild the tiny menu).
pub fn refresh_tray_menu(app: &AppHandle, paused: bool, state: AppState) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id("main") {
        let menu = build_menu(app, paused, state)?;
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}

/// Swap the tray icon by state color.
/// Icons are embedded at compile time (include_image!) so the bare
/// release exe works as a portable single file — no resource dir needed.
pub fn set_tray_icon(app: &AppHandle, state: AppState, paused: bool) -> tauri::Result<()> {
    let icon = if paused {
        tauri::include_image!("icons/tray-grey.png")
    } else {
        match state {
            AppState::Online => tauri::include_image!("icons/tray-green.png"),
            AppState::Offline | AppState::GaveUp => tauri::include_image!("icons/tray-red.png"),
            AppState::Authing | AppState::Backoff => tauri::include_image!("icons/tray-yellow.png"),
            AppState::Paused => tauri::include_image!("icons/tray-grey.png"),
        }
    };
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_icon(Some(icon))?;
    }
    Ok(())
}
