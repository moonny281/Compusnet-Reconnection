//! Lazy window creation. No window exists at startup (tauri.conf.json
//! `app.windows: []`); closing a window DESTROYS it so WebView2 memory is
//! freed. Config lives in Rust state, so recreation is cheap.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub fn open_settings(app: AppHandle) {
    open_window(
        &app,
        "settings",
        "设置 - CampusNet Reconnector",
        "index.html",
        720.0,
        640.0,
    );
}

pub fn open_logs(app: AppHandle) {
    open_window(
        &app,
        "logs",
        "日志 - CampusNet Reconnector",
        "logs.html",
        640.0,
        480.0,
    );
}

fn open_window(app: &AppHandle, label: &str, title: &str, path: &str, w: f64, h: f64) {
    // If it already exists (user left it open), just focus it.
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }
    let builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App(path.into()))
        .title(title)
        .inner_size(w, h)
        .resizable(true)
        // Destroy on close (not hide) to free WebView2 memory.
        .closable(true);
    if let Err(e) = builder.build() {
        // Window may have been created concurrently; that error class is benign.
        eprintln!("open window {label}: {e}");
    }
}
