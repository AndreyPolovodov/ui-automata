use ui_automata::mock::{MockDesktop, MockElement};
use ui_automata::{AnchorDef, SelectorPath, ShadowDom};

fn sel(s: &str) -> SelectorPath {
    SelectorPath::parse(s).unwrap()
}

fn desktop_with(window: MockElement) -> MockDesktop {
    MockDesktop::new(vec![window])
}

// ── Helpers to assert on change lines ────────────────────────────────────────

fn has_added(changes: &[String], label: &str) -> bool {
    changes
        .iter()
        .any(|c| c.contains("ADDED") && c.contains(label))
}

fn has_removed(changes: &[String], label: &str) -> bool {
    changes
        .iter()
        .any(|c| c.contains("REMOVED") && c.contains(label))
}

// ── Baseline: no changes on stable tree ──────────────────────────────────────

#[test]
fn no_changes_when_tree_is_stable() {
    let window = MockElement::parent("window", "App", vec![MockElement::leaf("button", "OK")]);
    let desktop = desktop_with(window);
    let mut dom = ShadowDom::new();
    dom.mount(vec![AnchorDef::root("win", sel("[name=App]"))], &desktop)
        .unwrap();

    // First sync after mount: snapshot already set at resolve time, nothing changed.
    let changes = dom.sync_changes("win", &desktop);
    assert!(changes.is_empty(), "expected no changes, got: {changes:#?}");

    // Second sync: still nothing.
    let changes = dom.sync_changes("win", &desktop);
    assert!(
        changes.is_empty(),
        "expected no changes on second sync, got: {changes:#?}"
    );
}

// ── Child added ───────────────────────────────────────────────────────────────

#[test]
fn detects_child_added() {
    let window = MockElement::parent("window", "App", vec![]);
    let desktop = desktop_with(window.clone());
    let mut dom = ShadowDom::new();
    dom.mount(vec![AnchorDef::root("win", sel("[name=App]"))], &desktop)
        .unwrap();

    window.set_children(vec![MockElement::leaf("button", "OK")]);

    let changes = dom.sync_changes("win", &desktop);
    assert!(!changes.is_empty(), "expected changes");
    assert!(
        has_added(&changes, "button"),
        "expected ADDED button, got: {changes:#?}"
    );
    assert!(
        has_added(&changes, "OK"),
        "expected 'OK' in ADDED line, got: {changes:#?}"
    );
}

// ── Child removed ─────────────────────────────────────────────────────────────

#[test]
fn detects_child_removed() {
    let window = MockElement::parent("window", "App", vec![MockElement::leaf("button", "OK")]);
    let desktop = desktop_with(window.clone());
    let mut dom = ShadowDom::new();
    dom.mount(vec![AnchorDef::root("win", sel("[name=App]"))], &desktop)
        .unwrap();

    window.set_children(vec![]);

    let changes = dom.sync_changes("win", &desktop);
    assert!(!changes.is_empty(), "expected changes");
    assert!(
        has_removed(&changes, "button"),
        "expected REMOVED button, got: {changes:#?}"
    );
}

// ── Popup prepended (the Format/File menu case) ───────────────────────────────
//
// This is the critical real-world scenario: a menu popup is inserted at the
// front of the window's children. The existing `[edit "Text Editor"]` sibling
// must NOT be reported as added or removed — only the new popup should appear.

#[test]
fn popup_prepended_only_reports_new_element() {
    let edit = MockElement::leaf("edit", "Text Editor");
    let window = MockElement::parent("window", "App", vec![edit.clone()]);
    let desktop = desktop_with(window.clone());
    let mut dom = ShadowDom::new();
    dom.mount(vec![AnchorDef::root("win", sel("[name=App]"))], &desktop)
        .unwrap();

    // Prepend popup menu before the existing edit (matches UIA behaviour).
    let menu_item = MockElement::leaf("menu item", "Font...");
    let popup = MockElement::parent("menu", "Format", vec![menu_item]);
    window.set_children(vec![popup, edit]);

    let changes = dom.sync_changes("win", &desktop);

    // Popup and its child must appear as ADDED.
    assert!(
        has_added(&changes, "menu"),
        "expected ADDED menu, got: {changes:#?}"
    );
    assert!(
        has_added(&changes, "Font"),
        "expected ADDED menu item, got: {changes:#?}"
    );

    // The pre-existing edit element must NOT appear in the changes.
    assert!(
        !changes.iter().any(|c| c.contains("edit")),
        "edit should not appear in changes (it was already present): {changes:#?}",
    );
}

// ── Popup removed ─────────────────────────────────────────────────────────────

#[test]
fn popup_removed_after_dismiss() {
    let edit = MockElement::leaf("edit", "Text Editor");
    let popup = MockElement::parent(
        "menu",
        "Format",
        vec![MockElement::leaf("menu item", "Font...")],
    );
    let window = MockElement::parent("window", "App", vec![popup, edit.clone()]);
    let desktop = desktop_with(window.clone());
    let mut dom = ShadowDom::new();
    dom.mount(vec![AnchorDef::root("win", sel("[name=App]"))], &desktop)
        .unwrap();

    // Dismiss popup.
    window.set_children(vec![edit]);

    let changes = dom.sync_changes("win", &desktop);

    assert!(
        has_removed(&changes, "menu"),
        "expected REMOVED menu, got: {changes:#?}"
    );
    assert!(
        !changes.iter().any(|c| c.contains("edit")),
        "edit should not appear in changes: {changes:#?}",
    );
}

// ── Multiple children with same role+name ─────────────────────────────────────
//
// Two buttons named "Item" (e.g. toolbar items). Adding a third should report
// exactly one ADDED, not a spurious remove+add.

#[test]
fn nth_of_kind_add() {
    let window = MockElement::parent(
        "window",
        "App",
        vec![
            MockElement::leaf("button", "Item"),
            MockElement::leaf("button", "Item"),
        ],
    );
    let desktop = desktop_with(window.clone());
    let mut dom = ShadowDom::new();
    dom.mount(vec![AnchorDef::root("win", sel("[name=App]"))], &desktop)
        .unwrap();

    window.set_children(vec![
        MockElement::leaf("button", "Item"),
        MockElement::leaf("button", "Item"),
        MockElement::leaf("button", "Item"),
    ]);

    let changes = dom.sync_changes("win", &desktop);

    let added: Vec<_> = changes.iter().filter(|c| c.contains("ADDED")).collect();
    let removed: Vec<_> = changes.iter().filter(|c| c.contains("REMOVED")).collect();
    assert_eq!(added.len(), 1, "expected 1 ADDED, got: {changes:#?}");
    assert!(removed.is_empty(), "expected no REMOVED, got: {changes:#?}");
}

// ── Grandchild change is detected ────────────────────────────────────────────
//
// A grandchild (depth 2 from the anchor root) appearing should be reported.

#[test]
fn grandchild_added_detected() {
    let toolbar = MockElement::parent("toolbar", "Main", vec![]);
    let window = MockElement::parent("window", "App", vec![toolbar.clone()]);
    let desktop = desktop_with(window.clone());
    let mut dom = ShadowDom::new();
    dom.mount(vec![AnchorDef::root("win", sel("[name=App]"))], &desktop)
        .unwrap();

    toolbar.set_children(vec![MockElement::leaf("button", "Open")]);

    let changes = dom.sync_changes("win", &desktop);

    assert!(
        has_added(&changes, "button"),
        "expected ADDED button, got: {changes:#?}"
    );
    assert!(
        has_added(&changes, "Open"),
        "expected 'Open' in ADDED, got: {changes:#?}"
    );
}

// ── Child name change collapses to CHANGED ────────────────────────────────────
//
// When a child element keeps the same role but its name changes (e.g. a status
// bar text node updating its cursor position), the diff should emit a single
// "name X → Y" event rather than a REMOVED + ADDED pair.

#[test]
fn child_name_change_collapses_to_changed() {
    let text = MockElement::leaf("text", "  Ln 1, Col 12");
    let status = MockElement::parent("status bar", "Status Bar", vec![text.clone()]);
    let window = MockElement::parent("window", "App", vec![status.clone()]);
    let desktop = desktop_with(window.clone());
    let mut dom = ShadowDom::new();
    dom.mount(vec![AnchorDef::root("win", sel("[name=App]"))], &desktop)
        .unwrap();

    // Simulate cursor move: text child is replaced with a new name.
    status.set_children(vec![MockElement::leaf("text", "  Ln 2, Col 14")]);

    let changes = dom.sync_changes("win", &desktop);

    // Must have exactly one change mentioning both old and new names.
    assert!(!changes.is_empty(), "expected a change event");
    let changed: Vec<_> = changes.iter().filter(|c| c.contains("→")).collect();
    assert_eq!(
        changed.len(),
        1,
        "expected one name-change event, got: {changes:#?}"
    );
    assert!(
        changed[0].contains("Ln 1"),
        "expected old name in change: {changed:#?}"
    );
    assert!(
        changed[0].contains("Ln 2"),
        "expected new name in change: {changed:#?}"
    );

    // Must NOT emit a bare REMOVED or ADDED.
    let removed: Vec<_> = changes.iter().filter(|c| c.contains("REMOVED")).collect();
    let added: Vec<_> = changes.iter().filter(|c| c.contains("ADDED")).collect();
    assert!(removed.is_empty(), "expected no REMOVED, got: {changes:#?}");
    assert!(added.is_empty(), "expected no ADDED, got: {changes:#?}");
}

// ── No duplicate events across consecutive syncs ──────────────────────────────

#[test]
fn changes_not_repeated_on_next_sync() {
    let window = MockElement::parent("window", "App", vec![]);
    let desktop = desktop_with(window.clone());
    let mut dom = ShadowDom::new();
    dom.mount(vec![AnchorDef::root("win", sel("[name=App]"))], &desktop)
        .unwrap();

    window.set_children(vec![MockElement::leaf("button", "OK")]);

    // First sync: detects the add.
    let first = dom.sync_changes("win", &desktop);
    assert!(!first.is_empty(), "expected changes on first sync");

    // Second sync: tree hasn't changed again, no new events.
    let second = dom.sync_changes("win", &desktop);
    assert!(second.is_empty(), "changes should not repeat: {second:#?}");
}
