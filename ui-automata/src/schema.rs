//! Manual `JsonSchema` implementations for types that use custom serde deserializers.
use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde_json::to_value;

use crate::{
    Condition, RetryPolicy, SelectorPath,
    condition::{TextMatch, TitleMatch},
};

// ── Duration helpers ──────────────────────────────────────────────────────────

/// Schema for a `Duration` field serialized as a string like `"5s"` or `"300ms"`.
/// Use as `#[schemars(schema_with = "crate::schema::duration_schema")]`.
pub fn duration_schema(_sg: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "string",
        "description": "Duration string, e.g. \"5s\", \"300ms\", \"2m\", \"1h\""
    })
}

// ── SelectorPath ──────────────────────────────────────────────────────────────

impl JsonSchema for SelectorPath {
    fn schema_name() -> Cow<'static, str> {
        "SelectorPath".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_sg: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "CSS-like path for navigating the UIA element tree.\n\nSyntax: [Combinator] Step [Combinator Step]*\n  Step      = [attr Op value]+ [:nth(n)] [:parent | :ancestor(n)]\n  Combinator = \">\" (direct child) | \">>\" (any descendant)\n  attr      = role | name | title | id\n  Op        = \"=\" exact | \"~=\" contains | \"^=\" starts-with | \"$=\" ends-with\n  :parent         navigate to the matched element's parent\n  :ancestor(n)    navigate n levels up (1 = parent)\n\nNo leading combinator: first step matches the scope root element itself.\nLeading >> or >: searches inside the scope root without re-matching it (use when scope IS the container).\n\nExamples:\n  \"[name~=Notepad]\"                                     root match by title substring\n  \">> [role=button][name=Close]\"                        any descendant button\n  \">> [role=title bar] > [role=button]\"                 child of a descendant\n  \">> [role=group]:nth(1)\"                              second group child (0-indexed)\n  \">> [role=button][name^=Don][name$=Save]\"             starts/ends-with for special chars\n  \">> [role=button][name=Performance]:parent > *:nth(9)\" 9th sibling of Performance"
        })
    }
}

// ── Condition ─────────────────────────────────────────────────────────────────

impl JsonSchema for Condition {
    fn schema_name() -> Cow<'static, str> {
        "Condition".into()
    }

    fn json_schema(sg: &mut SchemaGenerator) -> Schema {
        use serde_json::json;

        let text_match = to_value(sg.subschema_for::<TextMatch>()).unwrap();
        let title_match = to_value(sg.subschema_for::<TitleMatch>()).unwrap();
        let cond_ref = to_value(sg.subschema_for::<Condition>()).unwrap();
        let cond_arr = to_value(sg.subschema_for::<Vec<Condition>>()).unwrap();

        let mut variants: Vec<serde_json::Value> = Vec::new();

        let scope_s = || json!({ "type": "string", "description": "Anchor name to resolve the element tree from." });
        let selector_s =
            || json!({ "type": "string", "description": "Selector path within the scope anchor." });
        let anchor_s = || json!({ "type": "string", "description": "Name of the anchor whose window is tracked." });

        // scope + selector variants
        for (type_name, desc) in &[
            (
                "ElementFound",
                "True when the selector matches at least one live element under the scope anchor.",
            ),
            (
                "ElementEnabled",
                "True when the matched element is not greyed out (UIA IsEnabled).",
            ),
            (
                "ElementVisible",
                "True when the matched element is visible on screen (UIA IsOffscreen=false).",
            ),
            (
                "ElementHasChildren",
                "True when the matched element has at least one child element.",
            ),
        ] {
            variants.push(json!({
                "type": "object",
                "description": desc,
                "required": ["type", "scope", "selector"],
                "properties": {
                    "type": { "const": type_name },
                    "scope": scope_s(),
                    "selector": selector_s()
                },
                "additionalProperties": false
            }));
        }

        // ElementHasText — adds `pattern: TextMatch`
        variants.push(json!({
            "type": "object",
            "description": "True when the matched element's text value satisfies the pattern.",
            "required": ["type", "scope", "selector", "pattern"],
            "properties": {
                "type": { "const": "ElementHasText" },
                "scope": scope_s(),
                "selector": selector_s(),
                "pattern": text_match
            },
            "additionalProperties": false
        }));

        // WindowWithAttribute — title (TitleMatch) + automation_id + pid + process
        variants.push(json!({
            "type": "object",
            "description": "True when any open application window matches all specified attributes. Requires at least one of: title, automation_id, pid. `process` is an optional filter.",
            "required": ["type"],
            "properties": {
                "type": { "const": "WindowWithAttribute" },
                "title":          title_match.clone(),
                "automation_id":  { "type": "string", "description": "UIA AutomationId must match exactly." },
                "pid":            { "type": "integer", "minimum": 0, "description": "Process ID to match exactly." },
                "process":        { "type": "string", "description": "Optional: restrict to windows belonging to this process (name without .exe, case-insensitive)." }
            },
            "additionalProperties": false
        }));

        // ProcessRunning
        variants.push(json!({
            "type": "object",
            "description": "True when any application window belongs to a process whose name (without .exe) matches, case-insensitive.",
            "required": ["type", "process"],
            "properties": {
                "type": { "const": "ProcessRunning" },
                "process": { "type": "string", "description": "Process name without .exe (e.g. \"notepad\")." }
            },
            "additionalProperties": false
        }));

        // WindowWithState
        variants.push(json!({
            "type": "object",
            "description": "True when the anchor's window is in the given state. Use after ActivateWindow (active) or to confirm a window is not minimized (visible).",
            "required": ["type", "anchor", "state"],
            "properties": {
                "type": { "const": "WindowWithState" },
                "anchor": anchor_s(),
                "state": {
                    "type": "string",
                    "enum": ["active", "visible"],
                    "description": "active: a window belonging to the same process as the anchor is the OS foreground window. visible: the anchor's window is visible on screen (not minimized or hidden)."
                }
            },
            "additionalProperties": false
        }));

        // WindowClosed — uses `anchor` not `scope`
        variants.push(json!({
            "type": "object",
            "description": "True when the anchor's window is no longer present. HWND-locked anchors check that specific window handle; PID-only anchors check for any window of that process; unresolved anchors treat re-resolution failure as closed.",
            "required": ["type", "anchor"],
            "properties": {
                "type": { "const": "WindowClosed" },
                "anchor": anchor_s()
            },
            "additionalProperties": false
        }));

        // scope-only variants
        for (type_name, desc) in &[
            (
                "DialogPresent",
                "True when a direct child of the scope anchor's window has role=\"dialog\".",
            ),
            (
                "DialogAbsent",
                "True when no direct child of the scope anchor's window has role=\"dialog\".",
            ),
        ] {
            variants.push(json!({
                "type": "object",
                "description": desc,
                "required": ["type", "scope"],
                "properties": {
                    "type": { "const": type_name },
                    "scope": scope_s()
                },
                "additionalProperties": false
            }));
        }

        // ForegroundIsDialog — scope is parsed but ignored at runtime; only fg window role/title matter
        variants.push(json!({
            "type": "object",
            "description": "True when the OS foreground window has role=dialog. Optionally also checks the dialog title.",
            "required": ["type"],
            "properties": {
                "type": { "const": "ForegroundIsDialog" },
                "title": title_match
            },
            "additionalProperties": false
        }));

        // ExecSucceeded — no fields required
        variants.push(json!({
            "type": "object",
            "description": "True when the most recent Exec action exited with code 0.",
            "required": ["type"],
            "properties": {
                "type": { "const": "ExecSucceeded" }
            },
            "additionalProperties": false
        }));

        // EvalCondition — evaluates a boolean expression against outputs/locals/params
        variants.push(json!({
            "type": "object",
            "description": "Evaluates a boolean expression against the current outputs, locals, and params. The expression must return a Bool (use a comparison operator), e.g. \"output.count != '0'\".",
            "required": ["type", "expr"],
            "properties": {
                "type": { "const": "EvalCondition" },
                "expr": { "type": "string", "description": "Boolean expression to evaluate." }
            },
            "additionalProperties": false
        }));

        // FileExists — checks whether a path exists on disk
        variants.push(json!({
            "type": "object",
            "description": "True when the file at `path` exists on disk. `path` supports `{output.*}` substitution.",
            "required": ["type", "path"],
            "properties": {
                "type": { "const": "FileExists" },
                "path": { "type": "string", "description": "File path to check. Supports `{output.*}` substitution." }
            },
            "additionalProperties": false
        }));

        // Always — no fields required
        variants.push(json!({
            "type": "object",
            "description": "Always evaluates to true immediately. Use as `expect` on steps where success is guaranteed by the action (e.g. Eval, WriteOutput, NoOp).",
            "required": ["type"],
            "properties": {
                "type": { "const": "Always" }
            },
            "additionalProperties": false
        }));

        // AllOf / AnyOf — array of conditions
        for (type_name, desc) in &[
            (
                "AllOf",
                "Short-circuit AND: true when every sub-condition is true.",
            ),
            (
                "AnyOf",
                "Short-circuit OR: true when at least one sub-condition is true.",
            ),
        ] {
            variants.push(json!({
                "type": "object",
                "description": desc,
                "required": ["type", "conditions"],
                "properties": {
                    "type": { "const": type_name },
                    "conditions": cond_arr.clone()
                },
                "additionalProperties": false
            }));
        }

        // Not — single nested condition
        variants.push(json!({
            "type": "object",
            "description": "Negation: true when the inner condition is false.",
            "required": ["type", "condition"],
            "properties": {
                "type": { "const": "Not" },
                "condition": cond_ref
            },
            "additionalProperties": false
        }));

        // TabWithAttribute — checks title/url of a browser tab
        variants.push(json!({
            "type": "object",
            "description": "True when the browser tab anchored to `scope` matches all specified attribute filters. Requires at least one of: title, url.",
            "required": ["type", "scope"],
            "properties": {
                "type": { "const": "TabWithAttribute" },
                "scope": { "type": "string", "description": "Name of a mounted Tab anchor." },
                "title": text_match.clone(),
                "url": text_match.clone()
            },
            "additionalProperties": false
        }));

        // TabWithState — evaluates a JS expression in a browser tab; true only when result == "true"
        variants.push(json!({
            "type": "object",
            "description": "True when the JS expression `expr` evaluates to the string \"true\" in the browser tab anchored to `scope`. The expression must return a boolean, e.g. `document.readyState === 'complete'`.",
            "required": ["type", "scope", "expr"],
            "properties": {
                "type": { "const": "TabWithState" },
                "scope": { "type": "string", "description": "Name of a mounted Tab anchor." },
                "expr": { "type": "string", "description": "JS expression to evaluate in the tab. Must return a boolean — only the string \"true\" is treated as passing." }
            },
            "additionalProperties": false
        }));

        json!({ "oneOf": variants }).try_into().unwrap()
    }
}

// ── RetryPolicy ───────────────────────────────────────────────────────────────

impl JsonSchema for RetryPolicy {
    fn schema_name() -> Cow<'static, str> {
        "RetryPolicy".into()
    }

    fn json_schema(_sg: &mut SchemaGenerator) -> Schema {
        use serde_json::json;
        json!({
            "oneOf": [
                {
                    "const": "none",
                    "description": "On a step: inherit the workflow-level `defaults.retry` policy. On `defaults.retry` itself: disable retries — steps fail immediately on timeout."
                },
                {
                    "const": "with_recovery",
                    "description": "Opts out of fixed retries for this step — the phase default retry policy does not apply. Recovery handlers still fire as normal on timeout. Fails immediately when no handler matches."
                },
                {
                    "type": "object",
                    "description": "Retry a fixed number of times with a constant delay between attempts.",
                    "required": ["fixed"],
                    "properties": {
                        "fixed": {
                            "type": "object",
                            "required": ["count", "delay"],
                            "properties": {
                                "count": { "type": "integer", "minimum": 1, "description": "Number of additional attempts after the first failure." },
                                "delay": { "type": "string", "description": "Wait between retries, e.g. \"300ms\" or \"2s\"." }
                            },
                            "additionalProperties": false
                        }
                    },
                    "additionalProperties": false
                }
            ]
        }).try_into().unwrap()
    }
}
