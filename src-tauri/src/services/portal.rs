//! Auth submission engine: renders the user-configured template
//! (login URL, method, fields, headers, success checks) and submits it.
//!
//! The `Reprobe` success check is NOT evaluated here — the caller probes
//! after submission (it owns the probe client). `submit` therefore returns
//! `NeedReprobe` when every other check passed and a Reprobe was requested.

use crate::error::{AppError, AppResult};
use crate::types::{BodyType, PortalProfile, SuccessCheck, TemplateField};
use std::collections::HashMap;
use std::time::Duration;

/// Values available for template placeholders.
pub struct TemplateVars {
    pub username: String,
    pub password: String,
    /// The portal URL the probe redirect pointed at (optional hint).
    pub redirect_url: Option<String>,
}

pub enum SubmitOutcome {
    /// All checks passed.
    Success,
    /// Other checks passed; caller must re-probe to confirm.
    NeedReprobe,
    /// Checks failed. Argument lists which ones and why.
    Failed(String),
}

/// Submit credentials per the template.
pub async fn submit(
    http: &reqwest::Client,
    profile: &PortalProfile,
    vars: &TemplateVars,
    timeout_secs: u64,
) -> AppResult<SubmitOutcome> {
    if profile.login_url.trim().is_empty() {
        return Err(AppError::Portal("登录页 URL 未配置".into()));
    }
    let timeout = Duration::from_secs(timeout_secs.max(1));

    let mut full_vars: HashMap<String, String> = HashMap::new();
    full_vars.insert("username".into(), vars.username.clone());
    full_vars.insert("password".into(), vars.password.clone());
    if let Some(u) = &vars.redirect_url {
        full_vars.insert("portal_url".into(), u.clone());
    }

    // Optional pre-step (e.g. GET a challenge endpoint first).
    if let Some(pre) = &profile.pre_step {
        if pre.url.trim().is_empty() {
            return Err(AppError::Portal("前置请求 URL 未配置".into()));
        }
        let pre_url = render_with_map(&pre.url, &full_vars);
        let resp = http
            .get(&pre_url)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| AppError::Portal(format!("前置请求失败: {e}")))?;
        let text = resp.text().await.unwrap_or_default();
        let extracted = match &pre.extract_regex {
            Some(pattern) => {
                let re = regex::Regex::new(pattern)
                    .map_err(|e| AppError::Portal(format!("前置提取正则错误: {e}")))?;
                re.captures(&text)
                    .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
                    .unwrap_or_default()
            }
            None => text,
        };
        full_vars.insert("step1_body".into(), extracted);
    }

    // Build the request.
    let rendered_url = render_with_map(&profile.login_url, &full_vars);
    let method = profile.method.to_uppercase();
    let mut req = match method.as_str() {
        "GET" => http.get(&rendered_url),
        "POST" => http.post(&rendered_url),
        "PUT" => http.put(&rendered_url),
        m => return Err(AppError::Portal(format!("不支持的请求方法: {m}"))),
    };
    req = req.timeout(timeout);

    let rendered_fields: Vec<TemplateField> = profile
        .fields
        .iter()
        .map(|f| TemplateField {
            name: render_with_map(&f.name, &full_vars),
            value: render_with_map(&f.value, &full_vars),
        })
        .collect();

    for h in &profile.extra_headers {
        req = req.header(
            render_with_map(&h.name, &full_vars),
            render_with_map(&h.value, &full_vars),
        );
    }

    // GET carries the fields in the query string — servers and portals
    // routinely drop a GET body, which would lose the credentials.
    // POST/PUT encode them as a request body per body_type.
    if method == "GET" {
        if !rendered_fields.is_empty() {
            let query: Vec<(&str, &str)> = rendered_fields
                .iter()
                .map(|f| (f.name.as_str(), f.value.as_str()))
                .collect();
            req = req.query(&query);
        }
    } else {
        match profile.body_type {
            BodyType::Form => {
                let form: Vec<(&str, &str)> = rendered_fields
                    .iter()
                    .map(|f| (f.name.as_str(), f.value.as_str()))
                    .collect();
                if !form.is_empty() {
                    req = req.form(&form);
                }
            }
            BodyType::Json => {
                let map: serde_json::Map<String, serde_json::Value> = rendered_fields
                    .iter()
                    .map(|f| (f.name.clone(), serde_json::Value::String(f.value.clone())))
                    .collect();
                req = req.json(&serde_json::Value::Object(map));
            }
        }
    }

    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Portal(format!("认证请求失败: {e}")))?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    // Bodies may echo the password back; keep a short prefix for diagnostics.
    let body_prefix: String = body.chars().take(200).collect();

    let checks: Vec<SuccessCheck> = if profile.success.is_empty() {
        // Default: any 2xx counts as success.
        vec![SuccessCheck::Status2xx]
    } else {
        profile.success.clone()
    };

    let mut failures: Vec<String> = Vec::new();
    let mut needs_reprobe = false;
    for check in &checks {
        let ok = match check {
            SuccessCheck::Status2xx => (200..300).contains(&status),
            SuccessCheck::Status { value } => status == *value,
            SuccessCheck::BodyContains { value } => body.contains(value),
            SuccessCheck::BodyNotContains { value } => !body.contains(value),
            SuccessCheck::BodyRegex { value } => regex::Regex::new(value)
                .map(|re| re.is_match(&body))
                .unwrap_or(false),
            SuccessCheck::Reprobe => {
                needs_reprobe = true;
                true // evaluated by the caller
            }
        };
        if !ok {
            failures.push(describe_check(check));
        }
    }

    if !failures.is_empty() {
        return Ok(SubmitOutcome::Failed(format!(
            "HTTP {status} 未满足: {}；响应片段: {body_prefix}",
            failures.join(", ")
        )));
    }
    if needs_reprobe {
        return Ok(SubmitOutcome::NeedReprobe);
    }
    Ok(SubmitOutcome::Success)
}

fn describe_check(c: &SuccessCheck) -> String {
    match c {
        SuccessCheck::Status2xx => "状态码为 2xx".into(),
        SuccessCheck::Status { value } => format!("状态码 == {value}"),
        SuccessCheck::BodyContains { value } => format!("响应包含 {value:?}"),
        SuccessCheck::BodyNotContains { value } => format!("响应不包含 {value:?}"),
        SuccessCheck::BodyRegex { value } => format!("响应匹配正则 {value:?}"),
        SuccessCheck::Reprobe => "重新探测成功".into(),
    }
}

/// Render `{placeholder}` occurrences against `map`.
/// Unknown placeholders are kept verbatim so users notice typos.
pub fn render_with_map(template: &str, map: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = template[i..].find('}') {
                let key = &template[i + 1..i + end];
                match map.get(key) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&template[i..i + end + 1]),
                }
                i += end + 1;
                continue;
            }
        }
        // Multi-byte safe copy of the single char starting at i.
        let ch = template[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_placeholders() {
        let mut m: HashMap<String, String> = HashMap::new();
        m.insert("username".into(), "u1".into());
        m.insert("password".into(), "p@ss{weird".into());
        m.insert("ip".into(), "10.1.2.3".into());
        assert_eq!(render_with_map("u={username}&ip={ip}", &m), "u=u1&ip=10.1.2.3");
        // Unknown placeholder kept verbatim.
        assert_eq!(render_with_map("{unknown}", &m), "{unknown}");
        assert_eq!(render_with_map("{password}", &m), "p@ss{weird");
        // Multi-byte characters survive.
        m.insert("x".into(), "校园".into());
        assert_eq!(render_with_map("网{x}络", &m), "网校园络");
    }
}
