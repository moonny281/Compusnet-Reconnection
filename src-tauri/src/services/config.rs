//! Config persistence + Windows Credential Manager for the password.

use crate::error::{AppError, AppResult};
use crate::types::AppConfig;
use std::path::PathBuf;
use std::sync::RwLock;

const SERVICE: &str = "campus-net-reconnector";

pub struct ConfigStore {
    path: PathBuf,
    config: RwLock<AppConfig>,
}

impl ConfigStore {
    pub fn new() -> AppResult<Self> {
        let dir = app_config_dir()?;
        std::fs::create_dir_all(&dir).map_err(AppError::Io)?;
        let path = dir.join("config.json");
        let config = if path.exists() {
            let text = std::fs::read_to_string(&path).map_err(AppError::Io)?;
            match serde_json::from_str::<AppConfig>(&text) {
                Ok(c) => c,
                // Corrupt file: fall back to defaults but keep it for inspection.
                Err(e) => {
                    let _ = std::fs::rename(&path, path.with_extension("json.bak"));
                    eprintln!("config.json unreadable ({e}), using defaults");
                    AppConfig::default()
                }
            }
        } else {
            AppConfig::default()
        };
        Ok(Self {
            path,
            config: RwLock::new(config),
        })
    }

    pub fn get(&self) -> AppConfig {
        self.config.read().unwrap().clone()
    }

    pub fn update<F: FnOnce(&mut AppConfig)>(&self, f: F) -> AppResult<AppConfig> {
        let mut cfg = self.config.write().unwrap();
        f(&mut cfg);
        let json = serde_json::to_string_pretty(&*cfg)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(AppError::Io)?;
        }
        std::fs::write(&self.path, json).map_err(AppError::Io)?;
        Ok(cfg.clone())
    }
}

/// `%APPDATA%\<app>` — plain env read; no extra crate needed on Windows.
fn app_config_dir() -> AppResult<PathBuf> {
    let appdata = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| AppError::Config("APPDATA env not found".into()))?;
    Ok(appdata.join("campus-net-reconnector"))
}

// ---------------------------------------------------------------------------
// Credential manager (password), via the `keyring` crate.
// Stored per-username so switching accounts never leaves a stale credential
// under a shared key.
// ---------------------------------------------------------------------------

fn entry(username: &str) -> AppResult<keyring::Entry> {
    if username.is_empty() {
        return Err(AppError::Keyring("username is empty".into()));
    }
    keyring::Entry::new(SERVICE, username).map_err(|e| AppError::Keyring(e.to_string()))
}

pub fn save_password(username: &str, password: &str) -> AppResult<()> {
    entry(username)?
        .set_password(password)
        .map_err(|e| AppError::Keyring(e.to_string()))
}

pub fn load_password(username: &str) -> AppResult<Option<String>> {
    match entry(username)?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    }
}

pub fn delete_password(username: &str) -> AppResult<()> {
    let e = entry(username)?;
    match e.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(AppError::Keyring(err.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let tmp = std::env::temp_dir().join("cnr-test-config.json");
        let mut cfg = AppConfig::default();
        cfg.ssids = vec!["CampusNet".into()];
        cfg.interval_secs = 30;
        let json = serde_json::to_string(&cfg).unwrap();
        std::fs::write(&tmp, json).unwrap();
        let text = std::fs::read_to_string(&tmp).unwrap();
        let back: AppConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(back.ssids, cfg.ssids);
        assert_eq!(back.interval_secs, 30);
        std::fs::remove_file(&tmp).ok();
    }
}
