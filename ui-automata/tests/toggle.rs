mod common;
use common::*;
use ui_automata::mock::mock_desktop_from_yaml;

// ── ElementToggled condition ──────────────────────────────────────────────────

/// ElementToggled with no `state` passes for any toggle state.
#[test]
fn element_checked_no_state_passes_for_any_toggle() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: check box
    name: Option
    toggle_state: true
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
defaults:
  timeout: 2s
phases:
  - name: check
    mount: [app]
    steps:
      - intent: any toggle state passes when state omitted
        action:
          type: NoOp
        expect:
          type: ElementToggled
          scope: app
          selector: ">> [role='check box'][name=Option]"
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// ElementToggled `state: true` matches a checked element.
#[test]
fn element_checked_true_matches_on() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: check box
    name: Option
    toggle_state: true
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
defaults:
  timeout: 2s
phases:
  - name: check
    mount: [app]
    steps:
      - intent: verify checked
        action:
          type: NoOp
        expect:
          type: ElementToggled
          scope: app
          selector: ">> [role='check box'][name=Option]"
          state: true
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// ElementToggled `state: false` matches an unchecked element.
#[test]
fn element_checked_false_matches_off() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: check box
    name: Option
    toggle_state: false
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
defaults:
  timeout: 2s
phases:
  - name: check
    mount: [app]
    steps:
      - intent: verify unchecked
        action:
          type: NoOp
        expect:
          type: ElementToggled
          scope: app
          selector: ">> [role='check box'][name=Option]"
          state: false
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// ElementToggled `state: "indeterminate"` matches an indeterminate element.
#[test]
fn element_checked_indeterminate_matches() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: check box
    name: Option
    toggle_state: "indeterminate"
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
defaults:
  timeout: 2s
phases:
  - name: check
    mount: [app]
    steps:
      - intent: verify indeterminate
        action:
          type: NoOp
        expect:
          type: ElementToggled
          scope: app
          selector: ">> [role='check box'][name=Option]"
          state: "indeterminate"
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// ElementToggled `state: true` fails when element is in indeterminate state.
#[test]
fn element_checked_true_fails_when_indeterminate() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: check box
    name: Option
    toggle_state: "indeterminate"
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
defaults:
  timeout: 1s
phases:
  - name: check
    mount: [app]
    steps:
      - intent: should time out — indeterminate != true
        action:
          type: NoOp
        expect:
          type: ElementToggled
          scope: app
          selector: ">> [role='check box'][name=Option]"
          state: true
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_err(), "expected timeout");
}

// ── SetToggle action ──────────────────────────────────────────────────────────

/// SetToggle `state: true` on an already-checked element is a no-op.
#[test]
fn set_toggle_true_already_on_noop() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: check box
    name: CB
    toggle_state: true
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
defaults:
  timeout: 2s
phases:
  - name: toggle
    mount: [app]
    steps:
      - intent: set toggle true (already true)
        action:
          type: SetToggle
          scope: app
          selector: ">> [role='check box'][name=CB]"
          state: true
        expect:
          type: ElementToggled
          scope: app
          selector: ">> [role='check box'][name=CB]"
          state: true
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// SetToggle `state: false` on a checked element toggles it to off.
#[test]
fn set_toggle_false_from_on() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: check box
    name: CB
    toggle_state: true
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
defaults:
  timeout: 2s
phases:
  - name: toggle
    mount: [app]
    steps:
      - intent: set toggle false (currently true)
        action:
          type: SetToggle
          scope: app
          selector: ">> [role='check box'][name=CB]"
          state: false
        expect:
          type: ElementToggled
          scope: app
          selector: ">> [role='check box'][name=CB]"
          state: false
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// SetToggle `state: false` on an indeterminate element cycles through On to Off.
/// Indeterminate → On → Off (two toggles).
#[test]
fn set_toggle_false_from_indeterminate() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: check box
    name: CB
    toggle_state: "indeterminate"
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
defaults:
  timeout: 2s
phases:
  - name: toggle
    mount: [app]
    steps:
      - intent: set toggle false from indeterminate (cycles Indeterminate→On→Off)
        action:
          type: SetToggle
          scope: app
          selector: ">> [role='check box'][name=CB]"
          state: false
        expect:
          type: ElementToggled
          scope: app
          selector: ">> [role='check box'][name=CB]"
          state: false
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// SetToggle `state: true` on an indeterminate element — one toggle: Indeterminate→On.
#[test]
fn set_toggle_true_from_indeterminate() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: check box
    name: CB
    toggle_state: "indeterminate"
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
defaults:
  timeout: 2s
phases:
  - name: toggle
    mount: [app]
    steps:
      - intent: set toggle true from indeterminate (one toggle)
        action:
          type: SetToggle
          scope: app
          selector: ">> [role='check box'][name=CB]"
          state: true
        expect:
          type: ElementToggled
          scope: app
          selector: ">> [role='check box'][name=CB]"
          state: true
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// SetToggle on element without TogglePattern returns an error.
#[test]
fn set_toggle_no_pattern_returns_error() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: button
    name: Btn
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
defaults:
  timeout: 1s
phases:
  - name: toggle
    mount: [app]
    steps:
      - intent: SetToggle on element without TogglePattern should fail
        action:
          type: SetToggle
          scope: app
          selector: ">> [role=button][name=Btn]"
          state: true
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=Btn]"
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_err(), "expected error — no TogglePattern");
}
