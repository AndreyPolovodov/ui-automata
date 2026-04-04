mod common;
use common::*;

/// A `finally` phase runs even when an earlier phase failed.
#[test]
fn finally_runs_after_phase_failure() {
    let yaml = r#"
name: test
anchors:
  bad:
    type: Root
    selector: "[name=DoesNotExist]"
phases:
  - name: phase_a
    steps: []
  - name: phase_b
    mount: [bad]
    steps: []
  - name: phase_c
    finally: true
    steps: []
"#;
    let (result, events) = run(yaml, empty_desktop());
    assert!(result.is_err());
    assert_eq!(
        event_names(&events),
        [
            "started:phase_a",
            "completed:phase_a",
            "started:phase_c",
            "completed:phase_c",
            "Failed",
        ],
    );
}

/// Normal phases after a failure are skipped; `finally` phases still run.
#[test]
fn normal_phase_after_failure_is_skipped() {
    let yaml = r#"
name: test
anchors:
  bad:
    type: Root
    selector: "[name=DoesNotExist]"
phases:
  - name: failing
    mount: [bad]
    steps: []
  - name: skipped
    steps: []
  - name: cleanup
    finally: true
    steps: []
"#;
    let (result, events) = run(yaml, empty_desktop());
    assert!(result.is_err());
    assert_eq!(
        event_names(&events),
        ["started:cleanup", "completed:cleanup", "Failed"],
    );
}

/// Multiple `finally` phases all run in order after a failure.
#[test]
fn multiple_finally_phases_run_in_order() {
    let yaml = r#"
name: test
anchors:
  bad:
    type: Root
    selector: "[name=DoesNotExist]"
phases:
  - name: main
    mount: [bad]
    steps: []
  - name: cleanup_1
    finally: true
    steps: []
  - name: cleanup_2
    finally: true
    steps: []
"#;
    let (result, events) = run(yaml, empty_desktop());
    assert!(result.is_err());
    assert_eq!(
        event_names(&events),
        [
            "started:cleanup_1",
            "completed:cleanup_1",
            "started:cleanup_2",
            "completed:cleanup_2",
            "Failed",
        ],
    );
}

/// A `finally` phase that fails its steps still propagates the original error.
/// The finally phase itself emits started/failed; terminal event is Failed, not Completed.
#[test]
fn finally_phase_failure_does_not_suppress_original_error() {
    // Both phases have steps: main fails its step; cleanup (finally) also fails its step.
    // The original error from main must be returned and Completed must not be emitted.
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
  app2:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: fail this step
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=DoesNotExist]"
        timeout: 50ms
  - name: cleanup
    finally: true
    mount: [app2]
    steps:
      - intent: also fail
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app2
          selector: ">> [role=button][name=AlsoMissing]"
        timeout: 50ms
"#;
    let desktop = ui_automata::mock::mock_desktop_from_yaml(
        r#"
role: window
name: App
"#,
    );
    let (result, events) = run(yaml, desktop);
    assert!(result.is_err(), "original error must propagate");
    assert_eq!(
        event_names(&events),
        [
            "started:main",
            "failed:main",
            "started:cleanup",
            "failed:cleanup",
            "Failed"
        ],
    );
}

/// When all phases succeed (including finally), workflow returns Ok and Completed is emitted.
#[test]
fn all_phases_complete_on_success() {
    let yaml = r#"
name: test
phases:
  - name: phase_a
    steps: []
  - name: phase_b
    finally: true
    steps: []
"#;
    let (result, events) = run(yaml, empty_desktop());
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        event_names(&events),
        [
            "started:phase_a",
            "completed:phase_a",
            "started:phase_b",
            "completed:phase_b",
            "Completed",
        ],
    );
}
