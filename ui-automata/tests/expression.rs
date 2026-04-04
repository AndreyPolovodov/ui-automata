mod common;

use std::collections::HashMap;

use ui_automata::mock::{MockDesktop, MockElement, mock_desktop_from_yaml};
use ui_automata::yaml::WorkflowFile;
use ui_automata::{AutomataError, Executor};

// ── helpers ───────────────────────────────────────────────────────────────────

fn desktop_with_text(text: &str) -> MockDesktop {
    MockDesktop::new(vec![MockElement::parent(
        "window",
        "App",
        vec![MockElement::leaf_text("edit", "field", text)],
    )])
}

/// Load the workflow (with optional params) and run it.
/// Returns (locals, output).
fn run_workflow(
    yaml: &str,
    params: HashMap<String, String>,
    desktop: MockDesktop,
) -> Result<(HashMap<String, String>, ui_automata::Output), AutomataError> {
    let wf = WorkflowFile::load_from_str(yaml, &params).expect("YAML parse failed");
    let mut executor = Executor::new(desktop);
    wf.run(&mut executor, None, None)
        .map(|state| (state.locals, state.output))
}

fn no_params() -> HashMap<String, String> {
    HashMap::new()
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Extract a numeric string from the UI, then add a constant via Eval.
#[test]
fn eval_add_constant_to_extracted_value() {
    let desktop = desktop_with_text("10");
    let yaml = r#"
name: test
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: compute
    mount: [app]
    steps:
      - intent: extract the number
        action:
          type: Extract
          key: raw
          scope: app
          selector: ">> [role=edit]"
        expect: { type: Always }
      - intent: add 5
        action:
          type: Eval
          key: result
          expr: "raw + 5"
        expect: { type: Always }
"#;
    let (locals, _) = run_workflow(yaml, no_params(), desktop).unwrap();
    assert_eq!(locals.get("result").map(String::as_str), Some("15"));
}

/// Eval can concatenate a string when one operand is not numeric.
#[test]
fn eval_string_concat_with_extracted_value() {
    let desktop = desktop_with_text("world");
    let yaml = r#"
name: test
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: compute
    mount: [app]
    steps:
      - intent: extract word
        action:
          type: Extract
          key: word
          scope: app
          selector: ">> [role=edit]"
        expect: { type: Always }
      - intent: prepend greeting
        action:
          type: Eval
          key: greeting
          expr: "'hello ' + word"
        expect: { type: Always }
"#;
    let (locals, _) = run_workflow(yaml, no_params(), desktop).unwrap();
    assert_eq!(
        locals.get("greeting").map(String::as_str),
        Some("hello world")
    );
}

/// A workflow param is used as a multiplier in the expression.
#[test]
fn eval_uses_param_as_operand() {
    let desktop = desktop_with_text("6");
    let yaml = r#"
name: test
params:
  - name: factor
    default: "1"
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: compute
    mount: [app]
    steps:
      - intent: extract number
        action:
          type: Extract
          key: raw
          scope: app
          selector: ">> [role=edit]"
        expect: { type: Always }
      - intent: multiply by param
        action:
          type: Eval
          key: result
          expr: "raw * param.factor"
        expect: { type: Always }
"#;
    let mut params = HashMap::new();
    params.insert("factor".into(), "7".into());
    let (locals, _) = run_workflow(yaml, params, desktop).unwrap();
    assert_eq!(locals.get("result").map(String::as_str), Some("42"));
}

/// Eval result stored in locals is accessible as a bare identifier in a subsequent step.
#[test]
fn eval_result_accessible_in_subsequent_step() {
    let desktop = mock_desktop_from_yaml(
        r#"
role: window
name: App
children:
  - role: edit
    name: field
    text: "3"
  - role: text
    name: target
    text: ""
"#,
    );
    let yaml = r#"
name: test
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: compute
    mount: [app]
    steps:
      - intent: extract base
        action:
          type: Extract
          key: base
          scope: app
          selector: ">> [role=edit]"
        expect: { type: Always }
      - intent: compute square
        action:
          type: Eval
          key: squared
          expr: "base * base"
        expect: { type: Always }
      - intent: verify computed value is reachable via output substitution
        action:
          type: NoOp
        expect:
          type: ElementHasText
          scope: app
          selector: ">> [role=edit]"
          pattern:
            exact: "{output.base}"
"#;
    let (locals, _) = run_workflow(yaml, no_params(), desktop).unwrap();
    assert_eq!(locals.get("squared").map(String::as_str), Some("9"));
}

/// split_lines extracts the last line from a multi-line extracted value.
#[test]
fn eval_split_lines_on_extracted_multiline() {
    let desktop = desktop_with_text("line0\nline1\nresult");
    let yaml = r#"
name: test
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: compute
    mount: [app]
    steps:
      - intent: extract text block
        action:
          type: Extract
          key: block
          scope: app
          selector: ">> [role=edit]"
        expect: { type: Always }
      - intent: pick last line
        action:
          type: Eval
          key: last_line
          expr: "split_lines(block, -1)"
        expect: { type: Always }
"#;
    let (locals, _) = run_workflow(yaml, no_params(), desktop).unwrap();
    assert_eq!(locals.get("last_line").map(String::as_str), Some("result"));
}

/// round() converts a decimal extracted value to a whole number string.
#[test]
fn eval_round_extracted_decimal() {
    let desktop = desktop_with_text("3.75");
    let yaml = r#"
name: test
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: compute
    mount: [app]
    steps:
      - intent: extract decimal
        action:
          type: Extract
          key: raw
          scope: app
          selector: ">> [role=edit]"
        expect: { type: Always }
      - intent: round it
        action:
          type: Eval
          key: rounded
          expr: "round(raw)"
        expect: { type: Always }
"#;
    let (locals, _) = run_workflow(yaml, no_params(), desktop).unwrap();
    assert_eq!(locals.get("rounded").map(String::as_str), Some("4"));
}

/// Chained Evals: first computes an intermediate, second builds on it.
#[test]
fn eval_chained_computations() {
    let desktop = desktop_with_text("5");
    let yaml = r#"
name: test
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: compute
    mount: [app]
    steps:
      - intent: extract base
        action:
          type: Extract
          key: base
          scope: app
          selector: ">> [role=edit]"
        expect: { type: Always }
      - intent: double it
        action:
          type: Eval
          key: doubled
          expr: "base * 2"
        expect: { type: Always }
      - intent: add 1 to doubled
        action:
          type: Eval
          key: final
          expr: "doubled + 1"
        expect: { type: Always }
"#;
    let (locals, _) = run_workflow(yaml, no_params(), desktop).unwrap();
    assert_eq!(locals.get("doubled").map(String::as_str), Some("10"));
    assert_eq!(locals.get("final").map(String::as_str), Some("11"));
}

/// Sum 1..=100 using a loop built from flow_control phases and Eval actions.
///
/// Equivalent Python:
///   total = 0
///   for i in range(1, 101):
///       total += i
///   print(total)  # 5050
#[test]
fn eval_sum_1_to_100_with_flow_control_loop() {
    let desktop = MockDesktop::new(vec![]);
    let yaml = r#"
name: sum_loop
phases:
  - name: init
    steps:
      - intent: initialise counter
        action:
          type: Eval
          key: i
          expr: "1"
        expect:
          type: Always
      - intent: initialise total
        action:
          type: Eval
          key: total
          expr: "0"
        expect:
          type: Always

  - name: check
    flow_control:
      condition:
        type: EvalCondition
        expr: "i > 100"
      go_to: done

  - name: loop_body
    steps:
      - intent: accumulate
        action:
          type: Eval
          key: total
          expr: "total + i"
        expect:
          type: Always
      - intent: increment counter
        action:
          type: Eval
          key: i
          expr: "i + 1"
        expect:
          type: Always

  - name: loop_back
    flow_control:
      condition:
        type: Always
      go_to: check

  - name: done
    steps:
      - intent: record result into output buffer
        action:
          type: Eval
          key: total
          expr: "total"
          output: result
        expect:
          type: Always
"#;
    let (locals, output) = run_workflow(yaml, no_params(), desktop).unwrap();
    assert_eq!(locals.get("total").map(String::as_str), Some("5050"));
    assert_eq!(
        output.get("result").first().map(String::as_str),
        Some("5050")
    );
}
