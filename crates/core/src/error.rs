use thiserror::Error;

#[derive(Error, Debug)]
pub enum TrendRadarError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("AI error: {0}")]
    Ai(String),

    #[error("Notification error: {0}")]
    Notification(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, TrendRadarError>;
