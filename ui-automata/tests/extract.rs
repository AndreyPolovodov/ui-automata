mod common;
use common::*;
use ui_automata::mock::mock_desktop_from_yaml;

fn extract_yaml(key: &str, selector: &str, attribute: &str, multiple: bool) -> String {
    format!(
        r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: extract
    mount: [app]
    steps:
      - intent: extract value
        action:
          type: Extract
          key: {key}
          scope: app
          selector: "{selector}"
          attribute: {attribute}
          multiple: {multiple}
        expect:
          type: ElementFound
          scope: app
          selector: "{selector}"
"#
    )
}

/// `attribute: name` stores the element's UIA Name property.
#[test]
fn extract_name_attribute() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: button
    name: Save
"#,
    );
    let yaml = extract_yaml("btn", ">> [role=button][name=Save]", "name", false);
    let (result, _, output) = run_full(&yaml, desktop);

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(output.get("btn"), &["Save"]);
}

/// `attribute: text` is distinct from `name` when the element has a separate text value.
#[test]
fn extract_text_attribute() {
    let desktop_yaml = r#"
role: window
name: App
children:
  - role: edit
    name: field_label
    text: current value
"#;
    let yaml_name = extract_yaml("v", ">> [role=edit]", "name", false);
    let yaml_text = extract_yaml("v", ">> [role=edit]", "text", false);

    let (_, _, out_name) = run_full(&yaml_name, mock_desktop_from_yaml(desktop_yaml));
    let (_, _, out_text) = run_full(&yaml_text, mock_desktop_from_yaml(desktop_yaml));

    assert_eq!(out_name.get("v"), &["field_label"]);
    assert_eq!(out_text.get("v"), &["current value"]);
}

/// `attribute: inner_text` collects the element's own name plus all descendants' names.
#[test]
fn extract_inner_text_attribute() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: tool tip
    name: tip
    children:
      - role: text
        name: "Size: 42 bytes"
      - role: text
        name: "Modified: today"
"#,
    );
    let yaml = extract_yaml("tooltip", ">> [role='tool tip']", "inner_text", false);
    let (result, _, output) = run_full(&yaml, desktop);

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        output.get("tooltip"),
        &["tip
Size: 42 bytes
Modified: today"],
    );
}

/// `attribute: inner_text` includes child content that `name` does not.
#[test]
fn extract_inner_text_includes_children_that_name_does_not() {
    let desktop_yaml = r#"
role: window
name: App
children:
  - role: tool tip
    name: container
    children:
      - role: text
        name: child content
"#;
    let yaml_name = extract_yaml("v", ">> [role='tool tip']", "name", false);
    let yaml_inner = extract_yaml("v", ">> [role='tool tip']", "inner_text", false);

    let (_, _, out_name) = run_full(&yaml_name, mock_desktop_from_yaml(desktop_yaml));
    let (_, _, out_inner) = run_full(&yaml_inner, mock_desktop_from_yaml(desktop_yaml));

    assert_eq!(out_name.get("v"), &["container"]);
    let inner = &out_inner.get("v")[0];
    assert!(inner.contains("container"), "{inner:?}");
    assert!(inner.contains("child content"), "{inner:?}");
    assert_ne!(out_name.get("v"), out_inner.get("v"));
}

/// `multiple: false` extracts only the first matching element.
#[test]
fn extract_single_first_match_only() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: button
    name: First
  - role: button
    name: Second
  - role: button
    name: Third
"#,
    );
    let yaml = extract_yaml("btn", ">> [role=button]", "name", false);
    let (result, _, output) = run_full(&yaml, desktop);

    assert!(result.is_ok(), "{result:?}");
    let values = output.get("btn");
    assert_eq!(values.len(), 1, "only first match extracted: {values:?}");
    assert_eq!(values[0], "First");
}

/// `multiple: true` extracts every matching element in document order.
#[test]
fn extract_multiple_all_matches() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: button
    name: Alpha
  - role: button
    name: Beta
  - role: button
    name: Gamma
"#,
    );
    let yaml = extract_yaml("btns", ">> [role=button]", "name", true);
    let (result, _, output) = run_full(&yaml, desktop);

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(output.get("btns"), &["Alpha", "Beta", "Gamma"]);
}

/// When the selector finds no elements, no values are pushed and the step succeeds.
#[test]
fn extract_no_match_does_not_error() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: try_extract
    mount: [app]
    steps:
      - intent: extract missing element
        action:
          type: Extract
          key: missing
          scope: app
          selector: ">> [role=button][name=DoesNotExist]"
          attribute: name
          multiple: false
        expect:
          type: DialogAbsent
          scope: app
"#;
    let (result, _, output) = run_full(yaml, desktop);

    assert!(result.is_ok(), "{result:?}");
    assert!(output.get("missing").is_empty());
}

/// Two Extract steps using the same key append values in order.
#[test]
fn extract_same_key_appends() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: button
    name: OK
  - role: button
    name: Cancel
"#,
    );
    let yaml = r#"
name: test
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: extract
    mount: [app]
    steps:
      - intent: extract OK
        action:
          type: Extract
          key: btns
          scope: app
          selector: ">> [role=button][name=OK]"
          attribute: name
          multiple: false
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=OK]"
      - intent: extract Cancel
        action:
          type: Extract
          key: btns
          scope: app
          selector: ">> [role=button][name=Cancel]"
          attribute: name
          multiple: false
        expect:
          type: ElementFound
          scope: app
          selector: ">> [role=button][name=Cancel]"
"#;
    let (result, _, output) = run_full(yaml, desktop);

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(output.get("btns"), &["OK", "Cancel"]);
}
