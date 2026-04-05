use std::collections::HashMap;
use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AnchorDef, Condition, LaunchWait, ResumeStrategy, RetryPolicy, SelectorPath, Step, Tier,
};

mod runner;

// ── Top-level file ─────────────────────────────────────────────────────────

/// A declared script parameter. Passed at runtime via CLI (`--param name=value`) or the
/// RunWorkflow command's `params` map. The value is substituted into every YAML string in
/// the workflow wherever `{param.<name>}` appears.
#[derive(Deserialize, Clone, JsonSchema)]
pub struct ParamDef {
    /// Parameter name. Use `{param.<name>}` anywhere in the workflow YAML to substitute
    /// this value — selectors, intent strings, text fields, etc.
    pub name: String,
    /// Human-readable description of this parameter.
    #[serde(default)]
    pub description: Option<String>,
    /// Default value. Omit to make the parameter required.
    pub default: Option<String>,
}

/// A declared output key. When a workflow is used as a subflow, only keys listed here
/// are returned to the parent; all other `Extract` keys remain workflow-local.
#[derive(Deserialize, Clone, JsonSchema)]
pub struct OutputDef {
    /// Output key name. Must match the `key` field of an `Extract` action.
    pub name: String,
    /// Human-readable description of what this output contains.
    #[serde(default)]
    pub description: Option<String>,
}

/// A workflow script: ordered phases that automate a sequence of UI interactions.
///
/// ```
/// let yaml = r#"
/// name: my_workflow
/// params:
///   - name: text
///     default: "Hello World"
/// defaults:
///   timeout: 5s
/// launch:
///   exe: notepad.exe
///   timeout: 15s
/// anchors:
///   notepad: { type: Root, selector: "[name~=Notepad]" }
///   editor:  { type: Stable, parent: notepad, selector: ">> [role=edit][name='Text Editor']" }
/// recovery_handlers:
///   my_handler:
///     trigger: { type: DialogPresent, scope: notepad }
///     actions: [{ type: ClickForegroundButton, name: OK }]
///     resume: retry_step
/// phases:
///   - name: do_something
///     mount: [notepad, editor]
///     unmount: [editor]
///     recovery:
///       handlers: [my_handler]
///     steps:
///       - intent: click OK
///         action: { type: Click, scope: notepad, selector: ">> [role=button][name=OK]" }
///         expect: { type: DialogAbsent, scope: notepad }
/// "#;
/// let _: ui_automata::yaml::WorkflowFile = serde_yaml::from_str(yaml).unwrap();
/// ```
#[derive(Deserialize, JsonSchema)]
pub struct WorkflowFile {
    /// Unique workflow identifier, used in logs and progress events.
    pub name: String,
    /// Human-readable description of what this workflow does.
    #[serde(default)]
    pub description: Option<String>,
    /// Declared parameters. Before deserialization, every `{param.<name>}` token in the
    /// raw YAML is replaced with the corresponding runtime value. This substitution applies
    /// to selectors, intent strings, text fields, and any other string value. Example: a
    /// param named `folder` lets you write `[role=list item][name={param.folder}]` in a
    /// selector. Pass values via CLI (`--param folder=Downloads`) or the RunWorkflow `params` map.
    #[serde(default)]
    pub params: Vec<ParamDef>,
    /// Default timeout and retry policy applied to every step that does not set its own.
    #[serde(default)]
    pub defaults: Defaults,
    /// Named anchor definitions. An anchor is a named, cached handle to a live UI element.
    /// Anchors are activated per-phase via `mount:` and referenced by name as `scope` in
    /// actions and conditions. Each anchor declares a `type` (root/stable/ephemeral),
    /// a `selector` to find the element, and optional `process`/`pin_launch` filters.
    #[serde(default)]
    pub anchors: HashMap<String, YamlAnchor>,
    /// Named recovery handler definitions. A handler fires when a step times out and the
    /// handler's `trigger` condition is true. Phases opt in via their `recovery: handlers:` list.
    #[serde(default)]
    pub recovery_handlers: HashMap<String, YamlRecoveryHandler>,
    /// Optional application to launch before running phases. The executor waits for a window
    /// belonging to the launched PID to appear before proceeding.
    pub launch: Option<LaunchConfig>,
    /// Ordered list of phases. Phases run sequentially; the first failure stops the workflow.
    #[serde(default)]
    pub phases: Vec<YamlPhase>,
    /// Keys that this workflow returns to its parent when used as a subflow.
    ///
    /// When present, only `Extract` actions whose `key` is listed here propagate their
    /// value to the parent workflow; all other extracted keys are workflow-local and are
    /// discarded when the subflow returns. When absent the old behaviour is preserved:
    /// every extracted value is returned (equivalent to listing all keys).
    ///
    /// ```yaml
    /// outputs:
    ///   - name: saved_file
    ///     description: The final output CSV file
    ///   - name: export_path
    /// ```
    #[serde(default)]
    pub outputs: Option<Vec<OutputDef>>,
    /// Path of the file this workflow was loaded from. Set by `load()`; not serialized.
    #[serde(skip)]
    #[schemars(skip)]
    pub source_path: Option<std::path::PathBuf>,
    /// Resolved param values (defaults merged with CLI overrides). Available at runtime
    /// for `Eval` expressions via `param.key`. Not serialized.
    #[serde(skip)]
    #[schemars(skip)]
    pub params_resolved: HashMap<String, String>,
}

/// Workflow-level recovery defaults.
#[derive(Deserialize, JsonSchema, Default)]
pub struct DefaultsRecovery {
    /// Maximum number of times any recovery handler may fire per phase. Default: 10.
    pub limit: Option<u32>,
}

/// Step defaults applied when a step does not specify its own timeout or retry policy.
#[derive(Deserialize, JsonSchema)]
pub struct Defaults {
    /// Maximum time to wait for a step's `expect` condition to become true.
    /// Accepts duration strings such as `"5s"`, `"300ms"`, `"2m"`. Default: 10s.
    #[serde(default, with = "crate::duration::serde::option")]
    #[schemars(schema_with = "crate::schema::duration_schema")]
    pub timeout: Option<Duration>,
    /// Retry policy applied when a step times out and no recovery handler fires.
    /// Default: `fixed: { count: 1, delay: 1s }`.
    #[serde(default = "default_retry")]
    pub retry: RetryPolicy,
    /// Whether to snapshot the DOM after each action for diffing.
    #[serde(default)]
    pub action_snapshot: bool,
    /// Recovery defaults applied to all phases.
    #[serde(default)]
    pub recovery: DefaultsRecovery,
}

fn default_retry() -> RetryPolicy {
    RetryPolicy::Fixed {
        count: 1,
        delay: Duration::from_secs(1),
    }
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            timeout: None,
            retry: default_retry(),
            action_snapshot: false,
            recovery: DefaultsRecovery::default(),
        }
    }
}

/// Launches an application before phases run. Exactly one of `exe` or `app` must be set.
#[derive(Deserialize, JsonSchema)]
pub struct LaunchConfig {
    /// Executable name or path to launch, e.g. `"notepad.exe"`. Resolved via PATH.
    pub exe: Option<String>,
    /// Store / URI / UWP app to launch. Accepts:
    /// - URI scheme name (without colon): `"ms-windows-store"`, `"ms-settings"`
    /// - Full URI: `"ms-windows-store:"`, `"ms-settings:display"`
    /// - UWP AppID: `"Microsoft.WindowsStore_8wekyb3d8bbwe!App"`
    /// - Start Menu AppID: `"{GUID}\\path\\to\\App.exe"`
    pub app: Option<String>,
    /// How long to wait for the launched application's window to appear.
    /// Accepts duration strings such as `"15s"`. Default: 15s.
    #[serde(default, with = "crate::duration::serde::option")]
    #[schemars(schema_with = "crate::schema::duration_schema")]
    pub timeout: Option<Duration>,
    /// Window identification strategy. Controls which window the launch anchor locks to.
    ///
    /// - `match_any` *(default)*: wait until the anchor's selector resolves against any
    ///   window of the process. Suitable for apps that reuse an existing process (browsers).
    /// - `new_pid`: wait for a window owned by the exact PID returned by the OS launcher.
    ///   Use for normal multi-instance apps (Notepad, Word, Excel).
    /// - `new_window`: snapshot existing windows before launch; wait for a new window to
    ///   appear in the process. Use for single-instance apps (Explorer, VS Code) where the
    ///   launched process hands off to an existing one and exits immediately.
    #[serde(default)]
    pub wait: LaunchWait,
    /// Name of a declared anchor to wait for instead of using `wait:`.
    /// Snapshots existing windows before launch, then polls until a new window
    /// matching the anchor's `process:` filter appears. Use when the launched
    /// exe hands off to a different process (e.g. `control.exe` → `explorer`).
    #[serde(default)]
    pub wait_for: Option<String>,
}

// ── Anchor definition ──────────────────────────────────────────────────────

/// Declaration of a named UI element handle used as a `scope` in actions and conditions.
#[derive(Deserialize, JsonSchema)]
pub struct YamlAnchor {
    /// Lifetime tier that controls how the handle is resolved and cached.
    #[serde(rename = "type")]
    pub kind: AnchorKind,
    /// CSS-like selector path to find this element.
    /// Root/Session/Stable/Ephemeral anchors: required — matched against desktop windows or
    /// the parent anchor's subtree.
    /// Browser anchors: omit (not used).
    /// Tab anchors (attach mode): match against TabInfo fields
    /// (`title`, `url`). Example: `"[title~='Git for Windows'][url~='github.com']"`.
    #[serde(default)]
    pub selector: Option<SelectorPath>,
    /// For `Stable`, `Ephemeral`, and `Tab` anchors: name of the parent anchor.
    /// For `Tab` anchors the parent must be a `Browser` anchor.
    pub parent: Option<String>,
    /// Restrict resolution to windows belonging to a process with this name
    /// (case-insensitive, without `.exe`). Combine with `selector: "*"` to match
    /// any window of the process regardless of title.
    #[serde(default)]
    pub process: Option<String>,
    /// Pin this anchor to a specific process ID. Takes precedence over `process`.
    /// Useful when the caller knows the exact PID (e.g. from `list_windows` or
    /// after `open_application` returns a PID via a workflow param).
    #[serde(default)]
    pub pid: Option<u32>,
}

/// Lifetime tier of an anchor, controlling how it is resolved and what happens when it goes stale.
#[derive(Deserialize, JsonSchema)]
pub enum AnchorKind {
    /// Resolved against desktop application windows. A stale root anchor is a fatal error.
    Root,
    /// A top-level window that can appear and disappear during the workflow.
    /// Resolved lazily on first use; going stale is not fatal. Use for transient
    /// windows such as progress dialogs that may or may not be present.
    Session,
    /// Resolved within a parent anchor's subtree. Re-queried automatically when the cached
    /// handle goes stale (e.g. after a page navigation or panel reload).
    Stable,
    /// Short-lived anchor declared in a phase's `mount` list. Released when the phase exits.
    Ephemeral,
    /// A CDP browser session (Edge). Calls `browser.ensure()` on mount; stores the
    /// Edge window as a Root UIA anchor so UIA actions can target it normally.
    Browser,
    /// A specific browser tab. `parent` must name a `Browser` anchor.
    /// Omit `selector` (or use `"*"`) — open a new blank tab (or reuse an existing New Tab page);
    /// navigate via a `BrowserNavigate` action. Closed on unmount.
    /// Set `selector` — attach to an existing tab matched by title/url. Left open on unmount.
    Tab,
}

impl YamlAnchor {
    fn into_def(self, name: String) -> AnchorDef {
        // For UIA anchors, selector is required; use wildcard as fallback.
        let selector = self.selector.unwrap_or_else(|| {
            SelectorPath::parse("*").expect("wildcard selector is always valid")
        });
        match self.kind {
            AnchorKind::Root => AnchorDef {
                name,
                parent: None,
                selector,
                tier: Tier::Root,
                pid: self.pid,
                process_name: self.process,
                mount_depth: 0,
            },
            AnchorKind::Session => AnchorDef {
                name,
                parent: None,
                selector,
                tier: Tier::Session,
                pid: self.pid,
                process_name: self.process,
                mount_depth: 0,
            },
            AnchorKind::Stable => AnchorDef {
                name,
                parent: self.parent,
                selector,
                tier: Tier::Stable,
                pid: self.pid,
                process_name: self.process,
                mount_depth: 0,
            },
            AnchorKind::Ephemeral => AnchorDef {
                name,
                parent: self.parent,
                selector,
                tier: Tier::Ephemeral,
                pid: self.pid,
                process_name: self.process,
                mount_depth: 0,
            },
            AnchorKind::Browser => AnchorDef {
                name,
                parent: None,
                selector: SelectorPath::parse("*").expect("wildcard selector is always valid"),
                tier: Tier::Browser,
                pid: None,
                // Default to "msedge"; user can override with `process: chrome` etc.
                process_name: self.process.clone().or_else(|| Some("msedge".into())),
                mount_depth: 0,
            },
            AnchorKind::Tab => AnchorDef {
                name,
                parent: self.parent,
                selector,
                tier: Tier::Tab,
                pid: None,
                process_name: None,
                mount_depth: 0,
            },
        }
    }
}

// ── Recovery handler definition ────────────────────────────────────────────

/// A handler that fires when a step times out in a known bad state and executes
/// corrective actions before the step is retried, skipped, or failed.
#[derive(Deserialize, JsonSchema)]
pub struct YamlRecoveryHandler {
    /// Condition checked after a step timeout. The handler fires when this is true.
    pub trigger: Condition,
    /// Actions to run when the trigger fires, e.g. dismiss an unexpected dialog.
    pub actions: Vec<crate::Action>,
    /// What the executor does after the recovery actions complete.
    pub resume: ResumeStrategy,
}

/// Phase-level recovery configuration.
#[derive(Deserialize, JsonSchema, Default)]
pub struct YamlPhaseRecovery {
    /// Names from the top-level `recovery_handlers` map to enable for this phase's steps.
    #[serde(default)]
    pub handlers: Vec<String>,
    /// Maximum number of times any recovery handler may fire across all steps in this phase.
    /// Overrides `defaults.recovery.limit`. Default: 10.
    pub limit: Option<u32>,
}

// ── Phase ──────────────────────────────────────────────────────────────────

/// Condition-and-jump node: evaluates a condition and, if true, jumps to a
/// named phase. If false, falls through to the next phase in order.
#[derive(Deserialize, JsonSchema)]
pub struct FlowControl {
    /// Condition to evaluate.
    pub condition: Condition,
    /// Name of the phase to jump to when the condition is true.
    pub go_to: String,
}

/// A decision node: no steps, no mounts — just a branch.
#[derive(Deserialize, JsonSchema)]
pub struct YamlFlowControlPhase {
    pub name: String,
    pub flow_control: FlowControl,
}

/// An action node: the standard phase with steps and lifecycle hooks.
#[derive(Deserialize, JsonSchema)]
pub struct YamlActionPhase {
    /// Phase name used in logs and progress events.
    pub name: String,
    /// When true, this phase always runs regardless of whether earlier phases failed.
    /// Use for cleanup: closing dialogs, restoring focus, resetting state.
    /// Finally phase errors are logged but do not override the original workflow error.
    #[serde(default)]
    pub finally: bool,
    /// Evaluated before mounting anchors. If false, the phase is silently skipped
    /// and the workflow continues — not an error.
    #[serde(default)]
    pub precondition: Option<Condition>,
    /// Anchor names to activate at the start of this phase. Root anchors are resolved
    /// immediately; stable anchors are resolved on first use.
    #[serde(default)]
    pub mount: Vec<String>,
    /// Anchor names to release at the end of this phase, even if steps fail.
    #[serde(default)]
    pub unmount: Vec<String>,
    /// Recovery configuration: which named handlers to enable for this phase's steps.
    #[serde(default)]
    pub recovery: Option<YamlPhaseRecovery>,
    /// Ordered list of steps to execute. The first step failure stops the phase.
    pub steps: Vec<Step>,
}

/// A subflow phase: delegates to a child workflow YAML file.
#[derive(Deserialize, JsonSchema)]
pub struct YamlSubflowPhase {
    /// Phase name used in logs and progress events.
    pub name: String,
    /// Path to the child workflow YAML file, relative to this workflow's file.
    pub subflow: String,
    /// Parameter values to pass to the child workflow.
    #[serde(default)]
    pub params: HashMap<String, String>,
}

/// A phase is a decision node (`flow_control`), a subflow delegate (`subflow`),
/// or an action node (`steps`).
#[derive(JsonSchema)]
#[serde(untagged)] // kept for JsonSchema derive only; deserialization uses TryFrom below
pub enum YamlPhase {
    /// Decision node — tried first because it requires `flow_control` key.
    FlowControl(YamlFlowControlPhase),
    /// Subflow delegate — requires `subflow` key.
    Subflow(YamlSubflowPhase),
    /// Action node — fallback, requires `steps` key.
    Action(YamlActionPhase),
}

impl<'de> serde::Deserialize<'de> for YamlPhase {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = serde_yaml::Value::deserialize(deserializer)?;
        YamlPhase::try_from(v).map_err(serde::de::Error::custom)
    }
}

impl TryFrom<serde_yaml::Value> for YamlPhase {
    type Error = String;

    fn try_from(v: serde_yaml::Value) -> Result<Self, String> {
        let map = v.as_mapping().ok_or("phase must be a YAML mapping")?;
        let has = |k: &str| map.contains_key(&serde_yaml::Value::String(k.into()));

        if has("flow_control") {
            serde_yaml::from_value::<YamlFlowControlPhase>(v)
                .map(YamlPhase::FlowControl)
                .map_err(|e| format!("flow_control phase: {e}"))
        } else if has("subflow") {
            serde_yaml::from_value::<YamlSubflowPhase>(v)
                .map(YamlPhase::Subflow)
                .map_err(|e| format!("subflow phase: {e}"))
        } else {
            serde_yaml::from_value::<YamlActionPhase>(v)
                .map(YamlPhase::Action)
                .map_err(|e| format!("action phase: {e}"))
        }
    }
}

impl YamlPhase {
    pub fn name(&self) -> &str {
        match self {
            YamlPhase::FlowControl(p) => &p.name,
            YamlPhase::Subflow(p) => &p.name,
            YamlPhase::Action(p) => &p.name,
        }
    }
}

/// Lightweight struct for reading just the `name:` field without full parsing.
#[derive(Deserialize)]
pub struct WorkflowName {
    pub name: String,
}

impl WorkflowName {
    /// Returns the workflow name from raw YAML, or `None` on any parse error.
    pub fn read(raw: &str) -> Option<String> {
        serde_yaml::from_str::<WorkflowName>(raw)
            .ok()
            .map(|w| w.name)
    }
}

/// Lightweight struct for reading workflow metadata without full parsing.
/// Used by `ListWorkflows` to scan installed workflows cheaply.
#[derive(Deserialize)]
pub struct WorkflowHeader {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub params: Vec<ParamDef>,
}

/// Phase-level progress event emitted by `WorkflowFile::run()` via a sync channel.
#[derive(Debug, Clone)]
pub enum PhaseEvent {
    PhaseStarted(String),
    PhaseCompleted(String),
    /// Phase was skipped because its `precondition` evaluated to false.
    PhaseSkipped(String),
    PhaseFailed {
        phase: String,
        error: String,
    },
    Completed,
    Failed(String),
}

// ── Loading ────────────────────────────────────────────────────────────────

impl WorkflowFile {
    /// Load and parse a workflow YAML file, substituting CLI params into all
    /// string values. Param keys use `{param.key}` syntax in the YAML.
    pub fn load(path: &str, params: &HashMap<String, String>) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
        let source_path = std::path::PathBuf::from(path);
        let workflow_dir = source_path
            .parent()
            .map(|p| p.to_string_lossy().into_owned());
        let mut wf = Self::load_from_str_with_dir(&raw, params, workflow_dir.as_deref())?;
        wf.source_path = Some(source_path);
        Ok(wf)
    }

    /// Parse a workflow from a YAML string with CLI param overrides.
    pub fn load_from_str(raw: &str, params: &HashMap<String, String>) -> Result<Self, String> {
        Self::load_from_str_with_dir(raw, params, None)
    }

    fn load_from_str_with_dir(
        raw: &str,
        params: &HashMap<String, String>,
        workflow_dir: Option<&str>,
    ) -> Result<Self, String> {
        // Run linter first so missing fields surface with helpful messages.
        let diags = crate::lint::lint(raw);
        if !diags.is_empty() {
            let lines: Vec<String> = diags
                .iter()
                .map(|d| {
                    if let (Some(line), Some(col)) = (d.line, d.col) {
                        format!("  {}:{} [{}] {}", line, col, d.path, d.message)
                    } else {
                        format!("  [{}] {}", d.path, d.message)
                    }
                })
                .collect();
            return Err(format!("workflow lint errors:\n{}", lines.join("\n")));
        }

        // Two-pass: parse YAML Value → substitute strings → deserialize.
        let mut value: serde_yaml::Value =
            serde_yaml::from_str(raw).map_err(|e| format!("YAML parse error: {e}"))?;

        // Parse declared params, then validate and merge CLI overrides.
        let param_defs: Vec<ParamDef> = value
            .get("params")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| serde_yaml::from_value(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        // Reject unknown CLI params.
        for key in params.keys() {
            if !param_defs.iter().any(|p| p.name == *key) {
                return Err(format!("unknown parameter '--{}'", key.replace('_', "-")));
            }
        }

        // Build merged map; reject missing required params.
        let mut merged = HashMap::new();
        for def in &param_defs {
            if let Some(val) = params.get(&def.name) {
                merged.insert(def.name.clone(), val.clone());
            } else if let Some(default) = &def.default {
                merged.insert(def.name.clone(), default.clone());
            } else {
                return Err(format!(
                    "required parameter '--{}' not provided",
                    def.name.replace('_', "-")
                ));
            }
        }

        substitute_params(&mut value, &merged, workflow_dir);

        let mut wf: WorkflowFile = {
            // serde_yaml doesn't expose a Deserializer from Value directly, so we
            // round-trip through JSON to get serde_path_to_error working.
            let json_str = serde_json::to_value(&value)
                .map_err(|e| format!("internal serialization error: {e}"))?
                .to_string();
            let mut de = serde_json::Deserializer::from_str(&json_str);
            serde_path_to_error::deserialize(&mut de)
                .map_err(|e| format!("workflow error at {}: {}", e.path(), e.inner()))?
        };

        wf.params_resolved = merged;

        // Validate anchor names: colon-prefixed names are reserved for depth scoping.
        for name in wf.anchors.keys() {
            if name.starts_with(':') {
                return Err(format!("anchor name '{name}' must not start with ':'"));
            }
        }

        // Apply outputs declaration: mark Extract keys not in outputs as local.
        // Only done when `outputs` is explicitly present; omitting it preserves old behaviour.
        if let Some(outputs) = &wf.outputs {
            let outputs_set: std::collections::HashSet<String> =
                outputs.iter().map(|o| o.name.clone()).collect();
            for phase in &mut wf.phases {
                if let YamlPhase::Action(ap) = phase {
                    for step in &mut ap.steps {
                        step.action.apply_outputs(&outputs_set);
                    }
                }
            }
            for handler in wf.recovery_handlers.values_mut() {
                for action in &mut handler.actions {
                    action.apply_outputs(&outputs_set);
                }
            }
        }

        Ok(wf)
    }

    /// Resolve a path relative to the directory containing this workflow file.
    /// Falls back to treating `relative` as a path from the working directory
    /// if no source_path is available.
    pub fn resolve_path(&self, relative: &str) -> std::path::PathBuf {
        if let Some(src) = &self.source_path {
            if let Some(parent) = src.parent() {
                return parent.join(relative);
            }
        }
        std::path::PathBuf::from(relative)
    }
}

// ── Parameter substitution ────────────────────────────────────────────────

/// Recursively replace `{param.key}`, `{env.VAR}`, and `{workflow.dir}` in all YAML string values.
fn substitute_params(
    value: &mut serde_yaml::Value,
    params: &HashMap<String, String>,
    workflow_dir: Option<&str>,
) {
    match value {
        serde_yaml::Value::String(s) => {
            for (k, v) in params {
                *s = s.replace(&format!("{{param.{k}}}"), v);
            }
            // Expand {workflow.dir} — directory containing the workflow file.
            if let Some(dir) = workflow_dir {
                *s = s.replace("{workflow.dir}", dir);
            }
            // Expand {env.VARNAME} tokens.
            while let Some(start) = s.find("{env.") {
                let rest = &s[start + 5..];
                if let Some(end) = rest.find('}') {
                    let var_name = &rest[..end];
                    let replacement = std::env::var(var_name).unwrap_or_default();
                    s.replace_range(start..start + 5 + end + 1, &replacement);
                } else {
                    break;
                }
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                substitute_params(item, params, workflow_dir);
            }
        }
        serde_yaml::Value::Mapping(map) => {
            for (_, v) in map.iter_mut() {
                substitute_params(v, params, workflow_dir);
            }
        }
        _ => {}
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Condition, RetryPolicy};
    use std::time::Duration;

    fn parse(yaml: &str) -> WorkflowFile {
        WorkflowFile::load_from_str(yaml, &HashMap::new()).expect("parse failed")
    }

    fn action_phase(phase: &YamlPhase) -> &YamlActionPhase {
        match phase {
            YamlPhase::Action(p) => p,
            _ => panic!("expected Action phase"),
        }
    }

    #[test]
    fn minimal_workflow_parses() {
        let wf = parse(
            r#"
name: smoke
anchors:
  root: { type: Root, selector: "*" }
phases:
  - name: do_nothing
    mount: [root]
    steps:
      - intent: noop
        action: { type: NoOp }
        expect: { type: DialogAbsent, scope: root }
"#,
        );
        assert_eq!(wf.name, "smoke");
        assert_eq!(wf.phases.len(), 1);
        assert_eq!(action_phase(&wf.phases[0]).steps.len(), 1);
    }

    #[test]
    fn params_substituted() {
        let mut cli: HashMap<String, String> = HashMap::new();
        cli.insert("text".into(), "hello world".into());

        let raw = r#"
name: test
params:
  - name: text
    default: default
anchors:
  ed: { type: Root, selector: "*" }
phases:
  - name: type_it
    mount: [ed]
    steps:
      - intent: type
        action: { type: TypeText, scope: ed, selector: "[role=edit]", text: "{param.text}" }
        expect: { type: ElementHasText, scope: ed, selector: "[role=edit]", pattern: { contains: "{param.text}" } }
"#;
        let wf = WorkflowFile::load_from_str(raw, &cli).unwrap();

        let step = &action_phase(&wf.phases[0]).steps[0];
        match &step.action {
            crate::Action::TypeText { text, .. } => assert_eq!(text, "hello world"),
            _ => panic!("expected TypeText"),
        }
    }

    #[test]
    fn env_substituted() {
        // SAFETY: single-threaded test; no other threads read this env var.
        unsafe { std::env::set_var("TEST_UI_AUTOMATA_HOME", "C:\\Users\\testuser") };
        let raw = r#"
name: test
params:
  - name: save_dir
    default: "{env.TEST_UI_AUTOMATA_HOME}\\Documents\\"
phases: []
"#;
        let wf = WorkflowFile::load_from_str(raw, &HashMap::new()).unwrap();
        let param = wf.params.iter().find(|p| p.name == "save_dir").unwrap();
        assert_eq!(
            param.default.as_deref(),
            Some("C:\\Users\\testuser\\Documents\\")
        );
    }

    #[test]
    fn missing_required_param_errors() {
        let raw = r#"
name: test
params:
  - name: required_thing
phases: []
"#;
        let result = WorkflowFile::load_from_str(raw, &HashMap::new());
        assert!(result.is_err());
        assert!(result.err().unwrap().contains("required-thing"));
    }

    #[test]
    fn unknown_param_errors() {
        let raw = r#"
name: test
params:
  - name: text
    default: hi
phases: []
"#;
        let mut cli = HashMap::new();
        cli.insert("unknown_key".into(), "val".into());
        let result = WorkflowFile::load_from_str(raw, &cli);
        assert!(result.is_err());
        assert!(result.err().unwrap().contains("unknown-key"));
    }

    #[test]
    fn window_with_title_flat_fields() {
        let wf = parse(
            r#"
name: test
phases:
  - name: check
    steps:
      - intent: wait
        action: { type: NoOp }
        expect:
          type: WindowWithAttribute
          title:
            contains: Notepad
"#,
        );
        let expect = &action_phase(&wf.phases[0]).steps[0].expect;
        match expect {
            Condition::WindowWithAttribute { title, .. } => {
                let t = title.as_ref().expect("expected title");
                assert_eq!(t.contains.as_deref(), Some("Notepad"));
                assert!(t.exact.is_none());
            }
            _ => panic!("expected WindowWithAttribute"),
        }
    }

    #[test]
    fn not_condition_wraps_correctly() {
        let wf = parse(
            r#"
name: test
phases:
  - name: check
    steps:
      - intent: wait for window gone
        action: { type: NoOp }
        expect:
          type: Not
          condition:
            type: WindowWithAttribute
            title:
              contains: Notepad
"#,
        );
        let expect = &action_phase(&wf.phases[0]).steps[0].expect;
        match expect {
            Condition::Not { condition } => match condition.as_ref() {
                Condition::WindowWithAttribute { title, .. } => {
                    let t = title.as_ref().expect("expected title");
                    assert_eq!(t.contains.as_deref(), Some("Notepad"));
                }
                _ => panic!("expected WindowWithAttribute inside Not"),
            },
            _ => panic!("expected Not"),
        }
    }

    #[test]
    fn retry_policy_fixed_parses() {
        let wf = parse(
            r#"
name: test
defaults:
  timeout: 5s
  retry:
    fixed: { count: 2, delay: 500ms }
anchors:
  root: { type: Root, selector: "*" }
phases:
  - name: p
    mount: [root]
    steps:
      - intent: x
        action: { type: NoOp }
        expect: { type: DialogAbsent, scope: root }
"#,
        );
        assert_eq!(wf.defaults.timeout, Some(Duration::from_secs(5)));
        match &wf.defaults.retry {
            RetryPolicy::Fixed { count, delay } => {
                assert_eq!(*count, 2);
                assert_eq!(*delay, Duration::from_millis(500));
            }
            _ => panic!("expected Fixed retry"),
        }
    }

    #[test]
    fn anchor_library_parses() {
        let wf = parse(
            r#"
name: test
anchors:
  notepad:
    type: Root
    selector: "[name~=Notepad]"
  editor:
    type: Stable
    parent: notepad
    selector: "[role=edit]"
phases: []
"#,
        );
        assert!(wf.anchors.contains_key("notepad"));
        assert!(wf.anchors.contains_key("editor"));
        assert_eq!(wf.anchors["editor"].parent.as_deref(), Some("notepad"));
    }

    #[test]
    fn recovery_handler_library_parses() {
        let wf = parse(
            r#"
name: test
anchors:
  main: { type: Root, selector: "*" }
recovery_handlers:
  dismiss_error:
    trigger: { type: DialogPresent, scope: main }
    actions:
      - { type: ClickForegroundButton, name: OK }
    resume: retry_step
phases: []
"#,
        );
        let handler = &wf.recovery_handlers["dismiss_error"];
        assert!(matches!(handler.resume, ResumeStrategy::RetryStep));
    }
}
