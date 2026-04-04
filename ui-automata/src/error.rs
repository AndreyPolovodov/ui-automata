use thiserror::Error;

#[derive(Debug, Error)]
pub enum AutomataError {
    #[error("{0}")]
    Internal(String),
    #[error("{0}")]
    Platform(String),
    #[error("cancelled")]
    Cancelled,
}
