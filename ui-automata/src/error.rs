use thiserror::Error;

#[derive(Debug, Error)]
pub enum AutomataError {
    #[error("{0}")]
    Internal(String),
    #[error("{0}")]
    Platform(String),
    #[error("cancelled")]
    Cancelled,
    /// Condition evaluated to false with a diagnostic reason.
    /// Treated as Ok(false) by the poll loop but included in the timeout message.
    #[error("{0}")]
    ConditionFalse(String),
}
