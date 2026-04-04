/// Tests for the `subflow:` phase type.
///
/// Subflows are loaded from disk so each test writes a child workflow YAML
/// to a temp file and embeds the path in the parent workflow string.
use std::collections::HashMap;

use ui_automata::mock::{MockDesktop, MockElement};
use ui_automata::yaml::{PhaseEvent, WorkflowFile};
use ui_automata::{AutomataError, Executor, WorkflowState};

mod common;
use common::event_names;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn write_temp(name: &str, yaml: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("ui_automata_subflow_{name}.yml"));
    std::fs::write(&path, yaml).expect("failed to write temp workflow");
    path
}

fn run_subflow(
    parent_yaml: &str,
    desktop: MockDesktop,
) -> (Result<WorkflowState, AutomataError>, Vec<PhaseEvent>) {
    let wf = WorkflowFile::load_from_str(parent_yaml, &HashMap::new())
        .expect("parent YAML parse failed");
    let mut executor = Executor::new(desktop);
    let mut events = Vec::new();
    let result = wf.run(&mut executor, Some(&mut |e| events.push(e)), None);
    (result, events)
}

fn app_with_button() -> MockDesktop {
    MockDesktop::new(vec![MockElement::parent(
        "window",
        "App",
        vec![MockElement::leaf("button", "Save")],
    )])
}

fn app_with_two_buttons(first: &str, second: &str) -> MockDesktop {
    MockDesktop::new(vec![MockElement::parent(
        "window",
        "App",
        vec![
            MockElement::leaf("button", first),
            MockElement::leaf("button", second),
        ],
    )])
}

fn app_with_edit(text: &str) -> MockDesktop {
    MockDesktop::new(vec![MockElement::parent(
        "window",
        "App",
        vec![MockElement::leaf_text("edit", "Field", text)],
    )])
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Parent and child both Extract into the same key — values are appended, not overwritten.
#[test]
fn output_same_key_is_appended_not_overwritten() {
    let child = write_temp(
        "append_child",
        r#"
name: child
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: extract_save
    mount: [app]
    steps:
      - intent: extract Save button name into items
        action:
          type: Extract
          key: items
          scope: app
          selector: ">> [role=button][name=Save]"
          attribute: name
          multiple: false
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=Save]"
"#,
    );

    let parent = format!(
        r#"
name: parent
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: extract_open
    mount: [app]
    steps:
      - intent: extract Open button name into items
        action:
          type: Extract
          key: items
          scope: app
          selector: ">> [role=button][name=Open]"
          attribute: name
          multiple: false
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=Open]"
  - name: run_child
    subflow: {child}
"#,
        child = child.display()
    );

    let (result, _) = run_subflow(&parent, app_with_two_buttons("Open", "Save"));
    let state = result.expect("workflow failed");
    // Parent extracted "Open" first; child appended "Save".
    assert_eq!(state.output.get("items"), &["Open", "Save"]);
}

/// A subflow phase runs its child workflow and the parent completes.
#[test]
fn subflow_runs_and_completes() {
    let child = write_temp(
        "basic_child",
        r#"
name: child
anchors:
  root: { type: Root, selector: "*" }
phases:
  - name: child_step
    steps:
      - intent: no-op
        action: { type: NoOp }
        expect: { type: DialogAbsent, scope: root }
"#,
    );

    let parent = format!(
        r#"
name: parent
phases:
  - name: run_child
    subflow: {child}
"#,
        child = child.display()
    );

    let (result, events) = run_subflow(&parent, MockDesktop::new(vec![]));
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        event_names(&events),
        vec![
            "started:run_child",
            "started:child_step",
            "completed:child_step",
            "Completed",
            "completed:run_child",
            "Completed",
        ]
    );
}

/// Extract values from the child workflow appear in the parent's returned output.
#[test]
fn subflow_output_merges_into_parent() {
    let child = write_temp(
        "output_child",
        r#"
name: child
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: extract
    mount: [app]
    steps:
      - intent: extract button name
        action:
          type: Extract
          key: buttons
          scope: app
          selector: ">> [role=button]"
          attribute: name
          multiple: true
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button]"
"#,
    );

    let parent = format!(
        r#"
name: parent
phases:
  - name: run_child
    subflow: {child}
"#,
        child = child.display()
    );

    let (result, _) = run_subflow(&parent, app_with_button());
    let state = result.expect("workflow failed");
    assert_eq!(state.output.get("buttons"), &["Save"]);
}

/// A local Extract in the child does not leak into the parent's locals.
#[test]
fn subflow_locals_do_not_leak_to_parent() {
    let child = write_temp(
        "locals_child",
        r#"
name: child
outputs: []
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: extract
    mount: [app]
    steps:
      - intent: extract field text locally
        action:
          type: Extract
          key: child_local
          scope: app
          selector: ">> [role=edit]"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=edit]"
"#,
    );

    let parent = format!(
        r#"
name: parent
phases:
  - name: run_child
    subflow: {child}
"#,
        child = child.display()
    );

    let (result, _) = run_subflow(&parent, app_with_edit("hello"));
    let state = result.expect("workflow failed");
    assert!(
        !state.locals.contains_key("child_local"),
        "child local leaked into parent: {:?}",
        state.locals
    );
}

/// Literal params are passed to the child and substituted into its steps.
#[test]
fn subflow_receives_literal_params() {
    let child = write_temp(
        "params_child",
        r#"
name: child
params:
  - name: expected_name
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: verify
    mount: [app]
    steps:
      - intent: verify button exists with expected name
        action: { type: NoOp }
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name={param.expected_name}]"
"#,
    );

    let parent = format!(
        r#"
name: parent
phases:
  - name: run_child
    subflow: {child}
    params:
      expected_name: Save
"#,
        child = child.display()
    );

    let (result, _) = run_subflow(&parent, app_with_button());
    assert!(result.is_ok(), "{result:?}");
}

/// Parent captures a var at runtime; that var is forwarded as a child param.
#[test]
fn subflow_params_interpolated_from_parent_vars() {
    let child = write_temp(
        "interp_child",
        r#"
name: child
params:
  - name: btn_name
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: verify
    mount: [app]
    steps:
      - intent: verify button with forwarded name
        action: { type: NoOp }
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name={param.btn_name}]"
"#,
    );

    let parent = format!(
        r#"
name: parent
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: capture_name
    mount: [app]
    steps:
      - intent: capture button name
        action:
          type: Extract
          key: captured_name
          scope: app
          selector: ">> [role=button]"
          attribute: name
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button]"
  - name: run_child
    subflow: {child}
    params:
      btn_name: "{{output.captured_name}}"
"#,
        child = child.display()
    );

    let (result, _) = run_subflow(&parent, app_with_button());
    assert!(result.is_ok(), "{result:?}");
}

/// A failure inside the child propagates as a failure of the parent.
#[test]
fn subflow_failure_propagates_to_parent() {
    let child = write_temp(
        "fail_child",
        r#"
name: child
anchors:
  nonexistent_scope: { type: Root, selector: "*" }
phases:
  - name: doomed
    steps:
      - intent: find element that does not exist
        action: { type: NoOp }
        timeout: 100ms
        expect:
          type: ElementFound
          scope: nonexistent_scope
          selector: "[role=button]"
"#,
    );

    let parent = format!(
        r#"
name: parent
phases:
  - name: run_child
    subflow: {child}
"#,
        child = child.display()
    );

    let (result, events) = run_subflow(&parent, MockDesktop::new(vec![]));
    assert!(result.is_err(), "expected failure but got ok");
    assert_eq!(
        event_names(&events),
        vec![
            "started:run_child",
            "started:doomed",
            "failed:doomed",
            "Failed",
            "failed:run_child",
            "Failed",
        ]
    );
}

/// When a prior phase fails, the subflow phase is skipped (not run).
#[test]
fn subflow_skipped_after_prior_failure() {
    let child = write_temp(
        "skip_child",
        r#"
name: child
anchors:
  root: { type: Root, selector: "*" }
phases:
  - name: should_not_run
    steps:
      - intent: this must not execute
        action: { type: NoOp }
        expect: { type: DialogAbsent, scope: root }
"#,
    );

    let parent = format!(
        r#"
name: parent
anchors:
  nonexistent: {{ type: Root, selector: "*" }}
phases:
  - name: fail_first
    steps:
      - intent: fail immediately
        action: {{ type: NoOp }}
        timeout: 100ms
        expect:
          type: ElementFound
          scope: nonexistent
          selector: "[role=button]"
  - name: run_child
    subflow: {child}
"#,
        child = child.display()
    );

    let (result, events) = run_subflow(&parent, MockDesktop::new(vec![]));
    assert!(result.is_err());
    assert_eq!(
        event_names(&events),
        vec!["started:fail_first", "failed:fail_first", "Failed"]
    );
}

/// Parent's stable anchor survives a subflow that mounts the same anchor name.
/// The child's version is depth-prefixed (`:panel`), so the parent's (`panel`)
/// is untouched when the child unmounts it.
#[test]
fn stable_anchor_survives_subflow() {
    let child = write_temp(
        "anchor_child",
        r#"
name: child
anchors:
  app:
    type: Root
    selector: "[name=App]"
  btn:
    type: Stable
    parent: app
    selector: ">> [role=button]"
phases:
  - name: use_btn
    mount: [app, btn]
    unmount: [btn]
    steps:
      - intent: verify button accessible in child
        action: { type: NoOp }
        expect:
          type: ElementFound
          scope: btn
          selector: "*"
"#,
    );

    // Parent mounts `btn` before and uses it after the subflow.
    let parent = format!(
        r#"
name: parent
anchors:
  app:
    type: Root
    selector: "[name=App]"
  btn:
    type: Stable
    parent: app
    selector: ">> [role=button]"
phases:
  - name: before
    mount: [app, btn]
    steps:
      - intent: verify btn accessible before subflow
        action: {{ type: NoOp }}
        expect:
          type: ElementFound
          scope: btn
          selector: "*"
  - name: run_child
    subflow: {child}
  - name: after
    steps:
      - intent: verify btn still accessible after subflow
        action: {{ type: NoOp }}
        expect:
          type: ElementFound
          scope: btn
          selector: "*"
"#,
        child = child.display()
    );

    let (result, _) = run_subflow(&parent, app_with_button());
    assert!(
        result.is_ok(),
        "parent's stable anchor was destroyed by subflow: {result:?}"
    );
}

/// Child's depth-scoped stable anchor is cleaned up after the subflow returns.
/// The parent must not be able to use the child's anchor name after the child exits.
#[test]
fn child_stable_anchor_cleaned_up_after_subflow() {
    let child = write_temp(
        "cleanup_child",
        r#"
name: child
anchors:
  app:
    type: Root
    selector: "[name=App]"
  btn:
    type: Stable
    parent: app
    selector: ">> [role=button]"
phases:
  - name: use_btn
    mount: [app, btn]
    steps:
      - intent: use btn inside child
        action: { type: NoOp }
        expect:
          type: ElementFound
          scope: btn
          selector: "*"
"#,
    );

    // Parent declares `btn` in anchors but never mounts it. After the subflow,
    // `scope: btn` must fail at runtime because the anchor was never mounted
    // (the child's depth-scoped `:btn` is cleaned up and the parent's `btn`
    // is not mounted).
    let parent = format!(
        r#"
name: parent
anchors:
  app:
    type: Root
    selector: "[name=App]"
  btn:
    type: Stable
    parent: app
    selector: ">> [role=button]"
phases:
  - name: run_child
    subflow: {child}
  - name: after
    steps:
      - intent: btn should not be mounted in parent scope
        action: {{ type: NoOp }}
        timeout: 100ms
        expect:
          type: ElementFound
          scope: btn
          selector: "*"
"#,
        child = child.display()
    );

    // The `after` phase must fail because `btn` is not in the parent's dom.
    let (result, _) = run_subflow(&parent, app_with_button());
    assert!(
        result.is_err(),
        "child's scoped anchor should have been cleaned up, but parent succeeded: {result:?}"
    );
}

/// A root anchor introduced by a child subflow (not present in the parent) must
/// be cleaned up when the subflow exits, not leak into the parent's shadow DOM.
#[test]
fn child_root_anchor_cleaned_up_after_subflow() {
    let child = write_temp(
        "root_cleanup_child",
        r#"
name: child
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: use_app
    mount: [app]
    steps:
      - intent: verify app accessible inside child
        action: { type: NoOp }
        expect:
          type: ElementFound
          scope: app
          selector: "*"
"#,
    );

    // Parent declares `app` in anchors but never mounts it. After the subflow,
    // `scope: app` must fail at runtime because the anchor is not mounted
    // (the child's depth-scoped `:app` is cleaned up and the parent's `app`
    // is not mounted).
    let parent = format!(
        r#"
name: parent
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: run_child
    subflow: {child}
  - name: after
    steps:
      - intent: app should not be mounted in parent scope
        action: {{ type: NoOp }}
        timeout: 100ms
        expect:
          type: ElementFound
          scope: app
          selector: "*"
"#,
        child = child.display()
    );

    let (result, _) = run_subflow(&parent, app_with_button());
    assert!(
        result.is_err(),
        "child's root anchor should have been cleaned up, but parent succeeded: {result:?}"
    );
}

/// Two sequential subflows both mounting the same stable anchor name do not
/// interfere: the second subflow gets a fresh mount, not a stale handle from
/// the first.
#[test]
fn sequential_subflows_with_same_stable_anchor() {
    let child = write_temp(
        "seq_child",
        r#"
name: child
anchors:
  app:
    type: Root
    selector: "[name=App]"
  btn:
    type: Stable
    parent: app
    selector: ">> [role=button]"
phases:
  - name: use_btn
    mount: [app, btn]
    steps:
      - intent: verify button accessible
        action: { type: NoOp }
        expect:
          type: ElementFound
          scope: btn
          selector: "*"
"#,
    );

    let parent = format!(
        r#"
name: parent
phases:
  - name: first
    subflow: {child}
  - name: second
    subflow: {child}
"#,
        child = child.display()
    );

    let (result, _) = run_subflow(&parent, app_with_button());
    assert!(
        result.is_ok(),
        "second subflow failed after first cleaned up: {result:?}"
    );
}

/// Three levels each mount the same-named stable anchor `btn`.
/// They are stored as `btn` / `:btn` / `::btn` respectively.
/// After each subflow exits its depth-scoped slot is cleaned up,
/// while shallower slots survive. Parent can use `btn` throughout.
#[test]
fn stable_anchor_isolated_at_depth_2() {
    // Grandchild (depth 2): mounts btn as `::btn`, uses it, exits → cleanup removes `::btn`.
    let grandchild = write_temp(
        "depth2_grandchild",
        r#"
name: grandchild
anchors:
  app:
    type: Root
    selector: "[name=App]"
  btn:
    type: Stable
    parent: app
    selector: ">> [role=button]"
phases:
  - name: use_btn
    mount: [app, btn]
    steps:
      - intent: verify btn accessible at depth 2
        action: { type: NoOp }
        expect:
          type: ElementFound
          scope: btn
          selector: "*"
"#,
    );

    // Child (depth 1): mounts btn as `:btn`, calls grandchild, then re-uses `:btn`
    // after grandchild exits to confirm `::btn` cleanup didn't remove `:btn`.
    let child = write_temp(
        "depth2_child",
        &format!(
            r#"
name: child
anchors:
  app:
    type: Root
    selector: "[name=App]"
  btn:
    type: Stable
    parent: app
    selector: ">> [role=button]"
phases:
  - name: before_gc
    mount: [app, btn]
    steps:
      - intent: verify btn accessible at depth 1 before grandchild
        action: {{ type: NoOp }}
        expect:
          type: ElementFound
          scope: btn
          selector: "*"
  - name: call_grandchild
    subflow: {gc}
  - name: after_gc
    steps:
      - intent: verify btn still accessible at depth 1 after grandchild
        action: {{ type: NoOp }}
        expect:
          type: ElementFound
          scope: btn
          selector: "*"
"#,
            gc = grandchild.display()
        ),
    );

    // Parent (depth 0): mounts btn as `btn`, calls child, then re-uses `btn`
    // after child exits to confirm `:btn` cleanup didn't remove `btn`.
    let parent = format!(
        r#"
name: parent
anchors:
  app:
    type: Root
    selector: "[name=App]"
  btn:
    type: Stable
    parent: app
    selector: ">> [role=button]"
phases:
  - name: before_child
    mount: [app, btn]
    steps:
      - intent: verify btn accessible at depth 0 before child
        action: {{ type: NoOp }}
        expect:
          type: ElementFound
          scope: btn
          selector: "*"
  - name: call_child
    subflow: {child}
  - name: after_child
    steps:
      - intent: verify btn still accessible at depth 0 after child
        action: {{ type: NoOp }}
        expect:
          type: ElementFound
          scope: btn
          selector: "*"
"#,
        child = child.display()
    );

    let (result, _) = run_subflow(&parent, app_with_button());
    assert!(
        result.is_ok(),
        "stable anchor broken across 3 depths: {result:?}"
    );
}

/// Multiple levels of nesting work correctly.
#[test]
fn nested_subflows() {
    let grandchild = write_temp(
        "grandchild",
        r#"
name: grandchild
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: gc_extract
    mount: [app]
    steps:
      - intent: extract from grandchild
        action:
          type: Extract
          key: gc_result
          scope: app
          selector: ">> [role=button]"
          attribute: name
          multiple: false
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button]"
"#,
    );

    let child = write_temp(
        "nested_child",
        &format!(
            r#"
name: child
phases:
  - name: call_grandchild
    subflow: {gc}
"#,
            gc = grandchild.display()
        ),
    );

    let parent = format!(
        r#"
name: parent
phases:
  - name: call_child
    subflow: {child}
"#,
        child = child.display()
    );

    let (result, _) = run_subflow(&parent, app_with_button());
    let state = result.expect("nested subflow failed");
    // Output extracted three levels deep should reach the top.
    assert_eq!(state.output.get("gc_result"), &["Save"]);
}

#[test]
fn parse_simulator_save_operation() {
    let s = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../workflows/mastercam/simulator_save_operation.yml"
    ))
    .unwrap();
    WorkflowFile::load_from_str(&s, &HashMap::new()).unwrap_or_else(|e| panic!("{e}"));
}
