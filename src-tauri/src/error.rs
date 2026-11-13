//! Application error type.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("config: {0}")]
    Config(String),

    #[error("keyring: {0}")]
    Keyring(String),

    #[error("portal: {0}")]
    Portal(String),

    #[error("network: {0}")]
    Network(String),

    #[error("ssid: {0}")]
    Ssid(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("tauri: {0}")]
    Tauri(String),
}

pub type AppResult<T> = Result<T, AppError>;

// `AppResult<T>` is returned from `#[tauri::command]`s; Tauri's IPC plumbing
// requires the error side to be serializable (Into<InvokeError>).
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for AppError {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(AppError::Tauri(s))
    }
}
