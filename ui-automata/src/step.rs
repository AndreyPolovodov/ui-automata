use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::Action;
use crate::Condition;

/// A single automation intent: execute an action, then wait for an expected UI state.
#[derive(Deserialize, JsonSchema)]
pub struct Step {
    /// Human-readable label shown in logs, e.g. `"click the Save button"`.
    pub intent: String,
    /// Optional guard evaluated before the action. If false, the step is skipped (not an error).
    /// Useful for conditional steps such as "dismiss dialog if it appeared".
    #[serde(default)]
    pub precondition: Option<Condition>,
    /// The UI action to perform: click, type text, press a key, close a window, etc.
    pub action: Action,
    /// Optional fallback action run when `expect` times out on the primary action.
    /// After the fallback runs, `expect` is re-polled once with a fresh timeout.
    /// If it succeeds, the step succeeds; otherwise `on_failure` decides what happens.
    #[serde(default)]
    pub fallback: Option<Action>,
    /// Condition that must become true after the action for the step to succeed.
    /// Polled every 100 ms until satisfied or the timeout elapses.
    pub expect: Condition,
    /// Maximum time to wait for `expect` to become true. Overrides the workflow default.
    /// Accepts duration strings such as `"5s"`, `"300ms"`, `"2m"`.
    #[serde(default, with = "crate::duration::serde::option")]
    #[schemars(schema_with = "crate::schema::duration_schema")]
    pub timeout: Option<Duration>,
    /// Retry policy on timeout. Overrides the workflow default.
    /// Default: `none` — falls back to the workflow-level default.
    #[serde(default)]
    pub retry: RetryPolicy,
    /// What to do when this step fails (expect condition times out, or fallback also fails).
    /// Default: `abort` — propagate the error and stop the phase.
    #[serde(default)]
    pub on_failure: OnFailure,
    /// What to do immediately after this step succeeds.
    /// Default: `continue` — proceed to the next step.
    #[serde(default)]
    pub on_success: OnSuccess,
}

/// Controls executor behaviour when a step's `expect` condition times out (and any
/// `fallback` action also fails to satisfy it).
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnFailure {
    /// Propagate the error; abort the current phase. This is the default.
    #[default]
    Abort,
    /// Log the failure, then continue to the next step as if the step had succeeded.
    Continue,
}

/// Controls executor behaviour immediately after a step succeeds.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnSuccess {
    /// Proceed to the next step. This is the default.
    #[default]
    Continue,
    /// Stop executing steps in the current phase immediately (not an error).
    ReturnPhase,
}

/// What the executor does when a step's `expect` condition times out.
///
/// Custom `Deserialize` via `TryFrom<serde_yaml::Value>` so YAML
/// `fixed: { count: 1, delay: 300ms }` maps cleanly without serde_yaml's
/// externally-tagged enum quirks.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(try_from = "serde_yaml::Value")]
pub enum RetryPolicy {
    /// No retries — fall back to the workflow default, or fail immediately if there is none.
    #[default]
    None,
    /// Re-execute the action up to `count` additional times.
    Fixed { count: u32, delay: Duration },
    /// Run recovery handlers on timeout, then retry the step.
    WithRecovery,
}

impl TryFrom<serde_yaml::Value> for RetryPolicy {
    type Error = String;

    fn try_from(v: serde_yaml::Value) -> Result<Self, String> {
        match &v {
            serde_yaml::Value::String(s) => match s.as_str() {
                "none" => Ok(RetryPolicy::None),
                "with_recovery" => Ok(RetryPolicy::WithRecovery),
                other => Err(format!("unknown RetryPolicy '{other}'")),
            },
            serde_yaml::Value::Mapping(map) => {
                if let Some(fixed) = map.get("fixed") {
                    let count = fixed
                        .get("count")
                        .and_then(|v| v.as_u64())
                        .ok_or("RetryPolicy.fixed missing 'count'")?
                        as u32;
                    let delay = fixed
                        .get("delay")
                        .and_then(|v| v.as_str())
                        .ok_or("RetryPolicy.fixed missing 'delay'")
                        .and_then(|s| crate::duration::from_str(s))?;
                    Ok(RetryPolicy::Fixed { count, delay })
                } else {
                    Err(format!("unknown RetryPolicy mapping: {v:?}"))
                }
            }
            _ => Err(format!("invalid RetryPolicy value: {v:?}")),
        }
    }
}
