mod common;
use common::*;
use ui_automata::mock::mock_desktop_from_yaml;

// ── ItemSelected condition ────────────────────────────────────────────────────

/// ItemSelected with no `state` returns false when element has no SelectionItemPattern.
/// We use `Not(ItemSelected)` as expect — if ItemSelected returns false, Not returns true, step passes.
#[test]
fn item_selected_no_pattern_returns_false() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: radio button
    name: Option A
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
      - intent: verify not selected (no SelectionItemPattern)
        action:
          type: NoOp
        expect:
          type: Not
          condition:
            type: ElementItemSelected
            scope: app
            selector: ">> [role='radio button'][name='Option A']"
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// ItemSelected with `state: true` returns true when mock element is selected.
#[test]
fn item_selected_true_matches_selected_element() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: radio button
    name: Option A
    is_selected: true
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
      - intent: verify element is selected
        action:
          type: NoOp
        expect:
          type: ElementItemSelected
          scope: app
          selector: ">> [role='radio button'][name='Option A']"
          state: true
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// ItemSelected with `state: false` returns true when element is not selected.
#[test]
fn item_selected_false_matches_unselected_element() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: radio button
    name: Option A
    is_selected: false
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
      - intent: verify element is not selected
        action:
          type: NoOp
        expect:
          type: ElementItemSelected
          scope: app
          selector: ">> [role='radio button'][name='Option A']"
          state: false
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// ItemSelected with `state: true` fails when element is actually not selected.
#[test]
fn item_selected_true_fails_when_not_selected() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: radio button
    name: Option A
    is_selected: false
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
      - intent: should time out
        action:
          type: NoOp
        expect:
          type: ElementItemSelected
          scope: app
          selector: ">> [role='radio button'][name='Option A']"
          state: true
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_err(), "expected timeout error");
}

// ── Selected condition ────────────────────────────────────────────────────────

/// Selected returns false (via Not) when element has no SelectionPattern.
#[test]
fn selected_no_pattern_returns_false() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: combo box
    name: Options
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
      - intent: no SelectionPattern means no match
        action:
          type: NoOp
        expect:
          type: Not
          condition:
            type: ElementSelected
            scope: app
            selector: ">> [role='combo box'][name=Options]"
            pattern:
              non_empty: true
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// Selected matches when element's selection_text satisfies the pattern.
#[test]
fn selected_matches_selection_text() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: combo box
    name: Options
    selection_text: "Option B"
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
      - intent: verify combo selection
        action:
          type: NoOp
        expect:
          type: ElementSelected
          scope: app
          selector: ">> [role='combo box'][name=Options]"
          pattern:
            contains: "Option B"
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// Selected does not match when selection_text differs from pattern.
#[test]
fn selected_does_not_match_wrong_text() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: combo box
    name: Options
    selection_text: "Option A"
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
      - intent: wrong selection does not match
        action:
          type: NoOp
        expect:
          type: Not
          condition:
            type: ElementSelected
            scope: app
            selector: ">> [role='combo box'][name=Options]"
            pattern:
              exact: "Option B"
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

/// Selected with `exact` pattern succeeds when text matches exactly.
#[test]
fn selected_exact_match() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: combo box
    name: Options
    selection_text: "Option B"
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
      - intent: exact match
        action:
          type: NoOp
        expect:
          type: ElementSelected
          scope: app
          selector: ">> [role='combo box'][name=Options]"
          pattern:
            exact: "Option B"
"#;
    let (result, _) = run(yaml, desktop);
    assert!(result.is_ok(), "{result:?}");
}

// ── Select action ─────────────────────────────────────────────────────────────

/// Select action parses and executes; mock returns error (not supported), step fails.
#[test]
fn select_action_not_supported_on_mock() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: combo box
    name: Options
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
  - name: select_phase
    mount: [app]
    steps:
      - intent: select item
        action:
          type: Select
          scope: app
          selector: ">> [role='combo box'][name=Options]"
          value: "Option B"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role='combo box'][name=Options]"
"#;
    let (result, _) = run(yaml, desktop);
    // Mock does not implement select_item, so step should fail
    assert!(result.is_err(), "expected error from mock select_item");
}

// ── Lint tests ────────────────────────────────────────────────────────────────

/// Lint accepts a valid Select step.
#[test]
fn lint_select_valid() {
    use ui_automata::lint::lint;
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: do_select
    mount: [app]
    steps:
      - intent: pick item
        action:
          type: Select
          scope: app
          selector: ">> [role='combo box']"
          value: "Item 1"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role='combo box']"
"#;
    let diags = lint(yaml);
    assert!(diags.is_empty(), "unexpected lint errors: {diags:?}");
}

/// Lint rejects Select step missing the `value` field.
#[test]
fn lint_select_missing_value() {
    use ui_automata::lint::lint;
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: do_select
    mount: [app]
    steps:
      - intent: pick item
        action:
          type: Select
          scope: app
          selector: ">> [role='combo box']"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role='combo box']"
"#;
    let diags = lint(yaml);
    assert!(
        diags.iter().any(|d| d.message.contains("value")),
        "expected lint error about missing 'value': {diags:?}"
    );
}

/// Lint accepts ItemSelected step with explicit state.
#[test]
fn lint_item_selected_valid() {
    use ui_automata::lint::lint;
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: verify
    mount: [app]
    steps:
      - intent: verify selection
        action:
          type: NoOp
        expect:
          type: ElementItemSelected
          scope: app
          selector: ">> [role='radio button']"
          state: true
"#;
    let diags = lint(yaml);
    assert!(diags.is_empty(), "unexpected lint errors: {diags:?}");
}

/// Lint accepts Selected step.
#[test]
fn lint_selected_valid() {
    use ui_automata::lint::lint;
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: verify
    mount: [app]
    steps:
      - intent: verify combo selection
        action:
          type: NoOp
        expect:
          type: ElementSelected
          scope: app
          selector: ">> [role='combo box']"
          pattern:
            contains: "Option"
"#;
    let diags = lint(yaml);
    assert!(diags.is_empty(), "unexpected lint errors: {diags:?}");
}
