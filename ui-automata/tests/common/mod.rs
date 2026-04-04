/// Shared test helpers for workflow integration tests.
use std::collections::HashMap;

use ui_automata::mock::{MockDesktop, MockElement};
use ui_automata::yaml::{PhaseEvent, WorkflowFile};
use ui_automata::{AutomataError, Executor, Output};

#[allow(dead_code)]
pub fn parse(yaml: &str) -> WorkflowFile {
    WorkflowFile::load_from_str(yaml, &HashMap::new()).expect("YAML parse failed")
}

#[allow(dead_code)]
pub fn run(yaml: &str, desktop: MockDesktop) -> (Result<(), AutomataError>, Vec<PhaseEvent>) {
    let (result, events, _) = run_full(yaml, desktop);
    (result, events)
}

#[allow(dead_code)]
pub fn run_full(
    yaml: &str,
    desktop: MockDesktop,
) -> (Result<(), AutomataError>, Vec<PhaseEvent>, Output) {
    let wf = parse(yaml);
    let mut executor = Executor::new(desktop);
    let mut events = Vec::new();
    let result_with_state = wf.run(&mut executor, Some(&mut |e| events.push(e)), None);
    let (result, output) = match result_with_state {
        Ok(state) => (Ok(()), state.output),
        Err(e) => (Err(e), Output::new()),
    };
    (result, events, output)
}

#[allow(dead_code)]
pub fn empty_desktop() -> MockDesktop {
    MockDesktop::new(vec![])
}

#[allow(dead_code)]
pub fn app_desktop() -> MockDesktop {
    MockDesktop::new(vec![MockElement::parent("window", "App", vec![])])
}

#[allow(dead_code)]
pub fn event_names(events: &[PhaseEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| match e {
            PhaseEvent::PhaseStarted(n) => format!("started:{n}"),
            PhaseEvent::PhaseCompleted(n) => format!("completed:{n}"),
            PhaseEvent::PhaseSkipped(n) => format!("skipped:{n}"),
            PhaseEvent::PhaseFailed { phase, .. } => format!("failed:{phase}"),
            PhaseEvent::Completed => "Completed".into(),
            PhaseEvent::Failed(_) => "Failed".into(),
        })
        .collect()
}
