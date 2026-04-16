use schemars::JsonSchema;
use serde::Deserialize;

use std::collections::HashMap;

use crate::{
    AutomataError, Browser, Desktop, Element, SelectorPath, ShadowDom, action::sub_output,
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
            Condition::Always => Ok(true),
            Condition::ExecSucceeded => {
                Ok(locals.get(EXEC_EXIT_CODE_KEY).map(String::as_str) == Some("0"))
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
