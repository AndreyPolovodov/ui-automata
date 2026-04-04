use std::collections::HashMap;
use ui_automata::Executor;
use ui_automata::mock::mock_desktop_from_yaml;
use ui_automata::yaml::WorkflowFile;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Run a workflow and return (result, first value of `key` in locals+output).
fn run_extract(
    yaml: &str,
    desktop: ui_automata::mock::MockDesktop,
    key: &str,
) -> (Result<(), ui_automata::AutomataError>, Option<String>) {
    let wf = WorkflowFile::load_from_str(yaml, &HashMap::new()).expect("YAML parse failed");
    let mut executor = Executor::new(desktop);
    match wf.run(&mut executor, None, None) {
        Ok(state) => {
            let val = state
                .locals
                .get(key)
                .cloned()
                .or_else(|| state.output.get(key).first().cloned());
            (Ok(()), val)
        }
        Err(e) => (Err(e), None),
    }
}

// ── basic local extract ───────────────────────────────────────────────────────

/// Extract with local:true stores the element's text value, accessible via {output.key}.
#[test]
fn extract_local_stores_text() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: edit
    name: field_label
    text: hello world
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: read
    mount: [app]
    steps:
      - intent: extract edit value locally
        action:
          type: Extract
          key: captured
          scope: app
          selector: ">> [role=edit]"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=edit]"
"#;
    let (result, val) = run_extract(yaml, desktop, "captured");
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(val.as_deref(), Some("hello world"));
}

/// Default attribute is `text` (ValuePattern → Name fallback).
#[test]
fn extract_default_attribute_is_text() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: edit
    name: field_label
    text: edit_value
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: read
    mount: [app]
    steps:
      - intent: extract default
        action:
          type: Extract
          key: v
          scope: app
          selector: ">> [role=edit]"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=edit]"
"#;
    let (result, val) = run_extract(yaml, desktop, "v");
    assert!(result.is_ok(), "{result:?}");
    // default attribute=text reads the text value, not the label
    assert_eq!(val.as_deref(), Some("edit_value"));
}

/// `attribute: name` reads the UIA Name property instead of ValuePattern text.
#[test]
fn extract_attribute_name() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: edit
    name: field_label
    text: edit_value
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: read
    mount: [app]
    steps:
      - intent: extract name property
        action:
          type: Extract
          key: v
          scope: app
          selector: ">> [role=edit]"
          attribute: name
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=edit]"
"#;
    let (result, val) = run_extract(yaml, desktop, "v");
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(val.as_deref(), Some("field_label"));
}

/// When the selector matches nothing the action fails and no value is set.
#[test]
fn extract_missing_element_does_not_set_value() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
"#,
    );
    let yaml = r#"
name: test
defaults:
  timeout: 100ms
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: read
    mount: [app]
    steps:
      - intent: extract missing
        action:
          type: Extract
          key: v
          scope: app
          selector: ">> [role=edit]"
        on_failure: continue
        expect:
          type: DialogAbsent
          scope: app
"#;
    let (result, val) = run_extract(yaml, desktop, "v");
    assert!(result.is_ok(), "{result:?}");
    assert!(
        val.is_none(),
        "value should not be set when element not found"
    );
}

// ── {output.key} substitution ─────────────────────────────────────────────────

/// A local extract is substituted into `TypeText.text` in a later step.
#[test]
fn output_substituted_in_type_text() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: edit
    name: source
    text: my_value
  - role: edit
    name: target
    text: ""
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: copy
    mount: [app]
    steps:
      - intent: extract source
        action:
          type: Extract
          key: src
          scope: app
          selector: ">> [role=edit][name=source]"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=edit][name=source]"
      - intent: type extracted value into target
        action:
          type: TypeText
          scope: app
          selector: ">> [role=edit][name=target]"
          text: "{output.src}"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=edit][name=target]"
"#;
    let (result, val) = run_extract(yaml, desktop, "src");
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(val.as_deref(), Some("my_value"));
}

/// A local extract is substituted into an `ElementHasText` expect pattern.
#[test]
fn output_substituted_in_expect_pattern() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: edit
    name: source
    text: expected_text
  - role: static text
    name: expected_text
"#,
    );
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
      - intent: extract source value
        action:
          type: Extract
          key: val
          scope: app
          selector: ">> [role=edit][name=source]"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=edit][name=source]"
      - intent: check label matches extracted value
        action:
          type: NoOp
        expect:
          type: ElementHasText
          scope: app
          selector: ">> [role='static text']"
          pattern:
            exact: "{output.val}"
"#;
    let (result, _) = run_extract(yaml, desktop, "val");
    assert!(result.is_ok(), "{result:?}");
}

/// Locals are visible across phases — extracted in phase 1, used in phase 2.
#[test]
fn locals_persist_across_phases() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: edit
    name: source
    text: cross_phase_value
  - role: edit
    name: target
    text: ""
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: phase_one
    mount: [app]
    steps:
      - intent: extract in phase one
        action:
          type: Extract
          key: x
          scope: app
          selector: ">> [role=edit][name=source]"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=edit][name=source]"
  - name: phase_two
    mount: [app]
    steps:
      - intent: use extracted value in phase two
        action:
          type: TypeText
          scope: app
          selector: ">> [role=edit][name=target]"
          text: "{output.x}"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=edit][name=target]"
"#;
    let (result, val) = run_extract(yaml, desktop, "x");
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(val.as_deref(), Some("cross_phase_value"));
}

/// A later local Extract with the same key overwrites the previous value.
#[test]
fn local_extract_overwrites_existing() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: edit
    name: first
    text: first_value
  - role: edit
    name: second
    text: second_value
"#,
    );
    let yaml = r#"
name: test
outputs: []
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: overwrite
    mount: [app]
    steps:
      - intent: first extract
        action:
          type: Extract
          key: v
          scope: app
          selector: ">> [role=edit][name=first]"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=edit][name=first]"
      - intent: second extract overwrites
        action:
          type: Extract
          key: v
          scope: app
          selector: ">> [role=edit][name=second]"
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=edit][name=second]"
"#;
    let (result, val) = run_extract(yaml, desktop, "v");
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(val.as_deref(), Some("second_value"));
}
