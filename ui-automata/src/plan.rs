use std::time::Duration;

use crate::{RecoveryHandler, RetryPolicy, Step};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// An ordered sequence of steps toward a single goal, with local recovery handlers.
///
/// `Plan` borrows its steps, name, and unmount list from the owner (typically a
/// `YamlActionPhase`). This means phases are never consumed and backward `go_to`
/// jumps in flow-control loops just work.
pub struct Plan<'a> {
    /// Human-readable name shown in logs (e.g. `"open_file"`).
    pub name: &'a str,

    /// Steps executed in order.
    pub steps: &'a [Step],

    /// Recovery handlers checked (before global ones) when a step times out.
    pub recovery_handlers: Vec<RecoveryHandler>,

    /// Maximum number of recovery handler firings across all steps in this plan.
    pub max_recoveries: u32,

    /// Anchor names to unmount after the plan finishes (success or failure).
    pub unmount: &'a [String],

    /// Timeout applied to steps that do not specify their own `timeout_secs`.
    pub default_timeout: Duration,

    /// Retry policy applied to steps that do not specify their own `retry`.
    pub default_retry: RetryPolicy,
}
