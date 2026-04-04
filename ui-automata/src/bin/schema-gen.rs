//! Generates the JSON Schema for `WorkflowFile` and writes it to `workflow-schema.json`.
//!
//! Run from the workspace root:
//! ```
//! cargo run --bin schema-gen
//! ```
use schemars::schema_for;
use std::path::Path;
use ui_automata::yaml::WorkflowFile;

fn main() {
    let schema = schema_for!(WorkflowFile);
    let json = serde_json::to_string_pretty(&schema).expect("failed to serialize schema");
    let out = Path::new("workflow-schema.json");
    std::fs::write(out, &json).expect("failed to write schema file");
    println!("Schema written to {}", out.display());
}
