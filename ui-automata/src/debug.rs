use std::fmt::Write;

use crate::Element;

/// Format a UI element tree for debugging, limited to `max_depth` levels.
///
/// Each line: `<indent>[<role>] "<name>"`. Children are indented two spaces
/// per level. At the depth limit, a `...` line is emitted if children exist.
pub fn dump_tree<E: Element>(el: &E, max_depth: usize) -> String {
    let mut out = String::new();
    dump_node(el, 0, max_depth, &mut out);
    out
}

fn dump_node<E: Element>(el: &E, depth: usize, max_depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    let name = el.name().unwrap_or_else(|| "<unnamed>".to_string());
    let role = el.role();
    if role == "text" {
        let text = el.text().unwrap_or_default();
        if !text.is_empty() && text != name {
            let _ = writeln!(out, "{indent}[{role}] {name:?}: {text:?}");
        } else {
            let _ = writeln!(out, "{indent}[{role}] {name:?}");
        }
    } else {
        let _ = writeln!(out, "{indent}[{role}] {name:?}");
    }

    if depth < max_depth {
        let children = el.children().unwrap_or_default();
        for child in &children {
            dump_node(child, depth + 1, max_depth, out);
        }
    } else {
        let has_children = el.children().map(|c| !c.is_empty()).unwrap_or(false);
        if has_children {
            let _ = writeln!(out, "{indent}  ... (depth limit)");
        }
    }
}
