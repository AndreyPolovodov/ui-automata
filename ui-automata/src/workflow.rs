use crate::plan::DEFAULT_TIMEOUT;
use crate::{
    AutomataError, Desktop, Executor, Plan, RecoveryHandler, RetryPolicy, Step, WorkflowState,
};
use std::time::Duration;

/// Owned data for one phase of a `Workflow`.
pub struct WorkflowPhase {
    pub name: String,
    pub steps: Vec<Step>,
    pub recovery_handlers: Vec<RecoveryHandler>,
    pub unmount: Vec<String>,
    pub default_timeout: Duration,
    pub default_retry: RetryPolicy,
}

impl WorkflowPhase {
    pub fn new(name: impl Into<String>, steps: Vec<Step>) -> Self {
        Self {
            name: name.into(),
            steps,
            recovery_handlers: vec![],
            unmount: vec![],
            default_timeout: DEFAULT_TIMEOUT,
            default_retry: RetryPolicy::None,
        }
    }
}

/// A sequence of named plans run in order through a single executor.
pub struct Workflow<D: Desktop> {
    phases: Vec<WorkflowPhase>,
    _marker: std::marker::PhantomData<D>,
}

impl<D: Desktop> Workflow<D> {
    pub fn new() -> Self {
        Self {
            phases: vec![],
            _marker: std::marker::PhantomData,
        }
    }

    /// Add a phase to the workflow.
    pub fn phase(mut self, phase: WorkflowPhase) -> Self {
        self.phases.push(phase);
        self
    }

    /// Run all phases in order, stopping on the first failure.
    pub fn run(&self, executor: &mut Executor<D>) -> Result<(), AutomataError> {
        let mut state = WorkflowState::new(true);
        for phase in &self.phases {
            eprintln!("[workflow] phase: {}", phase.name);
            let plan = Plan {
                name: &phase.name,
                steps: &phase.steps,
                recovery_handlers: phase.recovery_handlers.clone(),
                max_recoveries: 10,
                unmount: &phase.unmount,
                default_timeout: phase.default_timeout,
                default_retry: phase.default_retry.clone(),
            };
            executor.run(&plan, &mut state)?;
        }
        Ok(())
    }
}

impl<D: Desktop> Default for Workflow<D> {
    fn default() -> Self {
        Self::new()
    }
}
