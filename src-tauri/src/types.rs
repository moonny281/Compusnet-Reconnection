//! Shared types: configuration, probe results, app status, log entries.

use serde::{Deserialize, Serialize};

/// Result of one connectivity probe.
#[derive(Debug, Clone, PartialEq)]
pub enum ProbeResult {
    /// Reached the internet (204 or expected content).
    Online,
    /// Redirected somewhere else — likely a captive portal.
    /// `redirect_url` is the `Location` header when available.
    NeedAuth { redirect_url: Option<String> },
    /// Probe timed out / connection failed.
    Offline,
}

/// High-level state surfaced to the tray and the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppState {
    /// Monitoring; last probe was online.
    Online,
    /// Monitoring; last probe says network is down.
    Offline,
    /// Currently submitting credentials to the auth page.
    Authing,
    /// Retrying with backoff after failed auth attempts.
    Backoff,
    /// Consecutive failures exceeded the limit; auto-auth paused until reset.
    GaveUp,
    /// User paused the monitor.
    Paused,
}

/// How the request body is encoded when submitting to the auth page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyType {
    #[default]
    Form,
    Json,
}

/// One success criterion for the auth submission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SuccessCheck {
    /// HTTP status is any 2xx.
    Status2xx,
    /// HTTP status equals this value.
    Status { value: u16 },
    /// Response body contains this string (case-sensitive).
    BodyContains { value: String },
    /// Response body does NOT contain this string.
    BodyNotContains { value: String },
    /// Response body matches this regex.
    BodyRegex { value: String },
    /// After the request, run a full probe again and require Online.
    Reprobe,
}

/// The user-specified auth page submission template.
/// The login URL is user-configured; we only POST/GET a rendered template to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortalProfile {
    /// Auth page URL, required. e.g. `http://10.10.10.10:8080/login`
    pub login_url: String,
    /// HTTP method for the submission. Default `POST`.
    #[serde(default = "default_method")]
    pub method: String,
    /// Body/field template. Placeholders: `{username}` `{password}`
    /// `{portal_url}` `{step1_body}`.
    #[serde(default)]
    pub fields: Vec<TemplateField>,
    /// Extra headers appended to the request.
    #[serde(default)]
    pub extra_headers: Vec<TemplateField>,
    /// Body encoding. Default form.
    #[serde(default)]
    pub body_type: BodyType,
    /// Success criteria; ALL must pass (empty list = status 2xx).
    #[serde(default)]
    pub success: Vec<SuccessCheck>,
    /// Optional request issued before the login request (e.g. fetch a token);
    /// its response text can be referenced by `{step1_body}` in fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_step: Option<PreStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateField {
    pub name: String,
    pub value: String,
}

/// A preparatory GET before the actual login submission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreStep {
    pub url: String,
    /// Regex with one capture group whose match replaces `{step1_body}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract_regex: Option<String>,
}

fn default_method() -> String {
    "POST".into()
}

impl Default for PortalProfile {
    fn default() -> Self {
        // The most common campus auth form: POST username/password as a form.
        PortalProfile {
            login_url: String::new(),
            method: default_method(),
            fields: vec![
                TemplateField {
                    name: "username".into(),
                    value: "{username}".into(),
                },
                TemplateField {
                    name: "password".into(),
                    value: "{password}".into(),
                },
            ],
            extra_headers: Vec::new(),
            body_type: BodyType::Form,
            success: vec![SuccessCheck::Reprobe],
            pre_step: None,
        }
    }
}

/// Retry policy after a failed auth submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_base_secs: u64,
    pub backoff_cap_secs: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 5,
            backoff_base_secs: 10,
            backoff_cap_secs: 300,
        }
    }
}

/// Application configuration, persisted as `%APPDATA%/<app>/config.json`.
/// The password is NOT here — it lives in the Windows Credential Manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Seconds between probes.
    pub interval_secs: u64,
    /// Per-probe HTTP timeout in seconds.
    pub probe_timeout_secs: u64,
    /// generate_204 style endpoints; any success counts as online.
    pub probe_endpoints: Vec<String>,
    /// Campus SSID whitelist. Empty = any network triggers auth (not recommended).
    pub ssids: Vec<String>,
    /// Auth page submission template.
    pub portal: PortalProfile,
    /// Account name (also the keyring account key).
    pub username: String,
    pub retry: RetryPolicy,
    /// Show bubble notifications for auth success/failure.
    pub notify: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            interval_secs: 60,
            probe_timeout_secs: 5,
            probe_endpoints: vec![
                "http://connect.rom.miui.com/generate_204".into(),
                "http://www.msftconnecttest.com/connecttest.txt".into(),
            ],
            ssids: Vec::new(),
            portal: PortalProfile::default(),
            username: String::new(),
            retry: RetryPolicy::default(),
            notify: true,
        }
    }
}

/// One entry in the ring log shown in the log window.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    /// Milliseconds since UNIX epoch.
    pub ts: u64,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// Status payload pushed to the frontend and used by the tray.
#[derive(Debug, Clone, Serialize)]
pub struct StatusPayload {
    pub state: AppState,
    /// Current SSID, if a WLAN connection exists.
    pub ssid: Option<String>,
    /// SSID whitelist matches the current connection?
    pub ssid_whitelisted: bool,
    /// Next probe in N seconds (0 when authing / paused).
    pub next_probe_in_secs: u64,
    /// Consecutive auth failures since the last success.
    pub fail_count: u32,
}
