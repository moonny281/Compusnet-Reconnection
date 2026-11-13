//! Application error type.

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
