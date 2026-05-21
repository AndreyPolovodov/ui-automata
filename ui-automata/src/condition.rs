use schemars::JsonSchema;
use serde::Deserialize;

use std::collections::HashMap;

use crate::{
    AutomataError, Browser, Desktop, Element, SelectorPath, ShadowDom, ToggleValue,
    action::sub_output,
    output::Output,
};

// ── Text / title match helpers ────────────────────────────────────────────────

/// Matches element text. Exactly one field should be set.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TextMatch {
    pub exact: Option<String>,
    pub contains: Option<String>,
    pub starts_with: Option<String>,
    /// Fancy-regex pattern (supports backreferences, lookahead, etc.).
    pub regex: Option<String>,
    #[serde(default)]
    pub non_empty: bool,
}

impl TextMatch {
    pub fn exact(s: impl Into<String>) -> Self {
        Self {
            exact: Some(s.into()),
            contains: None,
            starts_with: None,
            regex: None,
            non_empty: false,
        }
    }
    pub fn contains(s: impl Into<String>) -> Self {
        Self {
            exact: None,
            contains: Some(s.into()),
            starts_with: None,
            regex: None,
            non_empty: false,
        }
    }
    pub fn non_empty() -> Self {
        Self {
            exact: None,
            contains: None,
            starts_with: None,
            regex: None,
            non_empty: true,
        }
    }

    pub fn test(&self, s: &str) -> bool {
        if let Some(v) = &self.exact {
            return s == v;
        }
        if let Some(v) = &self.contains {
            return s.contains(v.as_str());
        }
        if let Some(v) = &self.starts_with {
            return s.starts_with(v.as_str());
        }
        if let Some(v) = &self.regex {
            return fancy_regex::Regex::new(v)
                .ok()
                .and_then(|re| re.is_match(s).ok())
                .unwrap_or(false);
        }
        if self.non_empty {
            return !s.is_empty();
        }
        false
    }
}

/// Matches a window title. Exactly one field should be set.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TitleMatch {
    pub exact: Option<String>,
    pub contains: Option<String>,
    pub starts_with: Option<String>,
}

impl TitleMatch {
    pub fn exact(s: impl Into<String>) -> Self {
        Self {
            exact: Some(s.into()),
            contains: None,
            starts_with: None,
        }
    }
    pub fn contains(s: impl Into<String>) -> Self {
        Self {
            exact: None,
            contains: Some(s.into()),
            starts_with: None,
        }
    }
    pub fn starts_with(s: impl Into<String>) -> Self {
        Self {
            exact: None,
            contains: None,
            starts_with: Some(s.into()),
        }
    }

    pub fn test(&self, s: &str) -> bool {
        if let Some(v) = &self.exact {
            return s == v;
        }
        if let Some(v) = &self.contains {
            return s.contains(v.as_str());
        }
        if let Some(v) = &self.starts_with {
            return s.starts_with(v.as_str());
        }
        false
    }
}

/// Key written to locals by every `Exec` action — holds the integer exit code as a string.
/// Read by the [`Condition::ExecSucceeded`] condition.
pub const EXEC_EXIT_CODE_KEY: &str = "__exec_exit_code__";

// ── WindowState ───────────────────────────────────────────────────────────────

/// Observable state of a window anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WindowState {
    /// A window belonging to the same process as the anchor is the OS foreground window.
    Active,
    /// The window is visible on screen — not minimized or hidden.
    Visible,
}

// ── Condition ─────────────────────────────────────────────────────────────────

/// Custom `Deserialize` via `TryFrom<serde_yaml::Value>` to work around the
/// serde limitation that `#[serde(tag)]` + `#[serde(flatten)]` don't compose
/// in serde_yaml. We hand-roll the mapping from a YAML map to enum variants.
fn default_fuzz_pct() -> f64 { 2.0 }
fn default_max_diff_pct() -> f64 { 4.0 }
fn default_downscale() -> u32 { 8 }

/// Box-filter downscale by integer factor `k`. Output dimensions are floor(w/k) × floor(h/k);
/// each output pixel is the mean of the corresponding k×k block of inputs.
///
/// Why: high-frequency rendering noise (text antialiasing, subpixel shifts) gets averaged
/// out into nearby pixels and becomes invisible after downscaling. Meanwhile, large-scale
/// pattern differences — like a 1px solid vs 1px-dotted line — collapse into mean intensity:
/// the dotted line's row averages to a half-density (lighter) tone vs the solid line's row.
/// That now shows up as a per-pixel intensity diff that the standard luma comparison sees.
fn downscale_luma(luma: &[i32], w: u32, h: u32, k: u32) -> (Vec<i32>, u32, u32) {
    let ow = w / k;
    let oh = h / k;
    let mut out = Vec::with_capacity((ow * oh) as usize);
    let k2 = (k * k) as i64;
    for oy in 0..oh {
        for ox in 0..ow {
            let mut sum: i64 = 0;
            for dy in 0..k {
                for dx in 0..k {
                    let x = ox * k + dx;
                    let y = oy * k + dy;
                    sum += luma[(y * w + x) as usize] as i64;
                }
            }
            out.push((sum / k2) as i32);
        }
    }
    (out, ow, oh)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "serde_yaml::Value")]
pub enum Condition {
    ElementFound {
        scope: String,
        selector: SelectorPath,
    },
    ElementEnabled {
        scope: String,
        selector: SelectorPath,
    },
    ElementVisible {
        scope: String,
        selector: SelectorPath,
    },
    ElementHasText {
        scope: String,
        selector: SelectorPath,
        pattern: TextMatch,
    },
    ElementHasChildren {
        scope: String,
        selector: SelectorPath,
    },

    /// True when the element at `selector` under `scope` has a toggle state matching `state`.
    /// `state: true` = on, `state: false` = off, `state: "indeterminate"` = indeterminate.
    /// If `state` is omitted, passes for any toggle state (verifies TogglePattern is supported).
    ElementToggled {
        scope: String,
        selector: SelectorPath,
        /// `Some(On/Off/Indeterminate)`, `None` = any.
        #[serde(default)]
        state: Option<ToggleValue>,
    },

    /// True when the element at `selector` under `scope` has a selection state matching `state`.
    /// Uses `SelectionItemPattern` — works for RadioButton, ListItem, TabItem, etc.
    /// `state: true` = selected, `state: false` = not selected.
    /// If `state` is omitted, passes for any selection state (verifies SelectionItemPattern is supported).
    ElementItemSelected {
        scope: String,
        selector: SelectorPath,
        /// `Some(true)` = selected, `Some(false)` = not selected, `None` = any.
        #[serde(default)]
        state: Option<bool>,
    },

    /// True when the container element's current selection (via `ISelectionProvider.GetSelection()`)
    /// matches `pattern`. Works on ComboBox, ListBox, TabControl etc. without expanding.
    ElementSelected {
        scope: String,
        selector: SelectorPath,
        pattern: TextMatch,
    },

    /// Any application window matches the given attribute filters.
    /// YAML: `type: WindowWithAttribute` + at least one of:
    ///   - `title`: `TitleMatch` against the window's name
    ///   - `automation_id: <string>` (exact match on UIA AutomationId)
    ///   - `pid: <u32>` (exact process ID match)
    /// Optional `process: <name>` restricts to a specific process (case-insensitive, no .exe).
    WindowWithAttribute {
        title: Option<TitleMatch>,
        automation_id: Option<String>,
        pid: Option<u32>,
        process: Option<String>,
    },

    /// True when any application window belongs to a process whose name
    /// (without `.exe`) matches `process` (case-insensitive).
    /// YAML: `type: ProcessRunning` + `process: <name>`.
    ProcessRunning {
        process: String,
    },
    /// True when the window anchored to `anchor` is no longer open.
    /// HWND-locked anchors check that specific window handle; PID-only anchors
    /// check for any window of that process; unresolved anchors treat re-resolution
    /// failure as closed.
    WindowClosed {
        anchor: String,
    },
    /// True when the anchor's window is in the given state.
    WindowWithState {
        anchor: String,
        state: WindowState,
    },
    DialogPresent {
        scope: String,
    },
    DialogAbsent {
        scope: String,
    },

    ForegroundIsDialog {
        title: Option<TitleMatch>,
    },

    /// True when the file at `path` exists on disk.
    /// `path` supports `{output.*}` substitution via `apply_output`.
    FileExists {
        path: String,
    },

    /// Always evaluates to true immediately. Use as `expect` on steps where
    /// success is guaranteed by the action itself (e.g. `Eval`, `WriteOutput`, `NoOp`).
    Always,

    /// True when the most recent `Exec` action exited with code 0.
    /// Reads the exit code stored in locals under `__exec_exit_code__` by the `Exec` action.
    ExecSucceeded,

    /// Evaluates a boolean expression against the current output, locals, and params.
    /// The expression **must** return a `Bool` (use a comparison operator).
    /// Example: `"count % 10 == 0"`, `"score >= param.threshold"`
    EvalCondition {
        expr: String,
    },

    /// True when the browser tab anchored to `scope` matches the given attribute filters.
    /// YAML: `type: TabWithAttribute` + at least one of:
    ///   - `title`: `TextMatch` against the tab's current title.
    ///   - `url`: `TextMatch` against the tab's current URL.
    /// `scope` must name a mounted `Tab` anchor.
    TabWithAttribute {
        scope: String,
        title: Option<TextMatch>,
        url: Option<TextMatch>,
    },

    /// True when the JS expression `expr` evaluates to the string `"true"` in the browser tab `scope`.
    /// The expression must return a boolean — only the string `"true"` is treated as passing.
    /// Example: `expr: "document.readyState === 'complete'"`
    TabWithState {
        scope: String,
        expr: String,
    },

    /// True when two PNG files are pixel-equivalent within the given tolerance.
    ///
    /// The comparison is per-channel: a pixel matches if every RGB channel differs
    /// by at most `fuzz_pct / 100 * 255`. Alpha is ignored.
    /// Returns false if either file is missing or dimensions differ.
    /// Both paths support `{output.*}` substitution via `apply_output`.
    SnapshotMatches {
        actual: String,
        golden: String,
        /// Allowed per-pixel luminance difference as a percentage of 255. Default: 2.
        #[serde(default = "default_fuzz_pct")]
        fuzz_pct: f64,
        /// Max allowed fraction of pixels that may exceed fuzz_pct, as a percentage (0–100). Default: 5.
        #[serde(default = "default_max_diff_pct")]
        max_diff_pct: f64,
        /// Box-filter downscale factor applied to both images before comparison. Default: 1 (no downscale).
        /// Use larger values (e.g. 4, 8) to suppress high-frequency noise (text antialiasing) and
        /// expose low-frequency pattern differences — useful when 1px-wide visual differences
        /// like solid-vs-dotted lines are masked by per-run rendering jitter.
        #[serde(default = "default_downscale")]
        downscale: u32,
    },

    AllOf {
        conditions: Vec<Condition>,
    },
    AnyOf {
        conditions: Vec<Condition>,
    },
    Not {
        condition: Box<Condition>,
    },
}

// ── Custom TryFrom for serde_yaml::Value ──────────────────────────────────────

impl TryFrom<serde_yaml::Value> for Condition {
    type Error = String;

    fn try_from(v: serde_yaml::Value) -> Result<Self, String> {
        let map = v.as_mapping().ok_or("Condition must be a YAML mapping")?;

        let type_str = map
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or("Condition missing string field 'type'")?;

        let str_field = |key: &str| -> Option<String> {
            map.get(key).and_then(|v| v.as_str()).map(String::from)
        };
        let req_str = |key: &str| -> Result<String, String> {
            str_field(key).ok_or_else(|| format!("Condition '{type_str}' missing '{key}'"))
        };
        let req_selector = |key: &str| -> Result<SelectorPath, String> {
            let s = req_str(key)?;
            SelectorPath::parse(&s).map_err(|e| e.to_string())
        };

        match type_str {
            "ElementFound" => Ok(Condition::ElementFound {
                scope: req_str("scope")?,
                selector: req_selector("selector")?,
            }),
            "ElementEnabled" => Ok(Condition::ElementEnabled {
                scope: req_str("scope")?,
                selector: req_selector("selector")?,
            }),
            "ElementVisible" => Ok(Condition::ElementVisible {
                scope: req_str("scope")?,
                selector: req_selector("selector")?,
            }),
            "ElementHasText" => {
                let pattern_val = map
                    .get("pattern")
                    .ok_or("ElementHasText missing 'pattern'")?;
                let pattern: TextMatch = serde_yaml::from_value(pattern_val.clone())
                    .map_err(|e| format!("ElementHasText.pattern: {e}"))?;
                Ok(Condition::ElementHasText {
                    scope: req_str("scope")?,
                    selector: req_selector("selector")?,
                    pattern,
                })
            }
            "ElementHasChildren" => Ok(Condition::ElementHasChildren {
                scope: req_str("scope")?,
                selector: req_selector("selector")?,
            }),
            "ElementToggled" => {
                let state = match map.get("state") {
                    None => None,
                    Some(v) => Some(serde_yaml::from_value::<ToggleValue>(v.clone())
                        .map_err(|e| e.to_string())?),
                };
                Ok(Condition::ElementToggled {
                    scope: req_str("scope")?,
                    selector: req_selector("selector")?,
                    state,
                })
            }
            "ElementItemSelected" => {
                let state = map.get("state").and_then(|v| v.as_bool());
                Ok(Condition::ElementItemSelected {
                    scope: req_str("scope")?,
                    selector: req_selector("selector")?,
                    state,
                })
            }
            "ElementSelected" => {
                let pattern_val = map
                    .get("pattern")
                    .ok_or("Selected missing 'pattern'")?;
                let pattern: TextMatch = serde_yaml::from_value(pattern_val.clone())
                    .map_err(|e| format!("Selected.pattern: {e}"))?;
                Ok(Condition::ElementSelected {
                    scope: req_str("scope")?,
                    selector: req_selector("selector")?,
                    pattern,
                })
            }
            "WindowWithAttribute" => {
                let title: Option<TitleMatch> = map
                    .get("title")
                    .and_then(|v| serde_yaml::from_value(v.clone()).ok());
                let automation_id = str_field("automation_id");
                let pid = map.get("pid").and_then(|v| v.as_u64()).map(|v| v as u32);
                if title.is_none() && automation_id.is_none() && pid.is_none() {
                    return Err(
                        "WindowWithAttribute requires at least one of: title, automation_id, pid"
                            .into(),
                    );
                }
                Ok(Condition::WindowWithAttribute {
                    title,
                    automation_id,
                    pid,
                    process: str_field("process"),
                })
            }
            "ProcessRunning" => Ok(Condition::ProcessRunning {
                process: req_str("process")?,
            }),
            "WindowClosed" => Ok(Condition::WindowClosed {
                anchor: req_str("anchor")?,
            }),
            "WindowWithState" => {
                let anchor = req_str("anchor")?;
                let state_str = req_str("state")?;
                let state = match state_str.as_str() {
                    "active" => WindowState::Active,
                    "visible" => WindowState::Visible,
                    other => return Err(format!("unknown WindowState '{other}'")),
                };
                Ok(Condition::WindowWithState { anchor, state })
            }
            "DialogPresent" => Ok(Condition::DialogPresent {
                scope: req_str("scope")?,
            }),
            "DialogAbsent" => Ok(Condition::DialogAbsent {
                scope: req_str("scope")?,
            }),
            "ForegroundIsDialog" => {
                let title = if let Some(t) = map.get("title") {
                    Some(
                        serde_yaml::from_value(t.clone())
                            .map_err(|e| format!("ForegroundIsDialog.title: {e}"))?,
                    )
                } else {
                    None
                };
                Ok(Condition::ForegroundIsDialog { title })
            }
            "FileExists" => Ok(Condition::FileExists {
                path: req_str("path")?,
            }),
            "AllOf" => {
                let conditions = parse_condition_list(map, "conditions", type_str)?;
                Ok(Condition::AllOf { conditions })
            }
            "AnyOf" => {
                let conditions = parse_condition_list(map, "conditions", type_str)?;
                Ok(Condition::AnyOf { conditions })
            }
            "Not" => {
                let inner_val = map
                    .get("condition")
                    .ok_or("Not missing 'condition'")?
                    .clone();
                let condition = Box::new(Condition::try_from(inner_val)?);
                Ok(Condition::Not { condition })
            }
            "TabWithAttribute" => {
                let title: Option<TextMatch> = map
                    .get("title")
                    .and_then(|v| serde_yaml::from_value(v.clone()).ok());
                let url: Option<TextMatch> = map
                    .get("url")
                    .and_then(|v| serde_yaml::from_value(v.clone()).ok());
                if title.is_none() && url.is_none() {
                    return Err("TabWithAttribute requires at least one of: title, url".into());
                }
                Ok(Condition::TabWithAttribute {
                    scope: req_str("scope")?,
                    title,
                    url,
                })
            }
            "TabWithState" => Ok(Condition::TabWithState {
                scope: req_str("scope")?,
                expr: req_str("expr")?,
            }),
            "SnapshotMatches" => {
                let actual = req_str("actual")?;
                let golden = req_str("golden")?;
                let fuzz_pct = map.get("fuzz_pct").and_then(|v| v.as_f64()).unwrap_or_else(default_fuzz_pct);
                let max_diff_pct = map.get("max_diff_pct").and_then(|v| v.as_f64()).unwrap_or_else(default_max_diff_pct);
                let downscale = map.get("downscale").and_then(|v| v.as_u64())
                    .map(|v| v as u32).unwrap_or_else(default_downscale);
                if downscale == 0 {
                    return Err("SnapshotMatches: downscale must be >= 1".into());
                }
                Ok(Condition::SnapshotMatches { actual, golden, fuzz_pct, max_diff_pct, downscale })
            }
            "Always" => Ok(Condition::Always),
            "ExecSucceeded" => Ok(Condition::ExecSucceeded),
            "EvalCondition" => {
                let expr = map
                    .get("expr")
                    .and_then(|v| v.as_str())
                    .ok_or("EvalCondition missing 'expr'")?
                    .to_string();
                Ok(Condition::EvalCondition { expr })
            }
            other => Err(format!("unknown Condition type '{other}'")),
        }
    }
}

fn parse_condition_list(
    map: &serde_yaml::Mapping,
    key: &str,
    type_str: &str,
) -> Result<Vec<Condition>, String> {
    let seq = map
        .get(key)
        .and_then(|v| v.as_sequence())
        .ok_or_else(|| format!("{type_str} missing sequence field '{key}'"))?;
    seq.iter().map(|v| Condition::try_from(v.clone())).collect()
}

// ── describe / scope_name / evaluate ─────────────────────────────────────────

impl Condition {
    /// Return a clone with all `{output.<key>}` tokens substituted in pattern strings.
    pub fn apply_output(&self, locals: &HashMap<String, String>, output: &Output) -> Self {
        let sub = |s: &str| sub_output(s, locals, output);
        let sub_tm = |tm: &TextMatch| TextMatch {
            exact: tm.exact.as_deref().map(|s| sub(s)),
            contains: tm.contains.as_deref().map(|s| sub(s)),
            starts_with: tm.starts_with.as_deref().map(|s| sub(s)),
            regex: tm.regex.clone(),
            non_empty: tm.non_empty,
        };
        match self {
            Condition::ElementHasText {
                scope,
                selector,
                pattern,
            } => Condition::ElementHasText {
                scope: scope.clone(),
                selector: selector.clone(),
                pattern: sub_tm(pattern),
            },
            Condition::ElementSelected {
                scope,
                selector,
                pattern,
            } => Condition::ElementSelected {
                scope: scope.clone(),
                selector: selector.clone(),
                pattern: sub_tm(pattern),
            },
            Condition::AllOf { conditions } => Condition::AllOf {
                conditions: conditions
                    .iter()
                    .map(|c| c.apply_output(locals, output))
                    .collect(),
            },
            Condition::AnyOf { conditions } => Condition::AnyOf {
                conditions: conditions
                    .iter()
                    .map(|c| c.apply_output(locals, output))
                    .collect(),
            },
            Condition::SnapshotMatches { actual, golden, fuzz_pct, max_diff_pct, downscale } => {
                Condition::SnapshotMatches {
                    actual: sub(actual),
                    golden: sub(golden),
                    fuzz_pct: *fuzz_pct,
                    max_diff_pct: *max_diff_pct,
                    downscale: *downscale,
                }
            }
            Condition::FileExists { path } => Condition::FileExists { path: sub(path) },
            Condition::Not { condition } => Condition::Not {
                condition: Box::new(condition.apply_output(locals, output)),
            },
            Condition::TabWithAttribute { scope, title, url } => Condition::TabWithAttribute {
                scope: scope.clone(),
                title: title.as_ref().map(|t| sub_tm(t)),
                url: url.as_ref().map(|u| sub_tm(u)),
            },
            Condition::TabWithState { scope, expr } => Condition::TabWithState {
                scope: scope.clone(),
                expr: sub(expr),
            },
            _ => self.clone(),
        }
    }

    pub fn scope_name(&self) -> Option<&str> {
        match self {
            Condition::ElementFound { scope, .. }
            | Condition::ElementEnabled { scope, .. }
            | Condition::ElementVisible { scope, .. }
            | Condition::ElementHasText { scope, .. }
            | Condition::ElementHasChildren { scope, .. }
            | Condition::ElementToggled { scope, .. }
            | Condition::ElementItemSelected { scope, .. }
            | Condition::ElementSelected { scope, .. }
            | Condition::DialogPresent { scope }
            | Condition::DialogAbsent { scope } => Some(scope),
            _ => None,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Condition::ElementFound { scope, selector } => {
                format!("ElementFound({scope}:{selector})")
            }
            Condition::ElementEnabled { scope, selector } => {
                format!("ElementEnabled({scope}:{selector})")
            }
            Condition::ElementVisible { scope, selector } => {
                format!("ElementVisible({scope}:{selector})")
            }
            Condition::ElementHasText {
                scope, selector, ..
            } => {
                format!("ElementHasText({scope}:{selector})")
            }
            Condition::ElementHasChildren { scope, selector } => {
                format!("ElementHasChildren({scope}:{selector})")
            }
            Condition::ElementToggled { scope, selector, state } => match state {
                Some(ToggleValue::On)            => format!("ElementToggled({scope}:{selector} on)"),
                Some(ToggleValue::Off)           => format!("ElementToggled({scope}:{selector} off)"),
                Some(ToggleValue::Indeterminate) => format!("ElementToggled({scope}:{selector} indeterminate)"),
                None                             => format!("ElementToggled({scope}:{selector})"),
            },
            Condition::ElementItemSelected { scope, selector, state } => match state {
                Some(true)  => format!("ItemSelected({scope}:{selector} selected)"),
                Some(false) => format!("ItemSelected({scope}:{selector} not-selected)"),
                None        => format!("ItemSelected({scope}:{selector})"),
            },
            Condition::ElementSelected { scope, selector, pattern } => {
                format!("Selected({scope}:{selector} {pattern:?})")
            }
            Condition::WindowWithAttribute {
                title,
                automation_id,
                pid,
                process,
            } => {
                let mut parts = Vec::new();
                if let Some(t) = title {
                    parts.push(format!("{t:?}"));
                }
                if let Some(aid) = automation_id {
                    parts.push(format!("automation_id={aid}"));
                }
                if let Some(p) = pid {
                    parts.push(format!("pid={p}"));
                }
                if let Some(p) = process {
                    parts.push(format!("process={p}"));
                }
                format!("WindowWithAttribute({})", parts.join(", "))
            }
            Condition::ProcessRunning { process } => format!("ProcessRunning({process})"),
            Condition::WindowClosed { anchor } => format!("WindowClosed({anchor})"),
            Condition::WindowWithState { anchor, state } => {
                format!("WindowWithState({anchor}:{state:?})")
            }
            Condition::DialogPresent { scope } => format!("DialogPresent({scope})"),
            Condition::DialogAbsent { scope } => format!("DialogAbsent({scope})"),
            Condition::ForegroundIsDialog { .. } => "ForegroundIsDialog".to_string(),
            Condition::Always => "Always".to_string(),
            Condition::ExecSucceeded => "ExecSucceeded".to_string(),
            Condition::SnapshotMatches { actual, golden, fuzz_pct, max_diff_pct, downscale } => {
                format!("SnapshotMatches({actual} vs {golden} downscale={downscale}x fuzz={fuzz_pct}% max_diff={max_diff_pct}%)")
            }
            Condition::AllOf { conditions } => format!(
                "AllOf({})",
                conditions
                    .iter()
                    .map(|c| c.describe())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Condition::AnyOf { conditions } => format!(
                "AnyOf({})",
                conditions
                    .iter()
                    .map(|c| c.describe())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Condition::FileExists { path } => format!("FileExists({path})"),
            Condition::Not { condition } => format!("Not({})", condition.describe()),
            Condition::EvalCondition { expr } => format!("EvalCondition({expr:?})"),
            Condition::TabWithAttribute { scope, .. } => format!("TabWithAttribute({scope})"),
            Condition::TabWithState { scope, expr } => {
                format!("TabWithState({scope}: {expr:?})")
            }
        }
    }

    pub fn evaluate<D: Desktop>(
        &self,
        dom: &mut ShadowDom<D>,
        desktop: &D,
        locals: &std::collections::HashMap<String, String>,
        params: &std::collections::HashMap<String, String>,
        output: &crate::Output,
    ) -> Result<bool, AutomataError> {
        match self {
            Condition::ElementFound { scope, selector } => {
                Ok(find_in_scope(dom, desktop, scope, selector)?.is_some())
            }
            Condition::ElementEnabled { scope, selector } => {
                Ok(find_in_scope(dom, desktop, scope, selector)?
                    .and_then(|el| el.is_enabled().ok())
                    .unwrap_or(false))
            }
            Condition::ElementVisible { scope, selector } => {
                Ok(find_in_scope(dom, desktop, scope, selector)?
                    .and_then(|el| el.is_visible().ok())
                    .unwrap_or(false))
            }
            Condition::ElementHasText {
                scope,
                selector,
                pattern,
            } => Ok(find_in_scope(dom, desktop, scope, selector)?
                .and_then(|el| el.text().ok())
                .map(|t| pattern.test(&t))
                .unwrap_or(false)),
            Condition::ElementHasChildren { scope, selector } => {
                Ok(find_in_scope(dom, desktop, scope, selector)?
                    .and_then(|el| el.children().ok())
                    .map(|ch| !ch.is_empty())
                    .unwrap_or(false))
            }
            Condition::ElementToggled { scope, selector, state } => {
                let ts = find_in_scope(dom, desktop, scope, selector)?
                    .map(|el| el.toggle_state())
                    .transpose()?
                    .flatten();
                Ok(match (ts, state) {
                    (None, _)                       => false,
                    (Some(_), None)                 => true,
                    (Some(actual), Some(expected))  => actual == *expected,
                })
            }
            Condition::ElementItemSelected { scope, selector, state } => {
                let sel = find_in_scope(dom, desktop, scope, selector)?
                    .map(|el| el.is_selected())
                    .transpose()?
                    .flatten();
                Ok(match (sel, state) {
                    (None, _) => false,           // no SelectionItemPattern → not selectable
                    (Some(_), None) => true,      // has SelectionItemPattern, any state
                    (Some(actual), Some(expected)) => actual == *expected,
                })
            }
            Condition::ElementSelected { scope, selector, pattern } => {
                let text = find_in_scope(dom, desktop, scope, selector)?
                    .map(|el| el.selection_text())
                    .transpose()?
                    .flatten()
                    .unwrap_or_default();
                Ok(pattern.test(&text))
            }
            Condition::WindowWithAttribute {
                title,
                automation_id,
                pid,
                process,
            } => {
                let proc_filter = process.as_deref().map(|s| s.to_lowercase());
                Ok(desktop
                    .application_windows()
                    .unwrap_or_default()
                    .iter()
                    .filter(|w| {
                        proc_filter.as_deref().map_or(true, |pf| {
                            w.process_name()
                                .map(|n| n.to_lowercase() == pf)
                                .unwrap_or(false)
                        })
                    })
                    .any(|w| {
                        let title_ok = title
                            .as_ref()
                            .map_or(true, |t| w.name().map(|n| t.test(&n)).unwrap_or(false));
                        let aid_ok = automation_id
                            .as_ref()
                            .map_or(true, |aid| w.automation_id().as_deref() == Some(aid));
                        let pid_ok =
                            pid.map_or(true, |p| w.process_id().map_or(false, |wp| wp == p));
                        title_ok && aid_ok && pid_ok
                    }))
            }
            Condition::ProcessRunning { process } => {
                let target = process.to_lowercase();
                Ok(desktop
                    .application_windows()
                    .unwrap_or_default()
                    .iter()
                    .any(|w| {
                        w.process_name()
                            .map(|n| n.to_lowercase() == target)
                            .unwrap_or(false)
                    }))
            }
            Condition::WindowClosed { anchor } => {
                let windows = desktop.application_windows().unwrap_or_default();
                if let Some(hwnd) = dom.anchor_hwnd(anchor) {
                    // HWND-locked anchor: closed when that specific window is gone.
                    Ok(!windows.iter().any(|w| w.hwnd() == Some(hwnd)))
                } else if let Some(pid) = dom.anchor_pid(anchor) {
                    // PID-only anchor (e.g. single-instance process): closed when
                    // no window exists for that process.
                    Ok(!windows
                        .iter()
                        .any(|w| w.process_id().map_or(false, |p| p == pid)))
                } else {
                    // Unpinned anchor: closed when re-resolution fails.
                    Ok(dom.get(anchor, desktop).is_err())
                }
            }
            Condition::WindowWithState { anchor, state } => {
                let el = match dom.get(anchor, desktop).ok().cloned() {
                    Some(e) => e,
                    None => return Ok(false),
                };
                Ok(match state {
                    WindowState::Active => {
                        let fg = match desktop.foreground_window() {
                            Some(w) => w,
                            None => return Ok(false),
                        };
                        el.process_id().unwrap_or(0) != 0
                            && el.process_id().ok() == fg.process_id().ok()
                    }
                    WindowState::Visible => el.is_visible().unwrap_or(false),
                })
            }
            Condition::DialogPresent { scope } => has_dialog_child(dom, desktop, scope),
            Condition::DialogAbsent { scope } => Ok(!has_dialog_child(dom, desktop, scope)?),
            Condition::ForegroundIsDialog { title } => {
                let fg = match desktop.foreground_window() {
                    Some(w) => w,
                    None => return Ok(false),
                };
                if fg.role() != "dialog" {
                    return Ok(false);
                }
                if let Some(tm) = title {
                    if !tm.test(&fg.name().unwrap_or_default()) {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Condition::AllOf { conditions } => {
                for c in conditions {
                    if !c.evaluate(dom, desktop, locals, params, output)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Condition::AnyOf { conditions } => {
                for c in conditions {
                    if c.evaluate(dom, desktop, locals, params, output)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Condition::SnapshotMatches { actual, golden, fuzz_pct, max_diff_pct, downscale } => {
                let golden_path = std::path::Path::new(golden.as_str());
                if !golden_path.exists() && params.get("__create_goldens").map(String::as_str) == Some("1") {
                    if let Some(parent) = golden_path.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| AutomataError::Internal(format!("create golden dir: {e}")))?;
                    }
                    std::fs::copy(actual.as_str(), golden_path)
                        .map_err(|e| AutomataError::Internal(format!("create golden: {e}")))?;
                    log::info!("snapshot: created golden {golden}");
                    return Ok(true);
                }
                let load = |path: &str| image::open(path).ok();
                let (Some(img_a), Some(img_g)) = (load(actual), load(golden)) else {
                    return Ok(false);
                };
                let a = img_a.to_rgba8();
                let g = img_g.to_rgba8();
                let (aw, ah) = a.dimensions();
                let (gw, gh) = g.dimensions();
                if aw.abs_diff(gw) > 2 || ah.abs_diff(gh) > 2 {
                    return Err(AutomataError::ConditionFalse(
                        format!("dimension mismatch: actual={aw}x{ah} golden={gw}x{gh}")
                    ));
                }
                let luma = |p: &image::Rgba<u8>| -> i32 {
                    (0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64) as i32
                };
                let a_luma_full: Vec<i32> = (0..ah).flat_map(|y| (0..aw).map(move |x| (x, y)))
                    .map(|(x, y)| luma(a.get_pixel(x, y))).collect();
                let g_luma_full: Vec<i32> = (0..gh).flat_map(|y| (0..gw).map(move |x| (x, y)))
                    .map(|(x, y)| luma(g.get_pixel(x, y))).collect();

                // Apply box-filter downscale before comparison. The default (k=8) suppresses
                // high-frequency rendering jitter (text antialiasing) and exposes low-frequency
                // pattern differences (1px solid vs 1px-dotted lines collapse to different mean
                // intensities at the same coarse position).
                let k = (*downscale).max(1);
                let (a_buf, aw2, ah2) = if k == 1 { (a_luma_full, aw, ah) } else { downscale_luma(&a_luma_full, aw, ah, k) };
                let (g_buf, gw2, gh2) = if k == 1 { (g_luma_full, gw, gh) } else { downscale_luma(&g_luma_full, gw, gh, k) };
                let w = aw2.min(gw2);
                let h = ah2.min(gh2);
                if w == 0 || h == 0 {
                    return Err(AutomataError::ConditionFalse(
                        format!("image too small for downscale={k}: {aw}x{ah}")
                    ));
                }

                let threshold = (255.0 * fuzz_pct / 100.0) as i32;
                // Symmetric morphological comparison: handles 1px rendering shifts without
                // hiding genuine regressions. Each pixel must find a close match in the
                // other image's 3×3 neighborhood, checked in both directions.
                let min_neighbor_dist = |val: i32, luma: &[i32], x: u32, y: u32, iw: u32, ih: u32| -> i32 {
                    let mut min_d = i32::MAX;
                    for dy in -1i32..=1 {
                        for dx in -1i32..=1 {
                            let nx = x as i32 + dx;
                            let ny = y as i32 + dy;
                            if nx >= 0 && ny >= 0 && (nx as u32) < iw && (ny as u32) < ih {
                                let d = (val - luma[(ny as u32 * iw + nx as u32) as usize]).abs();
                                if d < min_d { min_d = d; }
                            }
                        }
                    }
                    min_d
                };
                let total = (w * h) as usize;
                let failing = (0..h).flat_map(|y| (0..w).map(move |x| (x, y)))
                    .filter(|&(x, y)| {
                        let la = a_buf[(y * aw2 + x) as usize];
                        let lg = g_buf[(y * gw2 + x) as usize];
                        !(min_neighbor_dist(la, &g_buf, x, y, gw2, gh2) <= threshold
                            && min_neighbor_dist(lg, &a_buf, x, y, aw2, ah2) <= threshold)
                    })
                    .count();
                if failing == 0 {
                    return Ok(true);
                }
                let diff_pct = failing as f64 / total as f64 * 100.0;
                if *max_diff_pct > 0.0 && diff_pct <= *max_diff_pct {
                    return Ok(true);
                }
                Err(AutomataError::ConditionFalse(
                    format!("{failing}/{total} pixels differ ({diff_pct:.2}%)")
                ))
            }
            Condition::Always => Ok(true),
            Condition::ExecSucceeded => {
                match locals.get(EXEC_EXIT_CODE_KEY).map(String::as_str) {
                    Some("0") => Ok(true),
                    Some(code) => Err(AutomataError::ConditionFalse(
                        format!("exec exited with code {code}")
                    )),
                    None => Ok(false),
                }
            }
            Condition::FileExists { path } => Ok(std::path::Path::new(path).exists()),
            Condition::Not { condition } => {
                Ok(!condition.evaluate(dom, desktop, locals, params, output)?)
            }
            Condition::EvalCondition { expr } => {
                crate::expression::eval_bool_expr(expr, locals, params, output)
                    .map_err(|e| AutomataError::Internal(format!("EvalCondition: {e}")))
            }
            Condition::TabWithAttribute { scope, title, url } => {
                let tab_id = match dom.tab_handle(scope) {
                    Some(h) => h.tab_id.clone(),
                    None => return Ok(false),
                };
                let info = desktop
                    .browser()
                    .tab_info(&tab_id)
                    .map_err(|e| AutomataError::Internal(format!("tab_info: {e}")))?;
                let title_ok = title.as_ref().map_or(true, |t| t.test(&info.title));
                let url_ok = url.as_ref().map_or(true, |u| u.test(&info.url));
                Ok(title_ok && url_ok)
            }
            Condition::TabWithState { scope, expr } => {
                let tab_id = match dom.tab_handle(scope) {
                    Some(h) => h.tab_id.clone(),
                    None => return Ok(false),
                };
                let result = desktop
                    .browser()
                    .eval(&tab_id, expr)
                    .map_err(|e| AutomataError::Internal(format!("TabWithState eval: {e}")))?;
                Ok(result.trim() == "true")
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn find_in_scope<D: Desktop>(
    dom: &mut ShadowDom<D>,
    desktop: &D,
    scope: &str,
    selector: &SelectorPath,
) -> Result<Option<D::Elem>, AutomataError> {
    if scope == "_desktop" {
        for window in desktop.application_windows().unwrap_or_default() {
            if let Some(el) = selector.find_one(&window) {
                return Ok(Some(el));
            }
        }
        return Ok(None);
    }
    dom.find_descendant(scope, selector, desktop)
}

fn has_dialog_child<D: Desktop>(
    dom: &mut ShadowDom<D>,
    desktop: &D,
    scope: &str,
) -> Result<bool, AutomataError> {
    let root = match dom.get(scope, desktop).ok().cloned() {
        Some(el) => el,
        None => return Ok(false),
    };
    Ok(root
        .children()
        .unwrap_or_default()
        .iter()
        .any(|c| c.role() == "dialog"))
}
