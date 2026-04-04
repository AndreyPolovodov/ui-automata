use std::thread;
use std::time::{Duration, Instant};

use std::collections::HashMap;

use crate::{
    Action, AnchorDef, AutomataError, Desktop, OnFailure, Plan, RecoveryHandler, ResumeStrategy,
    RetryPolicy, ShadowDom, Step, output::Output, step::OnSuccess,
};

const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(PartialEq, Eq)]
enum StepOutcome {
    Continue,
    ReturnPhase,
}

/// Workflow-level mutable state: output, locals, and per-run flags.
/// Separated from `Executor` so it can be created, passed around, and
/// returned independently (e.g. across subflow invocations).
#[derive(Debug)]
pub struct WorkflowState {
    /// Resolved param values for this workflow invocation. Read-only during execution;
    /// accessible in `Eval` expressions as `param.key`.
    pub params: HashMap<String, String>,
    /// Workflow-local values from `Extract { local: true }`. Not propagated to
    /// parent workflows. Accessible via `{output.<key>}` within this workflow.
    pub locals: HashMap<String, String>,
    /// Accumulated output from `Extract` actions. Read after the workflow completes.
    /// Propagated to parent workflows via output.merge. Accessible via `{output.<key>}`.
    pub output: Output,
    /// When `false`, the post-action DOM snapshot is skipped entirely.
    /// Set via `defaults.action_snapshot: false` in the workflow YAML.
    pub action_snapshot: bool,
}

impl WorkflowState {
    pub fn new(action_snapshot: bool) -> Self {
        Self {
            locals: HashMap::new(),
            output: Output::new(),
            params: HashMap::new(),
            action_snapshot,
        }
    }
}

/// Runs plans against a live UIA desktop.
///
/// Owns the `ShadowDom` (element handle cache) and the desktop. Global recovery
/// handlers fire for every plan; plan-local handlers fire only within their plan.
pub struct Executor<D: Desktop> {
    pub dom: ShadowDom<D>,
    pub desktop: D,
    pub global_handlers: Vec<RecoveryHandler>,
}

impl<D: Desktop> Executor<D> {
    pub fn new(desktop: D) -> Self {
        Self {
            dom: ShadowDom::new(),
            desktop,
            global_handlers: vec![],
        }
    }

    /// Register anchor definitions.
    pub fn mount(&mut self, anchors: Vec<AnchorDef>) -> Result<(), AutomataError> {
        self.dom.mount(anchors, &self.desktop)
    }

    /// Unmount anchors by name.
    pub fn unmount(&mut self, names: &[&str]) {
        self.dom.unmount(names, &self.desktop);
    }

    /// Clean up DOM anchors at `depth`.
    pub fn cleanup_depth(&mut self, depth: usize) {
        self.dom.cleanup_depth(depth, &self.desktop);
    }

    /// Run all steps of a plan in order.
    ///
    /// Anchors listed in `plan.unmount` are always removed after the plan
    /// completes, whether it succeeds or fails (guaranteed cleanup).
    pub fn run(&mut self, plan: &Plan<'_>, state: &mut WorkflowState) -> Result<(), AutomataError> {
        self.log_info(&format!("plan: {}", plan.name));
        let total = plan.steps.len();
        let mut recovery_count: u32 = 0;
        let result = (|| {
            for (i, step) in plan.steps.iter().enumerate() {
                let outcome = self.run_step(
                    step,
                    &plan.recovery_handlers,
                    plan.max_recoveries,
                    &mut recovery_count,
                    i + 1,
                    total,
                    plan.default_timeout,
                    &plan.default_retry,
                    state,
                )?;
                if outcome == StepOutcome::ReturnPhase {
                    break;
                }
            }
            Ok(())
        })();
        if !plan.unmount.is_empty() {
            let names: Vec<&str> = plan.unmount.iter().map(String::as_str).collect();
            self.unmount(&names);
        }
        result
    }

    // ── Step execution ────────────────────────────────────────────────────────

    fn run_step(
        &mut self,
        step: &Step,
        local_handlers: &[RecoveryHandler],
        max_recoveries: u32,
        recovery_count: &mut u32,
        step_num: usize,
        total: usize,
        default_timeout: Duration,
        default_retry: &RetryPolicy,
        state: &mut WorkflowState,
    ) -> Result<StepOutcome, AutomataError> {
        let prefix = format!("step {step_num}/{total}");
        let label = format!("{prefix} '{}'", step.intent);
        self.log_info(&label);

        let timeout = step.timeout.unwrap_or(default_timeout);
        let retry = match &step.retry {
            RetryPolicy::None => default_retry,
            policy => policy,
        };

        if let Some(pre) = &step.precondition {
            let pre_desc = pre.describe();
            log::debug!("precondition: {pre_desc}");
            if !self.eval(pre, state)? {
                log::debug!("{prefix}: precondition not satisfied, skipping");
                return Ok(StepOutcome::Continue);
            }
        }

        let action = step.action.apply_output(&state.locals, &state.output);
        let expect = step.expect.apply_output(&state.locals, &state.output);

        let cond_desc = expect.describe();
        let action_desc = action.describe();

        let mut attempts: u32 = 0;
        let mut last_action_error: Option<String>;
        loop {
            last_action_error = None; // reset each attempt; prior attempt's error must not bleed into this one
            log::debug!("action: {action_desc}");
            let action_result = self.exec(&action, state);
            match &action_result {
                Ok(()) => log::debug!("action → Ok"),
                Err(e) => {
                    let msg = e.to_string();
                    // Downgrade to debug when on_failure=continue — the caller
                    // expects failure and the step-level outcome handles it.
                    match &step.on_failure {
                        OnFailure::Continue => {
                            log::debug!("{label}: action → Err: {msg}");
                        }
                        OnFailure::Abort => {
                            self.log_warn(&format!("{label}: action → Err: {msg}"));
                        }
                    }
                    last_action_error = Some(msg);
                }
            }
            // Sync once after the action so the trace captures what changed.
            // Skipped when action_snapshot is false (e.g. complex windows with deep trees).
            if state.action_snapshot {
                if let Some(scope) = expect.scope_name() {
                    self.dom.sync(scope, &self.desktop);
                }
            }

            let deadline = Instant::now() + timeout;
            let mut last_poll: Option<bool> = None;
            loop {
                let satisfied = self.eval(&expect, state)?;
                if last_poll != Some(satisfied) {
                    log::debug!("poll: {cond_desc} → {satisfied}");
                    last_poll = Some(satisfied);
                }
                if satisfied {
                    // Action error prevents success — fall through to retry/recovery
                    // rather than returning immediately, so those mechanisms can fire.
                    // Exception: on_failure=continue explicitly opts out of this.
                    if let (Some(_), OnFailure::Abort) = (&last_action_error, &step.on_failure) {
                        break;
                    }
                    self.log_info(&format!("{prefix}: ok"));
                    return Ok(match step.on_success {
                        OnSuccess::Continue => StepOutcome::Continue,
                        OnSuccess::ReturnPhase => {
                            log::debug!("{prefix}: on_success=return_phase, stopping phase");
                            StepOutcome::ReturnPhase
                        }
                    });
                }
                if Instant::now() >= deadline {
                    break;
                }
                thread::sleep(POLL_INTERVAL);
            }

            let timeout_msg = format!(
                "{label}: timed out (attempt {}), checking recovery",
                attempts + 1
            );
            self.log_warn(&timeout_msg);

            let all: Vec<(String, crate::Condition, Vec<Action>, ResumeStrategy)> = local_handlers
                .iter()
                .chain(self.global_handlers.iter())
                .map(|h| {
                    (
                        h.name.clone(),
                        h.trigger.clone(),
                        h.actions.clone(),
                        h.resume,
                    )
                })
                .collect();

            let mut fired: Option<(String, Vec<Action>, ResumeStrategy)> = None;
            for (name, trigger, actions, resume) in all {
                if self.eval(&trigger, state)? {
                    fired = Some((name, actions, resume));
                    break;
                }
            }

            match fired {
                Some((name, actions, resume)) if *recovery_count < max_recoveries => {
                    *recovery_count += 1;
                    self.log_info(&format!(
                        "{label}: recovery handler '{name}' fired ({recovery_count}/{max_recoveries})"
                    ));
                    for action in &actions {
                        let rdesc = action.describe();
                        log::debug!("recovery action: {rdesc}");
                        if let Err(e) = self.exec(action, state) {
                            log::debug!("{label}: recovery action → Err: {e}");
                        } else {
                            log::debug!("recovery action → Ok");
                        }
                    }
                    match resume {
                        ResumeStrategy::RetryStep => {
                            attempts += 1;
                            continue;
                        }
                        ResumeStrategy::SkipStep => {
                            self.log_info(&format!("{label}: skipped by recovery"));
                            return Ok(StepOutcome::Continue);
                        }
                        ResumeStrategy::Fail => {
                            let msg = format!("{label}: recovery handler '{name}' instructed Fail");
                            log::debug!("{msg}");
                            return Err(AutomataError::Internal(msg));
                        }
                    }
                }
                Some((name, _, _)) => {
                    let msg = format!(
                        "{label}: recovery handler '{name}' would fire but max_recoveries ({max_recoveries}) reached"
                    );
                    self.log_warn(&msg);
                    return self
                        .apply_on_failure_policy(step, &label, &expect, timeout, msg, state);
                }
                None => match retry {
                    RetryPolicy::Fixed { count, delay } if attempts < *count => {
                        attempts += 1;
                        thread::sleep(*delay);
                        continue;
                    }
                    _ => {
                        let msg = match &last_action_error {
                            Some(e) => format!(
                                "{label}: timed out after {} attempt(s)\n  action error: {e}\n  expect: {cond_desc}",
                                attempts + 1
                            ),
                            None => format!(
                                "{label}: timed out after {} attempt(s)\n  expect: {cond_desc}",
                                attempts + 1
                            ),
                        };
                        log::debug!("{msg}");
                        return self
                            .apply_on_failure_policy(step, &label, &expect, timeout, msg, state);
                    }
                },
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Try `fallback` (if any), re-poll `expect`, then apply `on_failure` policy.
    fn apply_on_failure_policy(
        &mut self,
        step: &Step,
        label: &str,
        expect: &crate::Condition,
        timeout: Duration,
        failure_msg: String,
        state: &mut WorkflowState,
    ) -> Result<StepOutcome, AutomataError> {
        if let Some(fallback) = &step.fallback {
            self.log_info(&format!("{label}: trying fallback action"));
            if let Err(e) = self.exec(fallback, state) {
                log::debug!("{label}: fallback action → Err: {e}");
            }
            // Re-poll expect with a fresh timeout.
            let deadline = Instant::now() + timeout;
            loop {
                if self.eval(expect, state)? {
                    self.log_info(&format!("{label}: fallback succeeded"));
                    return Ok(match step.on_success {
                        OnSuccess::Continue => StepOutcome::Continue,
                        OnSuccess::ReturnPhase => {
                            log::debug!("{label}: on_success=return_phase, stopping phase");
                            StepOutcome::ReturnPhase
                        }
                    });
                }
                if Instant::now() >= deadline {
                    break;
                }
                thread::sleep(POLL_INTERVAL);
            }
            self.log_warn(&format!("{label}: fallback did not satisfy expect"));
        }
        match &step.on_failure {
            OnFailure::Abort => Err(AutomataError::Internal(failure_msg)),
            OnFailure::Continue => {
                self.log_warn(&format!("{label}: on_failure=continue, proceeding"));
                Ok(StepOutcome::Continue)
            }
        }
    }

    fn exec(&mut self, action: &Action, state: &mut WorkflowState) -> Result<(), AutomataError> {
        action.execute(
            &mut self.dom,
            &self.desktop,
            &mut state.output,
            &mut state.locals,
            &state.params,
        )
    }

    fn eval(
        &mut self,
        cond: &crate::Condition,
        state: &WorkflowState,
    ) -> Result<bool, AutomataError> {
        cond.evaluate(
            &mut self.dom,
            &self.desktop,
            &state.locals,
            &state.params,
            &state.output,
        )
    }

    /// Evaluate a condition against the current DOM state. Used by `WorkflowFile::run()`
    /// for phase-level preconditions before mounting anchors.
    pub fn eval_condition(
        &mut self,
        cond: &crate::Condition,
        locals: &std::collections::HashMap<String, String>,
        params: &std::collections::HashMap<String, String>,
        output: &crate::output::Output,
    ) -> Result<bool, AutomataError> {
        cond.evaluate(&mut self.dom, &self.desktop, locals, params, output)
    }

    fn log_info(&self, msg: &str) {
        log::info!("{msg}");
    }

    fn log_warn(&self, msg: &str) {
        log::warn!("{msg}");
    }
}
