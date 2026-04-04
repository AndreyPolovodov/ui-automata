mod common;
use common::*;

/// A phase whose `precondition` evaluates to false emits `PhaseSkipped` and continues.
#[test]
fn phase_precondition_false_skips_phase() {
    let yaml = r#"
name: test
phases:
  - name: conditional
    precondition:
      type: WindowWithAttribute
      title:
        contains: DoesNotExist
    steps: []
  - name: always_runs
    steps: []
"#;
    let (result, events) = run(yaml, empty_desktop());
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        event_names(&events),
        [
            "skipped:conditional",
            "started:always_runs",
            "completed:always_runs",
            "Completed",
        ],
    );
}

/// A phase whose `precondition` is true runs normally.
#[test]
fn phase_precondition_true_runs_phase() {
    let yaml = r#"
name: test
phases:
  - name: conditional
    precondition:
      type: WindowWithAttribute
      title:
        contains: App
    steps: []
"#;
    let (result, events) = run(yaml, app_desktop());
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        event_names(&events),
        ["started:conditional", "completed:conditional", "Completed"],
    );
}

/// Precondition false → skip, then a later normal phase fails → finally still runs.
#[test]
fn precondition_skip_then_failure_then_finally() {
    let yaml = r#"
name: test
anchors:
  bad:
    type: Root
    selector: "[name=DoesNotExist]"
phases:
  - name: skip_me
    precondition:
      type: WindowWithAttribute
      title:
        contains: DoesNotExist
    steps: []
  - name: fail_me
    mount: [bad]
    steps: []
  - name: cleanup
    finally: true
    steps: []
"#;
    let (result, events) = run(yaml, empty_desktop());
    assert!(result.is_err());
    assert_eq!(
        event_names(&events),
        [
            "skipped:skip_me",
            "started:cleanup",
            "completed:cleanup",
            "Failed"
        ],
    );
}
