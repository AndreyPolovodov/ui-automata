use schemars::JsonSchema;
use serde::Deserialize;

use crate::{Action, Condition};

/// A domain heuristic that fires when the executor detects a known "lost state".
#[derive(Clone)]
pub struct RecoveryHandler {
    /// Descriptive name shown in logs (e.g. `"dismiss_error_dialog"`).
    pub name: String,

    /// Condition that identifies this recovery scenario.
    pub trigger: Condition,

    /// Actions to execute to restore a known-good state.
    pub actions: Vec<Action>,

    /// What the executor does after the recovery actions complete.
    pub resume: ResumeStrategy,
}

/// What the executor does after a recovery handler fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResumeStrategy {
    /// Re-execute the failing step from scratch.
    RetryStep,

    /// Consider the step already done and advance to the next one.
    SkipStep,

    /// Treat the step as failed — propagate the error.
    Fail,

    /// Restart the entire phase from step 1.
    /// Use when the recovery action may invalidate state that earlier steps established
    /// (e.g. dismissing a dialog that closed an open dropdown).
    RetryPhase,
}
