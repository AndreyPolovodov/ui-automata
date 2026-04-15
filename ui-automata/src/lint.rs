//! Workflow YAML linter.
//!
//! Walks a [`saphyr::MarkedYaml`] tree and collects diagnostics with line/column information:
//! unknown fields, missing required fields, invalid selectors, anchor cross-reference errors,
//! and structural constraint violations (e.g. TextMatch must specify exactly one pattern).
//!
//! Use [`lint`] to check a raw YAML string.

use std::collections::{HashMap, HashSet};

use saphyr::{LoadableYamlNode, MarkedYaml};
use saphyr_parser::Span;

use crate::expression::check_expr_syntax;
use crate::selector::SelectorPath;

// ── Public types ─────────────────────────────────────────────────────────────

/// A single lint diagnostic with source location.
#[derive(Debug, Clone)]
pub struct LintDiag {
    /// 1-based start line, if known.
    pub line: Option<usize>,
    /// 1-based start column, if known.
    pub col: Option<usize>,
    /// 1-based end column on the same line, if known. Used to render `^^^^` underlines.
    pub end_col: Option<usize>,
    /// Dot-bracket path to the offending node, e.g. `phases[2].steps[0].action.selector`.
    pub path: String,
    /// Human-readable description of the problem.
    pub message: String,
}

impl std::fmt::Display for LintDiag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.col) {
            (Some(l), Some(c)) => write!(f, "{}:{} {}: {}", l, c, self.path, self.message),
            (Some(l), None) => write!(f, "{} {}: {}", l, self.path, self.message),
            _ => write!(f, "{}: {}", self.path, self.message),
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Lint a raw YAML string. Returns an empty `Vec` when there are no problems.
pub fn lint(raw: &str) -> Vec<LintDiag> {
    let docs = match MarkedYaml::load_from_str(raw) {
        Ok(docs) => docs,
        Err(e) => {
            let m = e.marker();
            let fake_span = saphyr_parser::Span {
                start: saphyr_parser::Marker::new(0, m.line(), m.col()),
                end: saphyr_parser::Marker::new(0, m.line(), m.col()),
            };
            return vec![diag_at(
                &fake_span,
                "",
                format!("YAML parse error: {}", e.info()),
            )];
        }
    };

    match docs.into_iter().next() {
        None => vec![],
        Some(doc) => {
            let mut diags = Vec::new();
            lint_workflow(&doc, &mut diags);
            diags
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn opt(n: usize) -> Option<usize> {
    if n > 0 { Some(n) } else { None }
}

fn diag_at(span: &Span, path: &str, msg: impl Into<String>) -> LintDiag {
    let end_col = if span.end.line() == span.start.line() {
        opt(span.end.col())
    } else {
        None
    };
    LintDiag {
        line: opt(span.start.line()),
        col: opt(span.start.col()),
        end_col,
        path: path.to_owned(),
        message: msg.into(),
    }
}

/// Get a child node by key.
fn get<'a, 'b: 'a>(node: &'a MarkedYaml<'b>, key: &str) -> Option<&'a MarkedYaml<'b>> {
    node.data.as_mapping_get(key)
}

/// Get a child node as `&str` along with its span.
fn get_str<'a, 'b: 'a>(node: &'a MarkedYaml<'b>, key: &str) -> Option<(&'a str, Span)> {
    get(node, key).and_then(|n| n.data.as_str().map(|s| (s, n.span)))
}

/// Require a string field. Returns the value and its span on success; pushes a diagnostic on failure.
fn require_str<'a, 'b: 'a>(
    node: &'a MarkedYaml<'b>,
    field: &str,
    path: &str,
    diags: &mut Vec<LintDiag>,
) -> Option<(&'a str, Span)> {
    match get_str(node, field) {
        Some(pair) => Some(pair),
        None => {
            diags.push(diag_at(
                &node.span,
                path,
                format!("missing required field '{field}'"),
            ));
            None
        }
    }
}

/// Check a string value for unclosed `{` interpolation tokens and unknown `{param.xxx}` refs.
fn check_interpolation(
    s: &str,
    span: &Span,
    path: &str,
    params: &HashSet<String>,
    diags: &mut Vec<LintDiag>,
) {
    let mut depth: i32 = 0;
    let mut token_start: Option<usize> = None;

    for (i, ch) in s.char_indices() {
        match ch {
            '{' => {
                depth += 1;
                if depth == 1 {
                    token_start = Some(i + 1);
                }
            }
            '}' => {
                if depth > 0 {
                    if depth == 1 {
                        if let Some(start) = token_start.take() {
                            let token = &s[start..i];
                            if let Some(param_name) = token.strip_prefix("param.") {
                                if !params.is_empty() && !params.contains(param_name) {
                                    let known: Vec<&str> =
                                        params.iter().map(|s| s.as_str()).collect();
                                    let mut known = known;
                                    known.sort();
                                    diags.push(diag_at(
                                        span,
                                        path,
                                        format!(
                                            "unknown param '{}' (declared: {})",
                                            param_name,
                                            known.join(", ")
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    depth -= 1;
                }
            }
            _ => {}
        }
    }
    if depth > 0 {
        diags.push(diag_at(span, path, "unclosed '{' in interpolation token"));
    }
}

fn check_selector(node: &MarkedYaml<'_>, field: &str, path: &str, diags: &mut Vec<LintDiag>) {
    if let Some((s, span)) = get_str(node, field) {
        if let Err(e) = SelectorPath::parse(s) {
            diags.push(diag_at(&span, &format!("{path}.{field}"), e.to_string()));
        }
    }
}

fn check_anchor_ref(
    scope: &str,
    span: &Span,
    anchors: &HashSet<String>,
    path: &str,
    field: &str,
    diags: &mut Vec<LintDiag>,
) {
    if !anchors.contains(scope) {
        let mut known: Vec<&str> = anchors.iter().map(|s| s.as_str()).collect();
        known.sort();
        diags.push(diag_at(
            span,
            &format!("{path}.{field}"),
            format!(
                "unknown anchor '{}' (declared: {})",
                scope,
                known.join(", ")
            ),
        ));
    }
}

/// Like `check_anchor_ref` but also verifies the anchor is currently mounted.
/// Use for `scope:` fields in actions and conditions.
/// Non-scope anchor refs (e.g. `anchor:` in `WindowClosed`) skip the mount check.
fn check_scope_ref(
    scope: &str,
    span: &Span,
    anchors: &HashSet<String>,
    mounted: &HashSet<String>,
    path: &str,
    field: &str,
    diags: &mut Vec<LintDiag>,
) {
    if !anchors.contains(scope) {
        check_anchor_ref(scope, span, anchors, path, field, diags);
    } else if !mounted.contains(scope) {
        diags.push(diag_at(
            span,
            &format!("{path}.{field}"),
            format!("anchor '{scope}' is not mounted in this phase — add it to 'mount:'"),
        ));
    }
}

// ── Workflow-level ────────────────────────────────────────────────────────────

fn lint_workflow(v: &MarkedYaml<'_>, diags: &mut Vec<LintDiag>) {
    // Collect declared anchor names and their types.
    let (anchors, anchor_types): (HashSet<String>, HashMap<String, String>) = get(v, "anchors")
        .and_then(|a| a.data.as_mapping())
        .map(|m| {
            let names = m
                .keys()
                .filter_map(|k| k.data.as_str().map(|s| s.to_owned()))
                .collect();
            let types = m
                .iter()
                .filter_map(|(k, v)| {
                    let name = k.data.as_str()?.to_owned();
                    let ty = get_str(v, "type").map(|(s, _)| s.to_owned())?;
                    Some((name, ty))
                })
                .collect();
            (names, types)
        })
        .unwrap_or_default();

    // Collect declared param names.
    let params: HashSet<String> = if let Some(params_node) = get(v, "params") {
        if let Some(seq) = params_node.data.as_sequence() {
            seq.iter()
                .filter_map(|p| get_str(p, "name").map(|(s, _span)| s.to_owned()))
                .collect()
        } else {
            diags.push(diag_at(
                &params_node.span,
                "params",
                "params must be a sequence of {name, default?} objects, not a map",
            ));
            HashSet::new()
        }
    } else {
        HashSet::new()
    };

    // Collect phase names for go_to validation.
    let phase_names: HashSet<String> = get(v, "phases")
        .and_then(|p| p.data.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|p| get_str(p, "name").map(|(s, _span)| s.to_owned()))
                .collect()
        })
        .unwrap_or_default();

    // Check for duplicate phase names.
    if let Some(phases_seq) = get(v, "phases").and_then(|p| p.data.as_sequence()) {
        let mut seen: HashSet<&str> = HashSet::new();
        for (i, phase) in phases_seq.iter().enumerate() {
            if let Some((name, span)) = get_str(phase, "name") {
                if !seen.insert(name) {
                    diags.push(diag_at(
                        &span,
                        &format!("phases[{i}].name"),
                        format!("duplicate phase name '{name}'"),
                    ));
                }
            }
        }
    }

    // Lint anchors.
    if let Some(anchor_map) = get(v, "anchors").and_then(|a| a.data.as_mapping()) {
        for (key, anchor_v) in anchor_map {
            if let Some(name) = key.data.as_str() {
                lint_anchor(
                    anchor_v,
                    &format!("anchors.{name}"),
                    &anchors,
                    &anchor_types,
                    diags,
                );
            }
        }
    }

    // Collect declared recovery handler names.
    let handler_names: HashSet<String> = get(v, "recovery_handlers")
        .and_then(|r| r.data.as_mapping())
        .map(|m| {
            m.keys()
                .filter_map(|k| k.data.as_str().map(|s| s.to_owned()))
                .collect()
        })
        .unwrap_or_default();

    // Lint phases, tracking which anchors are currently mounted.
    if let Some(phases_seq) = get(v, "phases").and_then(|p| p.data.as_sequence()) {
        let mut mounted: HashSet<String> = HashSet::new();
        for (i, phase) in phases_seq.iter().enumerate() {
            // Apply mount: before linting this phase's steps.
            if let Some(seq) = get(phase, "mount").and_then(|m| m.data.as_sequence()) {
                for item in seq {
                    if let Some(n) = item.data.as_str() {
                        mounted.insert(n.to_owned());
                    }
                }
            }
            lint_phase(
                phase,
                &format!("phases[{i}]"),
                &anchors,
                &mounted,
                &params,
                &phase_names,
                &handler_names,
                diags,
            );
            // Apply unmount: after linting.
            if let Some(seq) = get(phase, "unmount").and_then(|u| u.data.as_sequence()) {
                for item in seq {
                    if let Some(n) = item.data.as_str() {
                        mounted.remove(n);
                    }
                }
            }
        }
    }

    // Lint recovery handlers.
    // Handlers fire from within phase execution, so can reference any declared anchor.
    // Pass anchors as mounted to suppress false "not mounted" diagnostics.
    if let Some(handlers_map) = get(v, "recovery_handlers").and_then(|r| r.data.as_mapping()) {
        for (key, handler_v) in handlers_map {
            if let Some(name) = key.data.as_str() {
                let path = format!("recovery_handlers.{name}");
                if let Some(trigger) = get(handler_v, "trigger") {
                    lint_condition(
                        trigger,
                        &format!("{path}.trigger"),
                        &anchors,
                        &anchors,
                        &params,
                        diags,
                    );
                }
                if let Some(actions_seq) =
                    get(handler_v, "actions").and_then(|a| a.data.as_sequence())
                {
                    for (j, action) in actions_seq.iter().enumerate() {
                        lint_action(
                            action,
                            &format!("{path}.actions[{j}]"),
                            &anchors,
                            &anchors,
                            &params,
                            diags,
                        );
                    }
                }
            }
        }
    }
}

// ── Anchor ────────────────────────────────────────────────────────────────────

fn lint_anchor(
    v: &MarkedYaml<'_>,
    path: &str,
    anchors: &HashSet<String>,
    anchor_types: &HashMap<String, String>,
    diags: &mut Vec<LintDiag>,
) {
    let anchor_type = get_str(v, "type").map(|(s, _)| s);
    if anchor_type.is_none() {
        diags.push(diag_at(&v.span, path, "missing required field 'type'"));
    }
    if !matches!(anchor_type, Some("Session") | Some("Browser") | Some("Tab")) {
        if get(v, "selector").is_none() {
            diags.push(diag_at(&v.span, path, "missing required field 'selector'"));
        } else {
            check_selector(v, "selector", path, diags);
        }
    }

    if let Some((parent, span)) = get_str(v, "parent") {
        check_anchor_ref(parent, &span, anchors, path, "parent", diags);

        // Tab anchor's parent must be a Browser anchor.
        if anchor_type == Some("Tab") {
            match anchor_types.get(parent) {
                Some(parent_type) if parent_type != "Browser" => {
                    diags.push(diag_at(
                        &span,
                        &format!("{path}.parent"),
                        format!(
                            "Tab anchor's parent '{parent}' must be of type Browser, got '{parent_type}'"
                        ),
                    ));
                }
                _ => {}
            }
        }
    } else if anchor_type == Some("Tab") {
        diags.push(diag_at(
            &v.span,
            path,
            "Tab anchor requires a 'parent' of type Browser",
        ));
    }
}

// ── Phase ─────────────────────────────────────────────────────────────────────

fn lint_phase(
    v: &MarkedYaml<'_>,
    path: &str,
    anchors: &HashSet<String>,
    mounted: &HashSet<String>,
    params: &HashSet<String>,
    phase_names: &HashSet<String>,
    handler_names: &HashSet<String>,
    diags: &mut Vec<LintDiag>,
) {
    if v.data.as_mapping().is_none() {
        diags.push(diag_at(&v.span, path, "phase must be a YAML mapping"));
        return;
    }

    if get_str(v, "name").is_none() {
        diags.push(diag_at(&v.span, path, "missing required field 'name'"));
    }

    let has_flow_control = get(v, "flow_control").is_some();
    let has_subflow = get(v, "subflow").is_some();

    if has_flow_control {
        lint_flow_control_phase(v, path, phase_names, diags);
    } else if has_subflow {
        lint_subflow_phase(v, path, params, diags);
    } else {
        lint_action_phase(v, path, anchors, mounted, params, handler_names, diags);
    }
}

fn lint_flow_control_phase(
    v: &MarkedYaml<'_>,
    path: &str,
    phase_names: &HashSet<String>,
    diags: &mut Vec<LintDiag>,
) {
    let fc_path = format!("{path}.flow_control");
    if let Some(fc) = get(v, "flow_control") {
        if let Some((go_to, span)) = get_str(fc, "go_to") {
            if !phase_names.contains(go_to) {
                diags.push(diag_at(
                    &span,
                    &format!("{fc_path}.go_to"),
                    format!("unknown phase '{go_to}'"),
                ));
            }
        } else {
            diags.push(diag_at(
                &fc.span,
                &fc_path,
                "missing required field 'go_to'",
            ));
        }
        if get(fc, "condition").is_none() {
            diags.push(diag_at(
                &fc.span,
                &fc_path,
                "missing required field 'condition'",
            ));
        }
    }
    if get(v, "steps").is_some() {
        diags.push(diag_at(
            &v.span,
            path,
            "flow_control phase must not have 'steps'",
        ));
    }
}

fn lint_subflow_phase(
    v: &MarkedYaml<'_>,
    path: &str,
    _params: &HashSet<String>,
    diags: &mut Vec<LintDiag>,
) {
    if get_str(v, "subflow").is_none() {
        diags.push(diag_at(&v.span, path, "missing required field 'subflow'"));
    }
    if get(v, "steps").is_some() {
        diags.push(diag_at(
            &v.span,
            path,
            "subflow phase must not have 'steps'",
        ));
    }
}

fn lint_action_phase(
    v: &MarkedYaml<'_>,
    path: &str,
    anchors: &HashSet<String>,
    mounted: &HashSet<String>,
    params: &HashSet<String>,
    handler_names: &HashSet<String>,
    diags: &mut Vec<LintDiag>,
) {
    for field in &["mount", "unmount"] {
        if let Some(seq) = get(v, field).and_then(|v| v.data.as_sequence()) {
            for (i, item) in seq.iter().enumerate() {
                if let Some(anchor_name) = item.data.as_str() {
                    check_anchor_ref(
                        anchor_name,
                        &item.span,
                        anchors,
                        path,
                        &format!("{field}[{i}]"),
                        diags,
                    );
                }
            }
        }
    }

    // Validate recovery.handlers references.
    if let Some(recovery) = get(v, "recovery") {
        if let Some(handlers_seq) = get(recovery, "handlers").and_then(|h| h.data.as_sequence()) {
            for (i, item) in handlers_seq.iter().enumerate() {
                if let Some((name, span)) = item.data.as_str().map(|s| (s, &item.span)) {
                    if !handler_names.contains(name) {
                        let mut known: Vec<&str> =
                            handler_names.iter().map(|s| s.as_str()).collect();
                        known.sort();
                        diags.push(diag_at(
                            span,
                            &format!("{path}.recovery.handlers[{i}]"),
                            format!(
                                "unknown recovery handler '{}' (declared: {})",
                                name,
                                if known.is_empty() {
                                    "none".to_owned()
                                } else {
                                    known.join(", ")
                                }
                            ),
                        ));
                    }
                }
            }
        }
    }

    match get(v, "steps").and_then(|s| s.data.as_sequence()) {
        None => diags.push(diag_at(&v.span, path, "missing required field 'steps'")),
        Some(steps) => {
            for (i, step) in steps.iter().enumerate() {
                lint_step(
                    step,
                    &format!("{path}.steps[{i}]"),
                    anchors,
                    mounted,
                    params,
                    diags,
                );
            }
        }
    }
}

// ── Step ──────────────────────────────────────────────────────────────────────

fn lint_step(
    v: &MarkedYaml<'_>,
    path: &str,
    anchors: &HashSet<String>,
    mounted: &HashSet<String>,
    params: &HashSet<String>,
    diags: &mut Vec<LintDiag>,
) {
    if get(v, "intent").is_none() {
        diags.push(diag_at(&v.span, path, "missing required field 'intent'"));
    }
    if let Some(precondition) = get(v, "precondition") {
        lint_condition(
            precondition,
            &format!("{path}.precondition"),
            anchors,
            mounted,
            params,
            diags,
        );
    }
    match get(v, "action") {
        None => diags.push(diag_at(&v.span, path, "missing required field 'action'")),
        Some(action) => lint_action(
            action,
            &format!("{path}.action"),
            anchors,
            mounted,
            params,
            diags,
        ),
    }
    match get(v, "expect") {
        None => diags.push(diag_at(&v.span, path, "missing required field 'expect'")),
        Some(expect) => lint_condition(
            expect,
            &format!("{path}.expect"),
            anchors,
            mounted,
            params,
            diags,
        ),
    }
    if let Some(fallback) = get(v, "fallback") {
        lint_action(
            fallback,
            &format!("{path}.fallback"),
            anchors,
            mounted,
            params,
            diags,
        );
    }
    if let Some(on_failure) = get(v, "on_failure") {
        lint_on_failure(on_failure, &format!("{path}.on_failure"), diags);
    }
    if let Some(on_success) = get(v, "on_success") {
        lint_on_success(on_success, &format!("{path}.on_success"), diags);
    }
}

// ── OnFailure / OnSuccess ─────────────────────────────────────────────────────

fn lint_on_failure(v: &MarkedYaml<'_>, path: &str, diags: &mut Vec<LintDiag>) {
    match &v.data {
        saphyr::YamlData::Value(saphyr::Scalar::String(s)) => {
            if !matches!(s.as_ref(), "abort" | "continue") {
                diags.push(diag_at(
                    &v.span,
                    path,
                    format!("unknown on_failure value '{s}' — expected 'abort' or 'continue'"),
                ));
            }
        }
        _ => diags.push(diag_at(
            &v.span,
            path,
            "on_failure must be 'abort' or 'continue'",
        )),
    }
}

fn lint_on_success(v: &MarkedYaml<'_>, path: &str, diags: &mut Vec<LintDiag>) {
    match &v.data {
        saphyr::YamlData::Value(saphyr::Scalar::String(s)) => {
            if !matches!(s.as_ref(), "continue" | "return_phase") {
                diags.push(diag_at(
                    &v.span,
                    path,
                    format!(
                        "unknown on_success value '{s}' — expected 'continue' or 'return_phase'"
                    ),
                ));
            }
        }
        _ => diags.push(diag_at(
            &v.span,
            path,
            "on_success must be 'continue' or 'return_phase'",
        )),
    }
}

// ── Action ────────────────────────────────────────────────────────────────────

const ACTIONS_SCOPE_SELECTOR: &[&str] = &[
    "Click",
    "DoubleClick",
    "Hover",
    "ScrollIntoView",
    "ClickAt",
    "TypeText",
    "PressKey",
    "Focus",
    "Invoke",
    "SetValue",
    "SetToggle",
    "Extract",
];
const ACTIONS_SCOPE_ONLY: &[&str] = &[
    "ActivateWindow",
    "MinimizeWindow",
    "CloseWindow",
    "DismissDialog",
];
const ALL_ACTION_TYPES: &[&str] = &[
    "Click",
    "DoubleClick",
    "Hover",
    "ScrollIntoView",
    "ClickAt",
    "TypeText",
    "PressKey",
    "Focus",
    "Invoke",
    "SetValue",
    "SetToggle",
    "ActivateWindow",
    "MinimizeWindow",
    "CloseWindow",
    "DismissDialog",
    "ClickForegroundButton",
    "ClickForeground",
    "NoOp",
    "Sleep",
    "WriteOutput",
    "Extract",
    "Exec",
    "Eval",
    "MoveFile",
    "BrowserNavigate",
    "BrowserEval",
];

fn lint_action(
    v: &MarkedYaml<'_>,
    path: &str,
    anchors: &HashSet<String>,
    mounted: &HashSet<String>,
    params: &HashSet<String>,
    diags: &mut Vec<LintDiag>,
) {
    let Some((type_str, type_span)) = get_str(v, "type") else {
        diags.push(diag_at(&v.span, path, "missing required field 'type'"));
        return;
    };

    if !ALL_ACTION_TYPES.contains(&type_str) {
        diags.push(diag_at(
            &type_span,
            &format!("{path}.type"),
            format!(
                "unknown action type '{}' — expected one of: {}",
                type_str,
                ALL_ACTION_TYPES.join(", ")
            ),
        ));
        check_selector(v, "selector", path, diags);
        return;
    }

    if ACTIONS_SCOPE_SELECTOR.contains(&type_str) {
        if let Some((scope, span)) = require_str(v, "scope", path, diags) {
            check_scope_ref(scope, &span, anchors, mounted, path, "scope", diags);
        }
        require_str(v, "selector", path, diags);
        check_selector(v, "selector", path, diags);
    }

    if ACTIONS_SCOPE_ONLY.contains(&type_str) {
        if let Some((scope, span)) = require_str(v, "scope", path, diags) {
            check_scope_ref(scope, &span, anchors, mounted, path, "scope", diags);
        }
    }

    match type_str {
        "TypeText" => {
            if let Some((s, span)) = require_str(v, "text", path, diags) {
                check_interpolation(s, &span, &format!("{path}.text"), params, diags);
            }
        }
        "PressKey" => {
            if let Some((s, span)) = require_str(v, "key", path, diags) {
                check_interpolation(s, &span, &format!("{path}.key"), params, diags);
            }
        }
        "SetValue" => {
            if let Some((s, span)) = require_str(v, "value", path, diags) {
                check_interpolation(s, &span, &format!("{path}.value"), params, diags);
            }
        }
        "SetToggle" => {
            if get(v, "state").is_none() {
                diags.push(diag_at(&v.span, path, "missing required field 'state'"));
            }
        }
        "ClickForegroundButton" | "ClickForeground" => {
            if let Some((s, span)) = require_str(v, "name", path, diags) {
                check_interpolation(s, &span, &format!("{path}.name"), params, diags);
            }
        }
        "Sleep" => {
            if get(v, "duration").is_none() {
                diags.push(diag_at(&v.span, path, "missing required field 'duration'"));
            }
        }
        "WriteOutput" => {
            require_str(v, "key", path, diags);
            if let Some((s, span)) = require_str(v, "path", path, diags) {
                check_interpolation(s, &span, &format!("{path}.path"), params, diags);
            }
        }
        "Extract" => {
            require_str(v, "key", path, diags);
            if let Some((attr, span)) = get_str(v, "attribute") {
                if !matches!(attr, "name" | "text" | "inner_text") {
                    diags.push(diag_at(
                        &span,
                        &format!("{path}.attribute"),
                        format!(
                            "unknown attribute '{attr}' — expected 'name', 'text', or 'inner_text'"
                        ),
                    ));
                }
            }
        }
        "Eval" => {
            require_str(v, "key", path, diags);
            if let Some((s, span)) = require_str(v, "expr", path, diags) {
                check_interpolation(s, &span, &format!("{path}.expr"), params, diags);
                if let Err(e) = check_expr_syntax(s) {
                    diags.push(diag_at(&span, &format!("{path}.expr"), e));
                }
            }
        }
        "MoveFile" => {
            if let Some((s, span)) = require_str(v, "source", path, diags) {
                check_interpolation(s, &span, &format!("{path}.source"), params, diags);
            }
            if let Some((s, span)) = require_str(v, "destination", path, diags) {
                check_interpolation(s, &span, &format!("{path}.destination"), params, diags);
            }
        }
        "Exec" => {
            if let Some((s, span)) = require_str(v, "command", path, diags) {
                check_interpolation(s, &span, &format!("{path}.command"), params, diags);
            }
            if let Some(args_seq) = get(v, "args").and_then(|a| a.data.as_sequence()) {
                for (i, arg) in args_seq.iter().enumerate() {
                    if let Some(s) = arg.data.as_str() {
                        check_interpolation(
                            s,
                            &arg.span,
                            &format!("{path}.args[{i}]"),
                            params,
                            diags,
                        );
                    }
                }
            }
        }
        "ClickAt" => {
            if get(v, "x_pct").is_none() {
                diags.push(diag_at(&v.span, path, "missing required field 'x_pct'"));
            }
            if get(v, "y_pct").is_none() {
                diags.push(diag_at(&v.span, path, "missing required field 'y_pct'"));
            }
            match get_str(v, "kind") {
                None => diags.push(diag_at(&v.span, path, "missing required field 'kind'")),
                Some((kind, span)) => {
                    if !matches!(kind, "left" | "double" | "triple" | "right" | "middle") {
                        diags.push(diag_at(
                            &span,
                            &format!("{path}.kind"),
                            format!("unknown click kind '{kind}' — expected 'left', 'double', 'triple', 'right', or 'middle'"),
                        ));
                    }
                }
            }
        }
        "BrowserNavigate" => {
            if let Some((scope, span)) = require_str(v, "scope", path, diags) {
                check_scope_ref(scope, &span, anchors, mounted, path, "scope", diags);
            }
            if let Some((s, span)) = require_str(v, "url", path, diags) {
                check_interpolation(s, &span, &format!("{path}.url"), params, diags);
            }
        }
        "BrowserEval" => {
            if let Some((scope, span)) = require_str(v, "scope", path, diags) {
                check_scope_ref(scope, &span, anchors, mounted, path, "scope", diags);
            }
            if let Some((s, span)) = require_str(v, "expr", path, diags) {
                check_interpolation(s, &span, &format!("{path}.expr"), params, diags);
            }
            // key is optional — omit to discard the result
        }
        _ => {}
    }
}

// ── Condition ─────────────────────────────────────────────────────────────────

const ALL_CONDITION_TYPES: &[&str] = &[
    "ElementFound",
    "ElementEnabled",
    "ElementVisible",
    "ElementHasText",
    "ElementHasChildren",
    "ElementChecked",
    "WindowWithAttribute",
    "ProcessRunning",
    "WindowClosed",
    "WindowWithState",
    "DialogPresent",
    "DialogAbsent",
    "ForegroundIsDialog",
    "FileExists",
    "Always",
    "AllOf",
    "AnyOf",
    "Not",
    "EvalCondition",
    "ExecSucceeded",
    "TabWithAttribute",
    "TabWithState",
];

fn lint_condition(
    v: &MarkedYaml<'_>,
    path: &str,
    anchors: &HashSet<String>,
    mounted: &HashSet<String>,
    params: &HashSet<String>,
    diags: &mut Vec<LintDiag>,
) {
    let Some((type_str, type_span)) = get_str(v, "type") else {
        diags.push(diag_at(&v.span, path, "missing required field 'type'"));
        return;
    };

    if !ALL_CONDITION_TYPES.contains(&type_str) {
        diags.push(diag_at(
            &type_span,
            &format!("{path}.type"),
            format!(
                "unknown condition type '{}' — expected one of: {}",
                type_str,
                ALL_CONDITION_TYPES.join(", ")
            ),
        ));
        return;
    }

    match type_str {
        "ElementFound" | "ElementEnabled" | "ElementVisible" | "ElementHasChildren" | "ElementChecked" => {
            if let Some((scope, span)) = require_str(v, "scope", path, diags) {
                check_scope_ref(scope, &span, anchors, mounted, path, "scope", diags);
            }
            require_str(v, "selector", path, diags);
            check_selector(v, "selector", path, diags);
        }
        "ElementHasText" => {
            if let Some((scope, span)) = require_str(v, "scope", path, diags) {
                check_scope_ref(scope, &span, anchors, mounted, path, "scope", diags);
            }
            require_str(v, "selector", path, diags);
            check_selector(v, "selector", path, diags);
            match get(v, "pattern") {
                None => diags.push(diag_at(&v.span, path, "missing required field 'pattern'")),
                Some(pattern) => lint_text_match(pattern, &format!("{path}.pattern"), diags),
            }
        }
        "WindowWithAttribute" => {
            if get(v, "title").is_none()
                && get(v, "automation_id").is_none()
                && get(v, "pid").is_none()
            {
                diags.push(diag_at(
                    &v.span,
                    path,
                    "WindowWithAttribute requires at least one of: title, automation_id, pid",
                ));
            }
            check_title_match_field(v, "title", path, diags);
        }
        "ProcessRunning" => {
            require_str(v, "process", path, diags);
        }
        "WindowClosed" => {
            if let Some((anchor, span)) = require_str(v, "anchor", path, diags) {
                check_anchor_ref(anchor, &span, anchors, path, "anchor", diags);
            }
        }
        "WindowWithState" => {
            if let Some((anchor, span)) = require_str(v, "anchor", path, diags) {
                check_anchor_ref(anchor, &span, anchors, path, "anchor", diags);
            }
            if let Some((state, span)) = require_str(v, "state", path, diags) {
                if !matches!(state, "active" | "visible") {
                    diags.push(diag_at(
                        &span,
                        &format!("{path}.state"),
                        format!("unknown state '{state}' — expected 'active' or 'visible'"),
                    ));
                }
            }
        }
        "DialogPresent" | "DialogAbsent" => {
            if let Some((scope, span)) = require_str(v, "scope", path, diags) {
                check_scope_ref(scope, &span, anchors, mounted, path, "scope", diags);
            }
        }
        "ForegroundIsDialog" => {
            check_title_match_field(v, "title", path, diags);
        }
        "FileExists" => {
            if let Some((s, span)) = require_str(v, "path", path, diags) {
                check_interpolation(s, &span, &format!("{path}.path"), params, diags);
            }
        }
        "AllOf" | "AnyOf" => match get(v, "conditions").and_then(|c| c.data.as_sequence()) {
            None => diags.push(diag_at(
                &v.span,
                path,
                "missing required field 'conditions'",
            )),
            Some(conds) => {
                for (i, cond) in conds.iter().enumerate() {
                    lint_condition(
                        cond,
                        &format!("{path}.conditions[{i}]"),
                        anchors,
                        mounted,
                        params,
                        diags,
                    );
                }
            }
        },
        "Not" => match get(v, "condition") {
            None => diags.push(diag_at(&v.span, path, "missing required field 'condition'")),
            Some(cond) => lint_condition(
                cond,
                &format!("{path}.condition"),
                anchors,
                mounted,
                params,
                diags,
            ),
        },
        "TabWithAttribute" => {
            if let Some((scope, span)) = require_str(v, "scope", path, diags) {
                check_scope_ref(scope, &span, anchors, mounted, path, "scope", diags);
            }
            let has_title = get(v, "title").is_some();
            let has_url = get(v, "url").is_some();
            if !has_title && !has_url {
                diags.push(diag_at(
                    &v.span,
                    path,
                    "TabWithAttribute requires at least one of: title, url",
                ));
            }
            check_text_match_field(v, "title", path, diags);
            check_text_match_field(v, "url", path, diags);
        }
        "TabWithState" => {
            if let Some((scope, span)) = require_str(v, "scope", path, diags) {
                check_scope_ref(scope, &span, anchors, mounted, path, "scope", diags);
            }
            require_str(v, "expr", path, diags);
        }
        "Always" => {}
        "EvalCondition" => {
            if let Some((s, span)) = require_str(v, "expr", path, diags) {
                check_interpolation(s, &span, &format!("{path}.expr"), params, diags);
                if let Err(e) = check_expr_syntax(s) {
                    diags.push(diag_at(&span, &format!("{path}.expr"), e));
                }
            }
        }
        _ => {}
    }
}

// ── TextMatch / TitleMatch ────────────────────────────────────────────────────

/// Lint an optional TextMatch field on `parent`. If the field is present, it
/// must be a map with at least one recognised TextMatch key.
fn check_text_match_field(
    parent: &MarkedYaml<'_>,
    field: &str,
    path: &str,
    diags: &mut Vec<LintDiag>,
) {
    if let Some(node) = get(parent, field) {
        lint_text_match(node, &format!("{path}.{field}"), diags);
    }
}

fn lint_text_match(v: &MarkedYaml<'_>, path: &str, diags: &mut Vec<LintDiag>) {
    let pattern_fields = ["exact", "contains", "starts_with", "regex"];
    let set_count = pattern_fields
        .iter()
        .filter(|&&f| get(v, f).is_some())
        .count();
    match set_count {
        0 => {
            let non_empty = get(v, "non_empty")
                .and_then(|n| n.data.as_bool())
                .unwrap_or(false);
            if !non_empty {
                diags.push(diag_at(&v.span, path,
                    "TextMatch must specify at least one of: exact, contains, starts_with, regex, non_empty"));
            }
        }
        2.. => {
            diags.push(diag_at(
                &v.span,
                path,
                "TextMatch must specify exactly one of: exact, contains, starts_with, regex",
            ));
        }
        _ => {}
    }
}

/// Lint an optional TitleMatch field on `parent`. TitleMatch is a subset of
/// TextMatch: only `exact`, `contains`, `starts_with` are valid (no `regex` or `non_empty`).
fn check_title_match_field(
    parent: &MarkedYaml<'_>,
    field: &str,
    path: &str,
    diags: &mut Vec<LintDiag>,
) {
    if let Some(node) = get(parent, field) {
        lint_title_match(node, &format!("{path}.{field}"), diags);
    }
}

fn lint_title_match(v: &MarkedYaml<'_>, path: &str, diags: &mut Vec<LintDiag>) {
    let pattern_fields = ["exact", "contains", "starts_with"];
    let set_count = pattern_fields
        .iter()
        .filter(|&&f| get(v, f).is_some())
        .count();
    match set_count {
        0 => diags.push(diag_at(
            &v.span,
            path,
            "TitleMatch must specify at least one of: exact, contains, starts_with",
        )),
        2.. => diags.push(diag_at(
            &v.span,
            path,
            "TitleMatch must specify exactly one of: exact, contains, starts_with",
        )),
        _ => {}
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn diag_messages(raw: &str) -> Vec<String> {
        lint(raw)
            .into_iter()
            .map(|d| format!("{}: {}", d.path, d.message))
            .collect()
    }

    fn assert_contains(msgs: &[String], needle: &str) {
        assert!(
            msgs.iter().any(|m| m.contains(needle)),
            "expected to find '{needle}' in:\n{:#?}",
            msgs
        );
    }

    fn assert_has_location(diags: &[LintDiag]) {
        for d in diags {
            assert!(d.line.is_some(), "expected line number in diag: {d}");
        }
    }

    #[test]
    fn clean_workflow_has_no_diags() {
        let raw = r#"
name: clean
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: click a button
        action:
          type: Click
          scope: app
          selector: ">> [role=button][name=OK]"
        expect:
          type: Always
"#;
        assert!(lint(raw).is_empty());
    }

    #[test]
    fn unknown_action_type_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: x
        action: { type: Clik, scope: app, selector: ">> *" }
        expect: { type: Always }
"#;
        let diags = lint(raw);
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "unknown action type 'Clik'");
        assert_has_location(&diags);
    }

    #[test]
    fn invalid_selector_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: x
        action: { type: Click, scope: app, selector: ">> [role=button" }
        expect: { type: Always }
"#;
        let diags = lint(raw);
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "unclosed '['");
        assert_has_location(&diags);
    }

    #[test]
    fn unknown_anchor_ref_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: main
    mount: [app, missing_anchor]
    steps:
      - intent: x
        action: { type: Click, scope: app, selector: ">> *" }
        expect: { type: Always }
"#;
        let diags = lint(raw);
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "unknown anchor 'missing_anchor'");
        assert_has_location(&diags);
    }

    #[test]
    fn text_match_mutual_exclusion_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: x
        action: { type: NoOp }
        expect:
          type: ElementHasText
          scope: app
          selector: ">> *"
          pattern:
            contains: hello
            exact: world
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "TextMatch must specify exactly one of");
    }

    #[test]
    fn undeclared_param_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name=App]" }
params:
  - name: file_path
phases:
  - name: main
    mount: [app]
    steps:
      - intent: set value
        action:
          type: SetValue
          scope: app
          selector: ">> [role=edit]"
          value: "{param.save_di}"
        expect: { type: Always }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "unknown param 'save_di'");
        assert_contains(&msgs, "file_path");
    }

    #[test]
    fn declared_param_no_diag() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name=App]" }
params:
  - name: file_path
phases:
  - name: main
    mount: [app]
    steps:
      - intent: set value
        action:
          type: SetValue
          scope: app
          selector: ">> [role=edit]"
          value: "{param.file_path}"
        expect: { type: Always }
"#;
        assert!(lint(raw).is_empty());
    }

    #[test]
    fn unclosed_brace_in_value_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name=App]" }
params:
  - name: file_path
phases:
  - name: main
    mount: [app]
    steps:
      - intent: set value
        action:
          type: SetValue
          scope: app
          selector: ">> [role=edit]"
          value: "{param.file_path"
        expect: { type: Always }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "unclosed '{'");
    }

    #[test]
    fn window_with_attribute_needs_at_least_one_field() {
        let raw = r#"
name: t
phases:
  - name: main
    steps:
      - intent: x
        action: { type: NoOp }
        expect: { type: WindowWithAttribute }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "WindowWithAttribute requires at least one of");
    }

    #[test]
    fn valid_eval_condition_expr_no_diag() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: check count
        action: { type: NoOp }
        expect:
          type: EvalCondition
          expr: "output_count('items') > 0"
"#;
        assert!(lint(raw).is_empty());
    }

    #[test]
    fn invalid_eval_condition_expr_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: broken expr
        action: { type: NoOp }
        expect:
          type: EvalCondition
          expr: "output_count('items' > 0"
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, ".expr");
    }

    #[test]
    fn valid_eval_action_expr_no_diag() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: compute something
        action:
          type: Eval
          key: result
          expr: "split_lines(output.raw, -1)"
        expect: { type: Always }
"#;
        assert!(lint(raw).is_empty());
    }

    #[test]
    fn invalid_eval_action_expr_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: broken eval
        action:
          type: Eval
          key: result
          expr: "split_lines(output.raw,"
        expect: { type: Always }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, ".expr");
    }

    #[test]
    fn tab_anchor_without_parent_reported() {
        let raw = r#"
name: t
anchors:
  my_tab: { type: Tab, selector: "[name~=Dashboard]" }
phases:
  - name: main
    steps:
      - intent: x
        action: { type: NoOp }
        expect: { type: Always }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "Tab anchor requires a 'parent' of type Browser");
    }

    #[test]
    fn tab_anchor_with_non_browser_parent_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name=App]" }
  my_tab: { type: Tab, selector: "[name~=Dashboard]", parent: app }
phases:
  - name: main
    steps:
      - intent: x
        action: { type: NoOp }
        expect: { type: Always }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "must be of type Browser");
    }

    #[test]
    fn valid_browser_and_tab_anchors_no_diag() {
        let raw = r#"
name: t
anchors:
  browser: { type: Browser, selector: "[name~=Edge]" }
  my_tab: { type: Tab, parent: browser }
phases:
  - name: main
    steps:
      - intent: x
        action: { type: NoOp }
        expect: { type: Always }
"#;
        assert!(lint(raw).is_empty());
    }

    #[test]
    fn browser_navigate_missing_url_reported() {
        let raw = r#"
name: t
anchors:
  browser: { type: Browser, selector: "[name~=Edge]" }
  tab: { type: Tab, parent: browser }
phases:
  - name: main
    steps:
      - intent: navigate
        action: { type: BrowserNavigate, scope: tab }
        expect: { type: Always }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "missing required field 'url'");
    }

    #[test]
    fn browser_eval_missing_expr_reported() {
        let raw = r#"
name: t
anchors:
  browser: { type: Browser, selector: "[name~=Edge]" }
  tab: { type: Tab, parent: browser }
phases:
  - name: main
    steps:
      - intent: eval js
        action: { type: BrowserEval, scope: tab }
        expect: { type: Always }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "missing required field 'expr'");
    }

    #[test]
    fn browser_eval_without_key_no_diag() {
        let raw = r#"
name: t
anchors:
  browser: { type: Browser, selector: "[name~=Edge]" }
  tab: { type: Tab, parent: browser }
phases:
  - name: main
    mount: [browser, tab]
    steps:
      - intent: click via js
        action:
          type: BrowserEval
          scope: tab
          expr: "document.querySelector('button').click()"
        expect: { type: Always }
"#;
        assert!(lint(raw).is_empty());
    }

    #[test]
    fn tab_with_attribute_missing_title_and_url_reported() {
        let raw = r#"
name: t
anchors:
  browser: { type: Browser, selector: "[name~=Edge]" }
  tab: { type: Tab, parent: browser }
phases:
  - name: main
    steps:
      - intent: wait for tab
        action: { type: NoOp }
        expect: { type: TabWithAttribute, scope: tab }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "TabWithAttribute requires at least one of");
    }

    #[test]
    fn tab_with_attribute_plain_string_title_reported() {
        let raw = r#"
name: t
anchors:
  browser: { type: Browser, selector: "[name~=Edge]" }
  tab: { type: Tab, parent: browser }
phases:
  - name: main
    steps:
      - intent: wait for tab
        action: { type: NoOp }
        expect: { type: TabWithAttribute, scope: tab, title: "Git for Windows" }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "TextMatch must specify at least one of");
    }

    #[test]
    fn tab_with_attribute_text_match_no_diag() {
        let raw = r#"
name: t
anchors:
  browser: { type: Browser, selector: "[name~=Edge]" }
  tab: { type: Tab, parent: browser }
phases:
  - name: main
    mount: [browser, tab]
    steps:
      - intent: wait for tab
        action: { type: NoOp }
        expect:
          type: TabWithAttribute
          scope: tab
          title:
            contains: "Git for Windows"
"#;
        assert!(lint(raw).is_empty());
    }

    #[test]
    fn window_with_attribute_plain_string_title_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    steps:
      - intent: wait for window
        action: { type: NoOp }
        expect: { type: WindowWithAttribute, title: "My App" }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "TitleMatch must specify at least one of");
    }

    #[test]
    fn scope_used_before_mount_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    steps:
      - intent: click something
        action: { type: Click, scope: app, selector: ">> [role=button]" }
        expect: { type: Always }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "not mounted in this phase");
    }

    #[test]
    fn scope_mounted_no_diag() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: click something
        action: { type: Click, scope: app, selector: ">> [role=button]" }
        expect: { type: Always }
"#;
        assert!(lint(raw).is_empty());
    }

    #[test]
    fn window_closed_unknown_anchor_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: wait for close
        action: { type: NoOp }
        expect: { type: WindowClosed, anchor: nonexistent }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "unknown anchor 'nonexistent'");
    }

    #[test]
    fn window_with_state_unknown_anchor_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: wait for active
        action: { type: NoOp }
        expect: { type: WindowWithState, anchor: ghost, state: active }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "unknown anchor 'ghost'");
    }

    #[test]
    fn window_with_state_invalid_state_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: wait for state
        action: { type: NoOp }
        expect: { type: WindowWithState, anchor: app, state: focused }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "unknown state 'focused'");
    }

    #[test]
    fn precondition_linted() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: conditional click
        precondition: { type: ElementFound, scope: app, selector: ">> [role=button]" }
        action: { type: Click, scope: app, selector: ">> [role=button]" }
        expect: { type: Always }
"#;
        assert!(lint(raw).is_empty());
    }

    #[test]
    fn precondition_unknown_condition_type_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: conditional click
        precondition: { type: Bogus, scope: app }
        action: { type: Click, scope: app, selector: ">> [role=button]" }
        expect: { type: Always }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "unknown condition type 'Bogus'");
    }

    #[test]
    fn on_success_invalid_value_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: click
        action: { type: Click, scope: app, selector: ">> [role=button]" }
        expect: { type: Always }
        on_success: break
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "unknown on_success value 'break'");
    }

    #[test]
    fn on_success_valid_return_phase_no_diag() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: click
        action: { type: Click, scope: app, selector: ">> [role=button]" }
        expect: { type: Always }
        on_success: return_phase
"#;
        assert!(lint(raw).is_empty());
    }

    #[test]
    fn foreground_is_dialog_title_validated() {
        let raw = r#"
name: t
phases:
  - name: main
    steps:
      - intent: wait for dialog
        action: { type: NoOp }
        expect:
          type: ForegroundIsDialog
          title: "plain string not a TitleMatch"
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "TitleMatch must specify at least one of");
    }

    #[test]
    fn click_at_invalid_kind_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: click at position
        action:
          type: ClickAt
          scope: app
          selector: ">> [role=button]"
          x_pct: 0.5
          y_pct: 0.5
          kind: center
        expect: { type: Always }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "unknown click kind 'center'");
    }

    #[test]
    fn click_at_valid_kind_no_diag() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: click at position
        action:
          type: ClickAt
          scope: app
          selector: ">> [role=button]"
          x_pct: 0.5
          y_pct: 0.5
          kind: right
        expect: { type: Always }
"#;
        assert!(lint(raw).is_empty());
    }

    #[test]
    fn extract_invalid_attribute_reported() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: extract something
        action:
          type: Extract
          scope: app
          selector: ">> [role=label]"
          key: result
          attribute: label
        expect: { type: Always }
"#;
        let msgs = diag_messages(raw);
        assert_contains(&msgs, "unknown attribute 'label'");
    }

    #[test]
    fn extract_valid_attribute_no_diag() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: main
    mount: [app]
    steps:
      - intent: extract something
        action:
          type: Extract
          scope: app
          selector: ">> [role=label]"
          key: result
          attribute: inner_text
        expect: { type: Always }
"#;
        assert!(lint(raw).is_empty());
    }

    #[test]
    fn scope_mounted_in_prior_phase_no_diag() {
        let raw = r#"
name: t
anchors:
  app: { type: Root, selector: "[name~=App]" }
phases:
  - name: open
    mount: [app]
    steps:
      - intent: activate app
        action: { type: ActivateWindow, scope: app }
        expect: { type: Always }
  - name: interact
    steps:
      - intent: click something
        action: { type: Click, scope: app, selector: ">> [role=button]" }
        expect: { type: Always }
"#;
        assert!(lint(raw).is_empty());
    }
}
