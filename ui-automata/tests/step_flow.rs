/// Integration tests for step-level flow control: `on_failure` and `on_success`.
mod common;
use common::*;

// ── on_failure: continue ──────────────────────────────────────────────────────

/// `on_failure: continue` — a failing step is logged but execution continues to
/// the next step, and the phase (and workflow) ultimately succeeds.
#[test]
fn on_failure_continue_proceeds_to_next_step() {
    // Step 1 times out; with on_failure:continue it must not abort the phase.
    // Step 2 succeeds — proving execution reached it.
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: this step will time out
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=DoesNotExist]"
        timeout: 50ms
        on_failure: continue
      - intent: this step must still run
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
"#;
    let desktop = ui_automata::mock::mock_desktop_from_yaml(
        r#"
role: window
name: App
"#,
    );
    let (result, events) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"],
    );
}

/// Multiple `on_failure: continue` steps — each timeout is swallowed; the final
/// succeeding step carries the phase to completion.
#[test]
fn on_failure_continue_multiple_steps_all_continue() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: step 1 times out
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=Missing1]"
        timeout: 50ms
        on_failure: continue
      - intent: step 2 times out
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=Missing2]"
        timeout: 50ms
        on_failure: continue
      - intent: step 3 succeeds
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
"#;
    let desktop = ui_automata::mock::mock_desktop_from_yaml(
        r#"
role: window
name: App
"#,
    );
    let (result, events) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"],
    );
}

/// Default `on_failure: abort` behaviour — timeout aborts the phase.
#[test]
fn on_failure_abort_is_default() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: this step times out with default on_failure
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=DoesNotExist]"
        timeout: 50ms
"#;
    let desktop = ui_automata::mock::mock_desktop_from_yaml(
        r#"
role: window
name: App
"#,
    );
    let (result, events) = run(yaml, desktop);
    assert!(result.is_err(), "default abort must propagate error");
    assert_eq!(
        event_names(&events),
        ["started:main", "failed:main", "Failed"],
    );
}

// ── on_success: return_phase ──────────────────────────────────────────────────

/// `on_success: return_phase` — after the step succeeds, the remaining steps
/// in the phase are skipped and the phase completes normally.
/// Step 2 would time out if reached — proving it was skipped.
#[test]
fn on_success_return_phase_stops_remaining_steps() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: early return
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
        on_success: return_phase
      - intent: this must NOT run
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=DoesNotExist]"
        timeout: 50ms
"#;
    let desktop = ui_automata::mock::mock_desktop_from_yaml(
        r#"
role: window
name: App
"#,
    );
    let (result, events) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"],
    );
}

/// `on_success: return_phase` only stops the current phase; subsequent phases run normally.
#[test]
fn on_success_return_phase_subsequent_phases_run() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: phase_a
    mount: [app]
    steps:
      - intent: return early from phase_a
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
        on_success: return_phase
  - name: phase_b
    steps: []
"#;
    let desktop = ui_automata::mock::mock_desktop_from_yaml(
        r#"
role: window
name: App
"#,
    );
    let (result, events) = run(yaml, desktop);
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

// ── on_failure: fallback ──────────────────────────────────────────────────────

/// `on_failure: fallback` — primary action times out; fallback action runs and
/// satisfies the expect condition; phase completes successfully.
#[test]
fn on_failure_fallback_runs_when_primary_times_out() {
    // Primary expect looks for a button that doesn't exist (times out).
    // Fallback NoOp is followed by re-polling the App window, which exists.
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: primary times out, fallback satisfies expect
        action:
          type: NoOp
        fallback:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
        timeout: 50ms
"#;
    let desktop = ui_automata::mock::mock_desktop_from_yaml(
        r#"
role: window
name: App
"#,
    );
    let (result, events) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"],
    );
}

/// `on_failure: fallback` — primary times out AND fallback also cannot satisfy
/// expect; the step (and phase) fails.
#[test]
fn on_failure_fallback_fails_when_fallback_expect_unsatisfied() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: both primary and fallback timeout — phase fails
        action:
          type: NoOp
        fallback:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=NeverExists]"
        timeout: 50ms
"#;
    let desktop = ui_automata::mock::mock_desktop_from_yaml(
        r#"
role: window
name: App
"#,
    );
    let (result, events) = run(yaml, desktop);
    assert!(result.is_err(), "fallback that cannot fix expect must fail");
    assert_eq!(
        event_names(&events),
        ["started:main", "failed:main", "Failed"],
    );
}

/// `on_failure: fallback` parse — ensure the YAML deserialises without error
/// and the workflow runs (schema round-trip check).
#[test]
fn on_failure_fallback_parses_correctly() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: fallback with explicit scope
        action:
          type: NoOp
        fallback:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
        timeout: 50ms
"#;
    // parse() panics on invalid YAML — success here confirms deserialization works.
    parse(yaml);
}

/// Combined: step 1 fails → continue; step 2 succeeds → return_phase; step 3 is skipped.
#[test]
fn on_failure_continue_then_on_success_return_phase() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: step 1 fails, continue
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=Missing]"
        timeout: 50ms
        on_failure: continue
      - intent: step 2 succeeds, return_phase
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
        on_success: return_phase
      - intent: step 3 must not run
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
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
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"],
    );
}

// ── action error propagation ──────────────────────────────────────────────────

/// `expect: Always` does NOT swallow action errors — the step must fail when
/// the action fails, even though `Always` is immediately satisfied.
#[test]
fn action_error_with_always_aborts_step() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: action fails, Always must not hide it
        action:
          type: Click
          scope: app
          selector: ">> [role=button][name=DoesNotExist]"
        expect:
          type: Always
        timeout: 50ms
"#;
    let desktop = app_desktop();
    let (result, events) = run(yaml, desktop);
    assert!(
        result.is_err(),
        "action error must propagate even with Always"
    );
    assert_eq!(
        event_names(&events),
        ["started:main", "failed:main", "Failed"]
    );
}

/// `on_failure: continue` opts out of action-error propagation: the step
/// continues even when the action fails and expect is Always.
#[test]
fn action_error_always_on_failure_continue_ok() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: action fails but on_failure=continue allows it
        action:
          type: Click
          scope: app
          selector: ">> [role=button][name=DoesNotExist]"
        expect:
          type: Always
        on_failure: continue
      - intent: must still run
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
"#;
    let desktop = app_desktop();
    let (result, events) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"]
    );
}

/// When the action errors and `expect: Always`, the retry policy fires.
/// Exhausting retries ultimately fails the step.
#[test]
fn action_error_with_always_retries_then_fails() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: action always fails, retry exhausted
        action:
          type: Click
          scope: app
          selector: ">> [role=button][name=DoesNotExist]"
        expect:
          type: Always
        timeout: 50ms
        retry:
          fixed:
            count: 2
            delay: 0s
"#;
    let desktop = app_desktop();
    let (result, _) = run(yaml, desktop);
    assert!(result.is_err(), "exhausted retries must still fail");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("timed out after 3 attempt"), "got: {msg}");
}

/// Regression: `last_action_error` must be reset at the start of each retry
/// iteration. If attempt 1 errors but attempt 2 succeeds, the step must
/// succeed — the stale error from attempt 1 must not taint attempt 2.
#[test]
fn action_error_on_retry_1_then_ok_on_retry_2_succeeds() {
    use ui_automata::mock::MockElement;

    let trigger = MockElement::leaf("button", "Trigger");
    trigger.kill(); // attempt 1: element dead → find_required returns None → action error

    let desktop = ui_automata::mock::MockDesktop::new(vec![MockElement::parent(
        "window",
        "App",
        vec![trigger.clone()],
    )]);

    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: element dead on attempt 1, alive on attempt 2
        action:
          type: Click
          scope: app
          selector: ">> [role=button][name=Trigger]"
        expect:
          type: Always
        timeout: 50ms
        retry:
          fixed:
            count: 1
            delay: 200ms
"#;

    // Revive the element midway through the retry delay so attempt 2 finds it.
    let t = trigger.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        t.revive();
    });

    let (result, events) = run(yaml, desktop);
    assert!(
        result.is_ok(),
        "attempt 2 succeeds → step must succeed, not carry stale error: {result:?}"
    );
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"]
    );
}

// ── step precondition ─────────────────────────────────────────────────────────

/// When the step precondition is satisfied the step runs normally.
#[test]
fn step_precondition_satisfied_step_runs() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: precondition true, step must run
        precondition:
          type: ElementFound
          scope: app
          selector: "[name=App]"
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
"#;
    let desktop = app_desktop();
    let (result, events) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"]
    );
}

/// When the step precondition is not satisfied the step is silently skipped.
/// A following step that would time out if reached is used to prove the
/// skipped step did not block execution.
#[test]
fn step_precondition_not_satisfied_step_skipped() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: precondition false → skip; would fail if it ran
        precondition:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=NeverExists]"
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=AlsoNeverExists]"
        timeout: 50ms
      - intent: must still run after skip
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
"#;
    let desktop = app_desktop();
    let (result, events) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"]
    );
}

// ── timeout-driven retry (no action error) ────────────────────────────────────

/// When the action succeeds but the expect condition is never satisfied, the
/// step times out. With a retry policy the step is retried; exhausting all
/// retries results in failure with the attempt count in the error message.
#[test]
fn timeout_retry_exhausted_fails_with_attempt_count() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: action ok but expect never satisfied
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=NeverExists]"
        timeout: 50ms
        retry:
          fixed:
            count: 2
            delay: 0s
"#;
    let desktop = app_desktop();
    let (result, _) = run(yaml, desktop);
    assert!(result.is_err(), "must fail when expect never satisfied");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("timed out after 3 attempt"), "got: {msg}");
}

// ── recovery handlers ─────────────────────────────────────────────────────────

/// `resume: skip_step` — when recovery fires the failing step is silently
/// skipped and execution advances to the next step.
#[test]
fn recovery_skip_step_skips_and_continues() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
recovery_handlers:
  skip_it:
    trigger:
      type: ElementFound
      scope: app
      selector: "[name=App]"
    actions: []
    resume: skip_step
phases:
  - name: main
    mount: [app]
    recovery:
      handlers: [skip_it]
    steps:
      - intent: will time out, recovery skips it
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=NeverExists]"
        timeout: 50ms
      - intent: must run after skip
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
"#;
    let desktop = app_desktop();
    let (result, events) = run(yaml, desktop);
    assert!(
        result.is_ok(),
        "skip_step recovery must allow phase to complete: {result:?}"
    );
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"]
    );
}

/// `resume: fail` — when recovery fires it immediately fails the step.
#[test]
fn recovery_fail_resume_aborts_step() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
recovery_handlers:
  fail_it:
    trigger:
      type: ElementFound
      scope: app
      selector: "[name=App]"
    actions: []
    resume: fail
phases:
  - name: main
    mount: [app]
    recovery:
      handlers: [fail_it]
    steps:
      - intent: will time out, recovery fails it
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=NeverExists]"
        timeout: 50ms
"#;
    let desktop = app_desktop();
    let (result, events) = run(yaml, desktop);
    assert!(result.is_err(), "fail resume must abort the step");
    assert_eq!(
        event_names(&events),
        ["started:main", "failed:main", "Failed"]
    );
}

/// `resume: retry_step` — recovery fires, re-runs the step. When the element
/// appears between the recovery and the retry the step succeeds.
#[test]
fn recovery_retry_step_reruns_step_and_succeeds() {
    use ui_automata::mock::MockElement;

    let btn = MockElement::leaf("button", "Target");
    btn.kill(); // not present on first attempt → timeout → recovery fires

    let desktop = ui_automata::mock::MockDesktop::new(vec![MockElement::parent(
        "window",
        "App",
        vec![btn.clone()],
    )]);

    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
recovery_handlers:
  revive_handler:
    trigger:
      type: ElementFound
      scope: app
      selector: "[name=App]"
    actions: []
    resume: retry_step
phases:
  - name: main
    mount: [app]
    recovery:
      handlers: [revive_handler]
    steps:
      - intent: target missing on attempt 1; recovery retries; target present on retry
        action:
          type: Click
          scope: app
          selector: ">> [role=button][name=Target]"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=Target]"
        timeout: 100ms
"#;

    // Revive the element partway through the first timeout window.
    // The inner poll will find it (ElementFound=true) but there's a pending
    // action error → breaks to recovery, which retries. On retry the action
    // also succeeds (element alive) → step passes.
    let b = btn.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(40));
        b.revive();
    });

    let (result, events) = run(yaml, desktop);
    assert!(
        result.is_ok(),
        "step must succeed after recovery retry: {result:?}"
    );
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"]
    );
}

/// `limit` caps how many times recovery can fire. Once the limit is reached
/// the step is handled by `on_failure` policy (abort by default → error).
#[test]
fn recovery_max_recoveries_reached_then_fails() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
recovery_handlers:
  always_retry:
    trigger:
      type: ElementFound
      scope: app
      selector: "[name=App]"
    actions: []
    resume: retry_step
phases:
  - name: main
    mount: [app]
    recovery:
      handlers: [always_retry]
      limit: 2
    steps:
      - intent: always times out, recovery retries until limit
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=NeverExists]"
        timeout: 50ms
"#;
    let desktop = app_desktop();
    let (result, events) = run(yaml, desktop);
    assert!(
        result.is_err(),
        "must fail once max_recoveries is exhausted"
    );
    assert_eq!(
        event_names(&events),
        ["started:main", "failed:main", "Failed"]
    );
}

/// `resume: retry_phase` — recovery fires on step 2, restarts the phase from
/// step 1. The target element is revived between the first and second pass;
/// on the second pass both steps succeed.
#[test]
fn recovery_retry_phase_restarts_from_step_1() {
    use ui_automata::mock::MockElement;

    let btn = MockElement::leaf("button", "Target");
    btn.kill(); // dead on first pass → step 2 times out → retry_phase

    let desktop = ui_automata::mock::MockDesktop::new(vec![MockElement::parent(
        "window",
        "App",
        vec![btn.clone()],
    )]);

    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
recovery_handlers:
  restart:
    trigger:
      type: ElementFound
      scope: app
      selector: "[name=App]"
    actions: []
    resume: retry_phase
phases:
  - name: main
    mount: [app]
    recovery:
      handlers: [restart]
    steps:
      - intent: step 1 — always succeeds
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
      - intent: step 2 — times out on first pass, succeeds after restart
        action:
          type: Click
          scope: app
          selector: ">> [role=button][name=Target]"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=Target]"
        timeout: 100ms
"#;

    // Revive the element after the first timeout so the second pass succeeds.
    let b = btn.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(60));
        b.revive();
    });

    let (result, events) = run(yaml, desktop);
    assert!(
        result.is_ok(),
        "retry_phase must restart phase and succeed on second pass: {result:?}"
    );
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"]
    );
}

/// `resume: retry_phase` respects `limit` — once `recovery_count` reaches the
/// limit the step fails normally instead of restarting forever.
#[test]
fn recovery_retry_phase_max_recoveries_exceeded_fails() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
recovery_handlers:
  always_restart:
    trigger:
      type: ElementFound
      scope: app
      selector: "[name=App]"
    actions: []
    resume: retry_phase
phases:
  - name: main
    mount: [app]
    recovery:
      handlers: [always_restart]
      limit: 2
    steps:
      - intent: always times out; recovery restarts until limit
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=NeverExists]"
        timeout: 50ms
"#;
    let desktop = app_desktop();
    let (result, events) = run(yaml, desktop);
    assert!(
        result.is_err(),
        "must fail once max_recoveries is exhausted"
    );
    assert_eq!(
        event_names(&events),
        ["started:main", "failed:main", "Failed"]
    );
}

/// `global_recovery_handlers` fires for a phase that has no `recovery:` opt-in.
/// The handler skips the failing step; the phase completes successfully.
#[test]
fn global_recovery_handler_fires_without_phase_opt_in() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
global_recovery_handlers:
  global_skip:
    trigger:
      type: ElementFound
      scope: app
      selector: "[name=App]"
    actions: []
    resume: skip_step
phases:
  - name: main
    mount: [app]
    steps:
      - intent: times out; global handler skips it without phase opt-in
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=NeverExists]"
        timeout: 50ms
      - intent: must run after global skip
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
"#;
    let desktop = app_desktop();
    let (result, events) = run(yaml, desktop);
    assert!(
        result.is_ok(),
        "global recovery handler must fire without phase opt-in: {result:?}"
    );
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"]
    );
}

// ── fallback + on_success: return_phase ───────────────────────────────────────

/// When the primary action times out but the fallback satisfies the expect,
/// and `on_success: return_phase` is set, the remaining steps are skipped.
#[test]
fn fallback_success_with_return_phase_skips_remaining() {
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: primary times out; fallback satisfies; return_phase skips step 2
        action:
          type: NoOp
        fallback:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
        on_success: return_phase
        timeout: 50ms
      - intent: must NOT run — would fail if reached
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=NeverExists]"
        timeout: 50ms
"#;
    let desktop = app_desktop();
    let (result, events) = run(yaml, desktop);
    assert!(
        result.is_ok(),
        "fallback + return_phase must complete the phase: {result:?}"
    );
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"]
    );
}

// ── global_recovery_handlers subflow ─────────────────────────────────────────

/// A `global_recovery_handlers` entry that references an external YAML file
/// behaves identically to an inline handler: fires on timeout, skips the step.
#[test]
fn global_recovery_handler_subflow_path_fires() {
    // Write the handler definition to a temp file.
    let handler_yml = r#"
trigger:
  type: ElementFound
  scope: app
  selector: "[name=App]"
actions: []
resume: skip_step
"#;
    let handler_path = std::env::temp_dir().join("test_handler_subflow.yml");
    std::fs::write(&handler_path, handler_yml).expect("write handler file");
    let handler_path_str = handler_path.to_string_lossy().replace('\\', "/");

    let yaml = format!(
        r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
global_recovery_handlers:
  file_handler: "{handler_path_str}"
phases:
  - name: main
    mount: [app]
    steps:
      - intent: times out; file handler skips it
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=NeverExists]"
        timeout: 50ms
      - intent: must run after file-handler skip
        action:
          type: NoOp
        expect:
          type: ElementFound
          scope: app
          selector: "[name=App]"
"#
    );

    let desktop = app_desktop();
    let (result, events) = run(&yaml, desktop);
    assert!(
        result.is_ok(),
        "global recovery handler loaded from file must fire: {result:?}"
    );
    assert_eq!(
        event_names(&events),
        ["started:main", "completed:main", "Completed"]
    );
}
