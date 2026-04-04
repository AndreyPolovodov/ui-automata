/// Point-in-time snapshot of a UI element and its (limited-depth) child tree.
///
/// Used by [`ShadowDom::sync`] to diff the live UIA tree against the last known
/// state and emit change events to the trace.
///
/// # Identity
///
/// Siblings are matched by `(role, name)`. This is stable across insertions and
/// deletions (e.g., a popup prepended at index 0 does not displace existing
/// siblings). When multiple siblings share the same `(role, name)` they are
/// matched positionally within that group.
///
/// # Change format
///
/// Each change is a single line:
/// ```text
/// dom: notepad: ADDED [menu "Format"]
/// dom: notepad > [menu "Format"]: ADDED [menu item "Word Wrap"]
/// dom: notepad > [menu "Format"]: ADDED [menu item "Font..."]
/// dom: notepad > [window "*Untitled - Notepad"]: name "Untitled - Notepad" → "*Untitled - Notepad"
/// dom: notepad: REMOVED [menu "Format"]
/// ```
use std::collections::BTreeMap;

use crate::Element;

/// Depth at which `ShadowDom::sync` snapshots element subtrees.
pub(crate) const SNAP_DEPTH: usize = 2;

pub(crate) struct SnapNode {
    pub role: String,
    pub name: String,
    /// Text content, captured only for "text"-role elements where `text()`
    /// may differ from or supplement the accessible `name`.
    pub text: Option<String>,
    pub children: Vec<SnapNode>,
}

impl SnapNode {
    /// Render this node and its subtree as an indented tree string.
    pub fn format_tree(&self, indent: usize) -> String {
        let label = elem_label(&self.role, &self.name);
        let prefix = "  ".repeat(indent);
        let line = match &self.text {
            Some(t) if !t.is_empty() && t != &self.name => {
                format!("{prefix}{label}: {t:?}")
            }
            _ => format!("{prefix}{label}"),
        };
        let mut lines = vec![line];
        for child in &self.children {
            lines.push(child.format_tree(indent + 1));
        }
        lines.join("\n")
    }

    /// Capture a snapshot of `el` and its subtree up to `depth` levels.
    pub fn capture<E: Element>(el: &E, depth: usize) -> Self {
        let name = el.name().unwrap_or_default();
        let role = el.role();
        let text = if role == "text" { el.text().ok() } else { None };
        let children = if depth > 0 {
            el.children()
                .unwrap_or_default()
                .into_iter()
                .map(|c| SnapNode::capture(&c, depth - 1))
                .collect()
        } else {
            vec![]
        };
        SnapNode {
            role,
            name,
            text,
            children,
        }
    }

    /// Diff `self` (old) against `new`, appending change-event lines to `out`.
    ///
    /// `path` is the display path *to* this node (used as the parent context
    /// when reporting changes to its children).
    pub fn diff_into(&self, new: &SnapNode, path: &str, out: &mut Vec<String>) {
        // Root element name change (e.g., window title acquires an asterisk).
        if self.name != new.name {
            out.push(format!(
                "dom: {path}: name {:?} → {:?}",
                self.name, new.name
            ));
        }
        // Text content change (only relevant for "text"-role elements, and only
        // when the text carries information beyond the accessible name). When
        // both sides have text == name, the name-change event already covers it.
        if self.text != new.text {
            let was_mirror = self.text.as_deref() == Some(self.name.as_str());
            let is_mirror = new.text.as_deref() == Some(new.name.as_str());
            if !(was_mirror && is_mirror) {
                out.push(format!(
                    "dom: {path}: text {:?} → {:?}",
                    self.text, new.text
                ));
            }
        }
        diff_children(path, &self.children, &new.children, out);
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Diff two child lists, emitting add/remove/recurse events into `out`.
/// `parent_path` is the display path of the parent element.
fn diff_children(parent_path: &str, old: &[SnapNode], new: &[SnapNode], out: &mut Vec<String>) {
    // Group siblings by (role, name) — stable identity key.
    // BTreeMap gives deterministic (alphabetical) output order.
    let mut old_groups: BTreeMap<(String, String), Vec<&SnapNode>> = BTreeMap::new();
    for n in old {
        old_groups
            .entry((n.role.clone(), n.name.clone()))
            .or_default()
            .push(n);
    }
    let mut new_groups: BTreeMap<(String, String), Vec<&SnapNode>> = BTreeMap::new();
    for n in new {
        new_groups
            .entry((n.role.clone(), n.name.clone()))
            .or_default()
            .push(n);
    }

    // Union of all (role, name) keys seen in either snapshot.
    let all_keys: BTreeMap<(String, String), ()> = old_groups
        .keys()
        .chain(new_groups.keys())
        .map(|k| (k.clone(), ()))
        .collect();

    let empty: Vec<&SnapNode> = vec![];

    // Orphan removes/adds grouped by role — used below to detect CHANGED pairs.
    let mut orphan_removes: BTreeMap<String, Vec<&SnapNode>> = BTreeMap::new();
    let mut orphan_adds: BTreeMap<String, Vec<&SnapNode>> = BTreeMap::new();

    for (key, _) in &all_keys {
        let old_vec = old_groups.get(key).unwrap_or(&empty);
        let new_vec = new_groups.get(key).unwrap_or(&empty);
        let label = elem_label(&key.0, &key.1);

        // Orphan adds: new_vec has more entries than old_vec.
        for node in new_vec.iter().skip(old_vec.len()) {
            orphan_adds.entry(key.0.clone()).or_default().push(node);
        }

        // Orphan removes: old_vec has more entries than new_vec.
        for node in old_vec.iter().skip(new_vec.len()) {
            orphan_removes.entry(key.0.clone()).or_default().push(node);
        }

        // Matched pairs: recurse.
        let match_count = old_vec.len().min(new_vec.len());
        for i in 0..match_count {
            let child_path = if match_count > 1 {
                format!("{parent_path} > {label}[{i}]")
            } else {
                format!("{parent_path} > {label}")
            };
            old_vec[i].diff_into(new_vec[i], &child_path, out);
        }
    }

    // Second pass: pair orphan removes and adds that share the same role.
    // A 1-remove + 1-add of the same role is almost always the same logical
    // element whose name/content changed (e.g. a status-bar text child updating
    // its cursor position). Emit as CHANGED via diff_into rather than
    // REMOVED + ADDED.
    let all_orphan_roles: BTreeMap<String, ()> = orphan_removes
        .keys()
        .chain(orphan_adds.keys())
        .map(|r| (r.clone(), ()))
        .collect();

    for (role, _) in &all_orphan_roles {
        let removes = orphan_removes.get(role).map(Vec::as_slice).unwrap_or(&[]);
        let adds = orphan_adds.get(role).map(Vec::as_slice).unwrap_or(&[]);
        let change_count = removes.len().min(adds.len());

        // Paired: emit CHANGED old → new, then recurse for any child diffs.
        for i in 0..change_count {
            let old_label = elem_label(&removes[i].role, &removes[i].name);
            let new_label = elem_label(&adds[i].role, &adds[i].name);
            out.push(format!(
                "dom: {parent_path}: CHANGED {old_label} → {new_label}"
            ));
            // Recurse into children in case sub-elements also changed.
            let child_path = if change_count > 1 {
                format!("{parent_path} > [{role}][{i}]")
            } else {
                format!("{parent_path} > [{role}]")
            };
            diff_children(&child_path, &removes[i].children, &adds[i].children, out);
        }

        // Excess removes with no matching add.
        for node in removes.iter().skip(change_count) {
            let label = elem_label(&node.role, &node.name);
            out.push(format!("dom: {parent_path}: REMOVED {label}"));
        }

        // Excess adds with no matching remove.
        for node in adds.iter().skip(change_count) {
            out.push(format!(
                "dom: {parent_path}: ADDED\n{}",
                node.format_tree(0)
            ));
        }
    }
}

fn elem_label(role: &str, name: &str) -> String {
    if name.is_empty() {
        format!("[{role}]")
    } else {
        format!("[{role} {name:?}]")
    }
}
