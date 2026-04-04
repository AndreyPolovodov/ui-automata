use schemars::schema_for;
use ui_automata::yaml::WorkflowFile;

/// The schema generator must not panic and must produce a non-trivial schema.
#[test]
fn schema_generates_for_workflow_file() {
    let schema = schema_for!(WorkflowFile);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.len() > 100, "schema seems too small");
    assert!(
        json.contains("\"name\""),
        "schema should contain 'name' field"
    );
    assert!(
        json.contains("\"phases\""),
        "schema should contain 'phases'"
    );
}

/// Condition schema must list all variant type names.
#[test]
fn condition_schema_has_all_variants() {
    use ui_automata::Condition;
    let schema = schema_for!(Condition);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    for variant in &[
        "ElementFound",
        "ElementEnabled",
        "ElementVisible",
        "ElementHasText",
        "ElementHasChildren",
        "WindowWithAttribute",
        "WindowClosed",
        "DialogPresent",
        "DialogAbsent",
        "ForegroundIsDialog",
        "AllOf",
        "AnyOf",
        "Not",
    ] {
        assert!(
            json.contains(variant),
            "condition schema missing variant {variant}"
        );
    }
}

/// RetryPolicy schema must cover all three forms.
#[test]
fn retry_policy_schema_has_all_variants() {
    use ui_automata::RetryPolicy;
    let schema = schema_for!(RetryPolicy);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("\"none\""), "missing 'none' variant");
    assert!(
        json.contains("\"with_recovery\""),
        "missing 'with_recovery' variant"
    );
    assert!(json.contains("\"fixed\""), "missing 'fixed' variant");
}
