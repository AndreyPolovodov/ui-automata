use uiautomation::controls::ControlType;
use uiautomation::core::UICondition;
use uiautomation::patterns::UIValuePattern;
use uiautomation::types::TreeScope;
use uiautomation::{UIAutomation, UIElement as UiaElement};

use crate::Result;
use crate::UIElement as WinElem;
use crate::util::window_pane_condition;

/// A node in the UIA element tree.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ElementNode {
    pub role: String,
    pub name: String,
    #[serde(rename = "id", skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ElementNode>,
}

// ── Element impl for in-memory selector evaluation ───────────────────────────

type R<T> = std::result::Result<T, ui_automata::AutomataError>;

impl ui_automata::Element for ElementNode {
    fn name(&self) -> Option<String> {
        if self.name.is_empty() {
            None
        } else {
            Some(self.name.clone())
        }
    }
    fn role(&self) -> String {
        self.role.clone()
    }
    fn automation_id(&self) -> Option<String> {
        self.automation_id.clone()
    }
    fn bounds(&self) -> R<(i32, i32, i32, i32)> {
        Ok((self.x, self.y, self.width, self.height))
    }
    fn children(&self) -> R<Vec<Self>> {
        Ok(self.children.clone())
    }
    fn text(&self) -> R<String> {
        Ok(self.text.clone().unwrap_or_default())
    }

    // ── not meaningful on a snapshot ─────────────────────────────────────────
    fn inner_text(&self) -> R<String> {
        Ok(String::new())
    }
    fn is_enabled(&self) -> R<bool> {
        Ok(true)
    }
    fn is_visible(&self) -> R<bool> {
        Ok(true)
    }
    fn process_id(&self) -> R<u32> {
        Ok(0)
    }
    fn click(&self) -> R<()> {
        unimplemented!("snapshot")
    }
    fn double_click(&self) -> R<()> {
        unimplemented!("snapshot")
    }
    fn hover(&self) -> R<()> {
        unimplemented!("snapshot")
    }
    fn click_at(&self, _: f64, _: f64, _: ui_automata::ClickType) -> R<()> {
        unimplemented!("snapshot")
    }
    fn type_text(&self, _: &str) -> R<()> {
        unimplemented!("snapshot")
    }
    fn press_key(&self, _: &str) -> R<()> {
        unimplemented!("snapshot")
    }
    fn set_value(&self, _: &str) -> R<()> {
        unimplemented!("snapshot")
    }
    fn focus(&self) -> R<()> {
        unimplemented!("snapshot")
    }
    fn scroll_into_view(&self) -> R<()> {
        unimplemented!("snapshot")
    }
    fn activate_window(&self) -> R<()> {
        unimplemented!("snapshot")
    }
    fn minimize_window(&self) -> R<()> {
        unimplemented!("snapshot")
    }
    fn close(&self) -> R<()> {
        unimplemented!("snapshot")
    }
}

// ── find_elements result types ────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct AncestorInfo {
    pub name: Option<String>,
    pub role: String,
    pub automation_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SiblingInfo {
    pub name: Option<String>,
    pub role: String,
    pub value: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ElementFindResult {
    pub name: Option<String>,
    pub role: String,
    pub value: Option<String>,
    pub enabled: bool,
    pub bounds: Option<(i32, i32, i32, i32)>,
    pub automation_id: Option<String>,
    pub pid: u32,
    pub ancestors: Vec<AncestorInfo>,
    pub siblings: Vec<SiblingInfo>,
}

const MAX_DEPTH: usize = 64;

/// Find a top-level window matching the given pid and/or title, then walk its element tree.
///
/// - Only `pid`: picks the first window belonging to that process.
/// - Only `title`: matches by exact title (original behaviour).
/// - Both: window must match both pid and title.
/// - Neither: returns an error.
/// - `selector`: when `Some`, only matched subtrees are returned as a JSON array.
///   When `None`, the full tree is returned as a JSON object.
pub fn build_element_tree(
    pid: Option<u32>,
    title: Option<&str>,
    automation_id: Option<&str>,
    process: Option<&str>,
    hwnd: Option<u64>,
    max_results: usize,
    selector: Option<&str>,
) -> Result<serde_json::Value> {
    let automation = UIAutomation::new_direct()?;

    let window = if let Some(h) = hwnd {
        automation
            .element_from_handle(uiautomation::types::Handle::from(h as isize))
            .map_err(|e| anyhow::anyhow!("element_from_handle({h:#x}) failed: {e}"))?
    } else {
        if pid.is_none() && title.is_none() && automation_id.is_none() && process.is_none() {
            anyhow::bail!(
                "at least one of hwnd, pid, title, automation_id, or process must be provided"
            );
        }
        let root = automation.get_root_element()?;
        let cond = window_pane_condition(&automation)?;
        let elements = root.find_all(TreeScope::Children, &cond)?;
        elements
            .into_iter()
            .find(|e| matches_window_filters(e, pid, title, automation_id, process))
            .ok_or_else(|| {
                let mut parts = Vec::new();
                if let Some(p) = pid {
                    parts.push(format!("pid={p}"));
                }
                if let Some(t) = title {
                    parts.push(format!("title={t:?}"));
                }
                if let Some(a) = automation_id {
                    parts.push(format!("automation_id={a:?}"));
                }
                if let Some(p) = process {
                    parts.push(format!("process={p:?}"));
                }
                anyhow::anyhow!("No window found with {}", parts.join(" and "))
            })?
    };

    let true_cond = automation.create_true_condition()?;

    if let Some(sel_str) = selector {
        let path = ui_automata::SelectorPath::parse(sel_str)
            .map_err(|e| anyhow::anyhow!("bad selector: {e}"))?;
        let wrapped = WinElem::new(window);
        let matches = path.find_all(&wrapped);
        let subtrees: Vec<ElementNode> = matches
            .iter()
            .take(max_results)
            .filter_map(|el| walk_element(&el.inner, &true_cond, 0).ok())
            .collect();
        return Ok(serde_json::to_value(&subtrees)?);
    }

    let tree = walk_element(&window, &true_cond, 0)?;
    Ok(serde_json::to_value(&tree)?)
}

/// Build a full in-memory `ElementNode` snapshot of the window identified by `hwnd`.
/// The returned tree can be queried with `SelectorPath::find_all` for fast, deterministic
/// selector evaluation without further UIA calls.
pub fn snapshot_tree(hwnd: u64) -> Result<ElementNode> {
    let automation = UIAutomation::new_direct()?;
    let window = automation
        .element_from_handle(uiautomation::types::Handle::from(hwnd as isize))
        .map_err(|e| anyhow::anyhow!("element_from_handle({hwnd:#x}) failed: {e}"))?;
    let true_cond = automation.create_true_condition()?;
    walk_element(&window, &true_cond, 0)
}

/// Find all elements matching `selector` within windows matching the given filters.
/// At least one of `pid`, `title`, `automation_id`, or `process` must be supplied.
///
/// - `include_ancestors` — when true, each result includes the full ancestor chain
///   from the window root down to (but not including) the matched element, root-first.
/// - `include_siblings` — when true, each result includes the other children of the
///   matched element's immediate parent.
/// - `max_results` — caps the total number of matched elements returned across all
///   windows. Iteration stops as soon as this limit is reached.
pub fn find_elements(
    pid: Option<u32>,
    title: Option<&str>,
    automation_id: Option<&str>,
    process: Option<&str>,
    hwnd: Option<u64>,
    selector: &str,
    include_ancestors: bool,
    include_siblings: bool,
    max_results: usize,
) -> Result<Vec<ElementFindResult>> {
    if pid.is_none()
        && title.is_none()
        && automation_id.is_none()
        && process.is_none()
        && hwnd.is_none()
    {
        anyhow::bail!(
            "find_elements requires at least one of: pid, title, automation_id, process, hwnd"
        );
    }

    let path = ui_automata::SelectorPath::parse(selector)
        .map_err(|e| anyhow::anyhow!("bad selector: {e}"))?;

    let uia = UIAutomation::new_direct()?;

    // Build the list of windows to search: either a single hwnd or all top-level windows.
    let windows_to_search: Vec<UiaElement> = if let Some(h) = hwnd {
        let win = uia
            .element_from_handle(uiautomation::types::Handle::from(h as isize))
            .map_err(|e| anyhow::anyhow!("element_from_handle({h:#x}) failed: {e}"))?;
        vec![win]
    } else {
        let root = uia.get_root_element()?;
        let cond = window_pane_condition(&uia)?;
        root.find_all(TreeScope::Children, &cond)?
    };

    let walker = uia.get_raw_view_walker()?;
    let mut results: Vec<ElementFindResult> = Vec::new();

    'outer: for win in &windows_to_search {
        if hwnd.is_none() && !matches_window_filters(win, pid, title, automation_id, process) {
            continue;
        }

        let win_pid = match win.get_process_id() {
            Ok(p) => p,
            Err(_) => continue,
        };

        let wrapped = WinElem::new(win.clone());
        for el in path.find_all(&wrapped) {
            if results.len() >= max_results {
                break 'outer;
            }

            let name = el.inner.get_name().ok().filter(|s| !s.is_empty());
            let role = el.inner.get_localized_control_type().unwrap_or_default();
            let value = el
                .inner
                .get_pattern::<UIValuePattern>()
                .ok()
                .and_then(|p| p.get_value().ok())
                .filter(|s| !s.is_empty());
            let enabled = el.inner.is_enabled().unwrap_or(false);
            let bounds = el
                .inner
                .get_bounding_rectangle()
                .ok()
                .map(|r| (r.get_left(), r.get_top(), r.get_width(), r.get_height()));
            let el_automation_id = el.inner.get_automation_id().ok().filter(|s| !s.is_empty());

            // Walk ancestor chain, staying within the same process.
            let ancestors = if include_ancestors {
                let el_pid = el.inner.get_process_id().ok();
                let mut anc: Vec<AncestorInfo> = Vec::new();
                let mut cur = el.inner.clone();
                loop {
                    match walker.get_parent(&cur) {
                        Ok(parent) => {
                            // Stop when we leave the process (desktop root or cross-process pane).
                            if parent.get_process_id().ok() != el_pid {
                                break;
                            }
                            anc.push(AncestorInfo {
                                name: parent.get_name().ok().filter(|s| !s.is_empty()),
                                role: parent.get_localized_control_type().unwrap_or_default(),
                                automation_id: parent
                                    .get_automation_id()
                                    .ok()
                                    .filter(|s| !s.is_empty()),
                            });
                            cur = parent;
                        }
                        Err(_) => break,
                    }
                }
                anc.reverse(); // root-first order
                anc
            } else {
                vec![]
            };

            let siblings = if include_siblings {
                collect_siblings_of(&el.inner)
            } else {
                vec![]
            };

            results.push(ElementFindResult {
                name,
                role,
                value,
                enabled,
                bounds,
                automation_id: el_automation_id,
                pid: win_pid,
                ancestors,
                siblings,
            });
        }
    }

    Ok(results)
}

/// Return true if `el` matches all supplied window filters (None = wildcard).
fn matches_window_filters(
    el: &UiaElement,
    pid: Option<u32>,
    title: Option<&str>,
    automation_id: Option<&str>,
    process: Option<&str>,
) -> bool {
    let pid_match = pid.map_or(true, |p| el.get_process_id().ok() == Some(p));
    let title_match = title.map_or(true, |t| el.get_name().ok().as_deref() == Some(t));
    let aid_match =
        automation_id.map_or(true, |a| el.get_automation_id().ok().as_deref() == Some(a));
    let proc_match = process.map_or(true, |proc| {
        el.get_process_id()
            .ok()
            .and_then(|p| crate::get_process_name(p as i32).ok())
            .map(|n| n.eq_ignore_ascii_case(proc))
            .unwrap_or(false)
    });
    pid_match && title_match && aid_match && proc_match
}

fn collect_siblings_of(el: &UiaElement) -> Vec<SiblingInfo> {
    let auto = match UIAutomation::new_direct() {
        Ok(a) => a,
        Err(_) => return vec![],
    };
    let walker = match auto.get_raw_view_walker() {
        Ok(w) => w,
        Err(_) => return vec![],
    };
    let parent = match walker.get_parent(el) {
        Ok(p) => p,
        Err(_) => return vec![],
    };
    if parent.get_process_id().ok() == Some(0) {
        return vec![];
    }
    let true_cond = match auto.create_true_condition() {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    // Self-exclusion: prefer automation_id match; fall back to bounds.
    let el_aid = el.get_automation_id().ok().filter(|s| !s.is_empty());
    let el_bounds = el.get_bounding_rectangle().ok();

    parent
        .find_all(TreeScope::Children, &true_cond)
        .unwrap_or_default()
        .into_iter()
        .filter(|child| {
            if let (Some(eid), Ok(caid)) = (&el_aid, child.get_automation_id()) {
                if &caid == eid {
                    return false;
                }
            } else if let (Some(eb), Ok(cb)) = (&el_bounds, child.get_bounding_rectangle()) {
                if eb.get_left() == cb.get_left()
                    && eb.get_top() == cb.get_top()
                    && eb.get_width() == cb.get_width()
                    && eb.get_height() == cb.get_height()
                {
                    return false;
                }
            }
            true
        })
        .map(|child| SiblingInfo {
            name: child.get_name().ok().filter(|s| !s.is_empty()),
            role: child.get_localized_control_type().unwrap_or_default(),
            value: child
                .get_pattern::<UIValuePattern>()
                .ok()
                .and_then(|p| p.get_value().ok())
                .filter(|s| !s.is_empty()),
            enabled: child.is_enabled().unwrap_or(false),
        })
        .collect()
}

fn walk_element(
    element: &UiaElement,
    true_cond: &UICondition,
    depth: usize,
) -> Result<ElementNode> {
    let control_type = element.get_control_type().ok();
    let role = element
        .get_localized_control_type()
        .unwrap_or_else(|_| format!("{:?}", control_type));
    let name = element.get_name().unwrap_or_default();
    let (x, y, width, height) = element
        .get_bounding_rectangle()
        .map(|r| (r.get_left(), r.get_top(), r.get_width(), r.get_height()))
        .unwrap_or((0, 0, 0, 0));

    let text = match control_type {
        Some(ControlType::Edit) | Some(ControlType::Text) => element
            .get_pattern::<UIValuePattern>()
            .ok()
            .and_then(|p| p.get_value().ok())
            .filter(|s| !s.is_empty()),
        _ => None,
    };

    let children = if depth < MAX_DEPTH {
        element
            .find_all(TreeScope::Children, true_cond)
            .unwrap_or_default()
            .iter()
            .map(|child| walk_element(child, true_cond, depth + 1))
            .collect::<Result<Vec<_>>>()?
    } else {
        vec![]
    };

    let automation_id = element.get_automation_id().ok().filter(|s| !s.is_empty());

    Ok(ElementNode {
        role,
        name,
        automation_id,
        text,
        x,
        y,
        width,
        height,
        children,
    })
}
