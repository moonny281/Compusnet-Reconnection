//! App state machine + monitoring loop.
//!
//! Flow per PLAN.md §5.1: a scheduler triggers probes on an interval;
//! when the SSID is whitelisted and the probe says "need auth", the auth
//! template is submitted; failures back off exponentially and eventually
//! give up until the user resets or the network changes.

use crate::error::AppResult;
use crate::services::config::{load_password, ConfigStore};
use crate::services::logging::{mask_password, RingLog};
use crate::services::portal::{submit, SubmitOutcome, TemplateVars};
use crate::services::probe::probe_all;
use crate::services::ssid::SsidProvider;
use crate::types::{AppConfig, AppState, LogEntry, ProbeResult, StatusPayload};
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::{watch, Mutex as AsyncMutex};

/// Events emitted to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AppEvent {
    Status { payload: StatusPayload },
    Log { entry: LogEntry },
}

pub struct Monitor {
    app: AppHandle,
    cfg: Arc<ConfigStore>,
    log: Arc<RingLog>,
    ssid: Arc<SsidProvider>,
    http: reqwest::Client,
    /// Serializes probe cycles; a running cycle blocks the next tick.
    busy: AsyncMutex<()>,
    state: StateWatch,
}

// tokio's watch channel type alias to keep the struct readable.
type StateWatch = (watch::Sender<Inner>, watch::Receiver<Inner>);

#[derive(Debug, Clone)]
pub(crate) struct Inner {
    pub state: AppState,
    pub next_probe_at: Instant,
    pub fail_count: u32,
    pub paused: bool,
}

impl Monitor {
    pub fn new(app: AppHandle, cfg: Arc<ConfigStore>, log: Arc<RingLog>) -> AppResult<Self> {
        let http = reqwest::Client::builder()
            // Never follow redirects: a captive portal must surface as 30x.
            .redirect(reqwest::redirect::Policy::none())
            // Strict TLS by default; set CNR_INSECURE_TLS=1 to relax (advanced).
            .danger_accept_invalid_certs(std::env::var("CNR_INSECURE_TLS").is_ok())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| crate::error::AppError::Network(e.to_string()))?;

        let state: StateWatch = watch::channel(Inner {
            state: AppState::Online,
            next_probe_at: Instant::now(),
            fail_count: 0,
            paused: false,
        });

        Ok(Self {
            app,
            cfg,
            log,
            ssid: Arc::new(SsidProvider::new(Duration::from_secs(3))),
            http,
            busy: AsyncMutex::new(()),
            state,
        })
    }

    pub fn config(&self) -> Arc<ConfigStore> {
        self.cfg.clone()
    }

    pub fn log(&self) -> Arc<RingLog> {
        self.log.clone()
    }

    pub fn current_status(&self) -> StatusPayload {
        let s = self.state.1.borrow().clone();
        let cfg = self.cfg.get();
        let ssid = self.ssid.current_ssid();
        let ssid_whitelisted = ssid
            .as_ref()
            .map(|s| cfg.ssids.iter().any(|w| w.eq_ignore_ascii_case(s)))
            .unwrap_or(false);
        let next_in = if s.paused {
            0
        } else {
            s.next_probe_at
                .checked_duration_since(Instant::now())
                .map(|d| d.as_secs())
                .unwrap_or(0)
        };
        StatusPayload {
            state: s.state,
            ssid,
            ssid_whitelisted,
            next_probe_in_secs: next_in,
            fail_count: s.fail_count,
        }
    }

    pub fn set_paused(&self, paused: bool) {
        let _ = self.state.0.send_modify(|s| {
            s.paused = paused;
            if paused {
                s.state = AppState::Paused;
            } else {
                s.state = AppState::Online;
                s.next_probe_at = Instant::now(); // probe right away on resume
            }
        });
        self.log_info(if paused {
            "已暂停监控"
        } else {
            "已恢复监控，立即检测一次"
        });
        self.emit_status();
    }

    /// User pressed "probe now" / reset after GaveUp.
    pub fn request_probe_now(&self) {
        let _ = self.state.0.send_modify(|s| {
            s.next_probe_at = Instant::now();
            if s.state == AppState::GaveUp {
                s.state = AppState::Online;
                s.fail_count = 0;
            }
        });
        self.emit_status();
    }

    pub fn is_paused(&self) -> bool {
        self.state.1.borrow().paused
    }

    pub fn state(&self) -> AppState {
        self.state.1.borrow().state
    }

    /// Subscribe to state changes. Cheap watcher handle; used by the tray
    /// and the monitor loop to react without polling.
    pub fn subscribe(&self) -> watch::Receiver<Inner> {
        self.state.1.clone()
    }

    fn emit_status(&self) {
        let payload = self.current_status();
        let _ = self.app.emit("app://status", &AppEvent::Status { payload });
    }

    /// Log through the ring AND push the entry to open log windows.
    fn log_info(&self, msg: impl Into<String>) {
        self.log.info(msg);
        self.emit_last_log();
    }

    fn log_warn(&self, msg: impl Into<String>) {
        self.log.warn(msg);
        self.emit_last_log();
    }

    fn log_error(&self, msg: impl Into<String>) {
        self.log.error(msg);
        self.emit_last_log();
    }

    fn emit_last_log(&self) {
        if let Some(entry) = self.log.last() {
            let _ = self.app.emit("app://log", &AppEvent::Log { entry });
        }
    }

    /// Re-read config and reset the timer (called after settings save).
    pub fn reload_config(&self) {
        let cfg = self.cfg.get();
        let _ = self.state.0.send_modify(|s| {
            s.next_probe_at = Instant::now() + Duration::from_secs(cfg.interval_secs);
        });
        self.log_info(format!("配置已更新，检测间隔 {}s", cfg.interval_secs));
        self.emit_status();
    }

    /// Main loop: run one probe cycle when due. Spawned once at startup.
    /// Wakes on: next_probe_at due, pause/resume, probe-now — all through the
    /// watch channel, so there is no fixed 1s polling; the only periodic tick
    /// is a 1h safety net that re-reads paused state.
    pub async fn run(&self) {
        let mut rx = self.state.1.clone();
        rx.mark_changed(); // ensure the loop evaluates immediately at startup
        loop {
            let (paused, due) = {
                let s = rx.borrow().clone();
                (s.paused, s.next_probe_at)
            };

            // Sleep until due (or a long safety tick when paused), but wake
            // early on any state change (pause/resume/probe-now/config).
            let sleep_for = if paused {
                Duration::from_secs(3600)
            } else {
                due.checked_duration_since(Instant::now())
                    .unwrap_or(Duration::ZERO)
            };

            tokio::select! {
                _ = tokio::time::sleep(sleep_for) => {}
                changed = rx.changed() => {
                    if changed.is_err() {
                        return; // sender dropped: app shutting down
                    }
                    // Recompute on the next iteration; if not yet due we just
                    // re-sleep with the new deadline.
                    continue;
                }
            }

            if paused {
                continue;
            }

            // Hold the guard for the whole cycle so concurrent triggers
            // (future call sites) skip while one cycle is running.
            let Ok(_guard) = self.busy.try_lock() else {
                self.schedule_next();
                continue;
            };

            self.cycle().await;
            self.schedule_next();
            self.emit_status();
            drop(_guard);
        }
    }

    fn schedule_next(&self) {
        let interval = self.cfg.get().interval_secs.max(5);
        let _ = self.state.0.send_modify(|s| {
            s.next_probe_at = Instant::now() + Duration::from_secs(interval);
        });
    }

    /// One full cycle: probe → maybe auth → report.
    async fn cycle(&self) {
        let cfg = self.cfg.get();
        let ssid = self.ssid.current_ssid();
        let whitelisted = ssid
            .as_ref()
            .map(|s| cfg.ssids.iter().any(|w| w.eq_ignore_ascii_case(s)))
            .unwrap_or(false);
        let gave_up = self.state() == AppState::GaveUp;

        let result = probe_all(&self.http, &cfg.probe_endpoints, cfg.probe_timeout_secs).await;

        if !whitelisted {
            // Not our campus network: record connectivity, never auth.
            // Leaving the campus SSID also clears GaveUp — fresh network,
            // fresh retry budget.
            let new_state = match result {
                ProbeResult::Online => AppState::Online,
                _ => AppState::Offline,
            };
            self.set_state(new_state, 0);
            return;
        }

        match result {
            ProbeResult::Online => {
                self.set_state(AppState::Online, 0);
            }
            ProbeResult::NeedAuth { redirect_url } => {
                if gave_up {
                    // Retry budget exhausted earlier: do NOT hammer the portal
                    // every interval. Stay GaveUp until the user probes now,
                    // or the network changes (handled above).
                    return;
                }
                self.log_info(format!(
                    "检测到需要认证 (SSID: {}) redirect={:?}",
                    ssid.as_deref().unwrap_or("?"),
                    redirect_url
                ));
                self.auth(&cfg, redirect_url).await;
            }
            ProbeResult::Offline => {
                self.log_warn(format!(
                    "探测超时/失败 (SSID: {})",
                    ssid.as_deref().unwrap_or("?")
                ));
                self.set_state(AppState::Offline, 0);
            }
        }
    }

    fn set_state(&self, state: AppState, fail_count: u32) {
        let _ = self.state.0.send_modify(|s| {
            if s.paused {
                // A submit already in flight when the user paused must not
                // un-grey the tray; resume probes immediately anyway.
                return;
            }
            s.state = state;
            s.fail_count = fail_count;
        });
        self.emit_status();
    }

    /// Sleep that returns early. `true` means the user paused — callers
    /// should abort the current chain; `false` means the wait elapsed (or
    /// some other state change happened, e.g. config reload — proceed).
    async fn sleep_interruptible(&self, dur: Duration) -> bool {
        let mut rx = self.subscribe();
        tokio::select! {
            _ = tokio::time::sleep(dur) => false,
            changed = rx.changed() => changed.is_ok() && self.is_paused(),
        }
    }

    /// Submit credentials with retry/backoff (runs within one cycle).
    async fn auth(&self, cfg: &AppConfig, redirect_url: Option<String>) {
        self.set_state(AppState::Authing, 0);

        // Config errors must not consume the retry budget: fail fast instead
        // of backing off through every attempt against a nonexistent target.
        if cfg.portal.login_url.trim().is_empty() {
            self.log_error("认证页 URL 未配置，请在设置中填写");
            self.set_state(AppState::GaveUp, 0);
            self.notify("自动登录失败", "认证页 URL 未配置，请打开设置填写");
            return;
        }

        let password = match load_password(&cfg.username) {
            Ok(Some(p)) if !p.is_empty() => p,
            _ => {
                self.log_error("凭据管理器中没有密码，请先在设置中保存密码");
                self.set_state(AppState::GaveUp, 0);
                self.notify("自动登录失败", "未找到密码，请打开设置填写");
                return;
            }
        };

        let vars = TemplateVars {
            username: cfg.username.clone(),
            password: password.clone(),
            redirect_url,
        };

        let profile = cfg.portal.clone();
        let mut fail_count = 0u32;
        let max = cfg.retry.max_attempts.max(1);
        let cap = cfg.retry.backoff_cap_secs.max(1);
        // Never start above the cap, even if base > cap was misconfigured.
        let mut backoff = cfg.retry.backoff_base_secs.max(1).min(cap);

        loop {
            // Covers the pause window before the first submit too (the probe
            // the cycle started with can't be aborted, but this gate stops it
            // from turning into a login).
            if self.is_paused() {
                self.log_info("监控已暂停，中止本次自动登录");
                return;
            }
            let outcome = submit(&self.http, &profile, &vars, cfg.probe_timeout_secs).await;

            let result = match outcome {
                Ok(SubmitOutcome::Success) => Ok(()),
                Ok(SubmitOutcome::NeedReprobe) => {
                    // Confirm via a fresh probe of the configured endpoints.
                    let probe =
                        probe_all(&self.http, &cfg.probe_endpoints, cfg.probe_timeout_secs).await;
                    if probe == ProbeResult::Online {
                        Ok(())
                    } else {
                        Err("提交后重新探测仍需认证".to_string())
                    }
                }
                Ok(SubmitOutcome::Failed(reason)) => Err(reason),
                Err(e) => Err(mask_password(&e.to_string(), &password)),
            };

            match result {
                Ok(()) => {
                    self.log_info(format!("认证成功 (用户: {})", mask_user(&cfg.username)));
                    self.set_state(AppState::Online, 0);
                    self.notify("校园网", "自动登录成功");
                    return;
                }
                Err(reason) => {
                    // Portal error pages may echo the submitted password —
                    // mask before the reason reaches the log.
                    let reason = mask_password(&reason, &password);
                    fail_count += 1;
                    self.log_error(format!("认证尝试 {fail_count}/{max} 失败: {reason}"));
                    if fail_count >= max {
                        self.set_state(AppState::GaveUp, fail_count);
                        self.notify("校园网自动登录失败", "已达到最大重试次数，请手动检查");
                        return;
                    }
                    self.set_state(AppState::Backoff, fail_count);
                    self.log_info(format!("{backoff}s 后重试"));
                    // Interruptible backoff: "pause" aborts the retry chain
                    // instead of letting it hammer the portal in the background.
                    if self.sleep_interruptible(Duration::from_secs(backoff)).await {
                        self.log_info("监控已暂停，中止本次自动登录");
                        return;
                    }
                    backoff = (backoff * 2).min(cap);
                }
            }
        }
    }

    fn notify(&self, title: &str, body: &str) {
        if !self.cfg.get().notify {
            return;
        }
        let app = self.app.clone();
        let title = title.to_string();
        let body = body.to_string();
        tauri::async_runtime::spawn(async move {
            let _ = app.notification().builder().title(title).body(body).show();
        });
    }
}

fn mask_user(u: &str) -> String {
    let chars: Vec<char> = u.chars().collect();
    if chars.len() <= 2 {
        "*".repeat(chars.len())
    } else {
        format!("{}***{}", chars[0], chars[chars.len() - 1])
    }
}
