//! Connectivity probing: generate_204 style endpoints to tell
//! Online / NeedAuth(captive portal redirect) / Offline apart.

use crate::types::ProbeResult;
use std::time::Duration;

/// Probe one endpoint. No redirects are followed so a captive portal
/// shows up as a 30x with a Location header instead of the portal page.
pub async fn probe_one(client: &reqwest::Client, endpoint: &str, timeout: Duration) -> ProbeResult {
    let resp = match client.get(endpoint).timeout(timeout).send().await {
        Ok(r) => r,
        Err(_) => return ProbeResult::Offline,
    };

    match resp.status().as_u16() {
        204 => ProbeResult::Online,
        // msftconnecttest style: 200 with an expected body. A captive portal
        // hijacking this URL also returns 200 — only the expected content
        // counts as Online; anything else is treated as a portal page.
        200 => {
            let body = resp.text().await.unwrap_or_default();
            if body.contains("Microsoft Connect Test") {
                ProbeResult::Online
            } else {
                ProbeResult::NeedAuth { redirect_url: None }
            }
        }
        301 | 302 | 303 | 307 | 308 => {
            // Resolve relative Location values against the request URL so
            // {portal_url} is always a usable absolute hint.
            let redirect_url = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|loc| resp.url().join(loc).ok())
                .map(|u| u.to_string());
            ProbeResult::NeedAuth { redirect_url }
        }
        _ => ProbeResult::NeedAuth { redirect_url: None },
    }
}

/// Probe all configured endpoints concurrently and return:
/// - Online if ANY endpoint is Online
/// - otherwise NeedAuth (with one resolved redirect URL) if ANY said NeedAuth
/// - otherwise Offline
///
/// Concurrent on purpose: these are public reachability endpoints, not the
/// auth portal, so parallel hits are harmless — and it bounds cycle latency
/// by one timeout instead of N (blocked endpoints are common on campus nets).
pub async fn probe_all(
    client: &reqwest::Client,
    endpoints: &[String],
    timeout_secs: u64,
) -> ProbeResult {
    if endpoints.is_empty() {
        return ProbeResult::Offline;
    }
    let timeout = Duration::from_secs(timeout_secs.max(1));

    let mut set = tokio::task::JoinSet::new();
    for ep in endpoints {
        let client = client.clone();
        let ep = ep.clone();
        set.spawn(async move { probe_one(&client, &ep, timeout).await });
    }

    let mut first_need_auth: Option<ProbeResult> = None;
    while let Some(joined) = set.join_next().await {
        match joined.unwrap_or(ProbeResult::Offline) {
            // Returning drops the set, cancelling the remaining probes.
            ProbeResult::Online => return ProbeResult::Online,
            r @ ProbeResult::NeedAuth { .. } => {
                if first_need_auth.is_none() {
                    first_need_auth = Some(r);
                }
            }
            ProbeResult::Offline => {}
        }
    }

    first_need_auth.unwrap_or(ProbeResult::Offline)
}
