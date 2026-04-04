use ui_automata::mock::{MockDesktop, MockElement};
use ui_automata::{AnchorDef, Element, SelectorPath, ShadowDom};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn sel(s: &str) -> SelectorPath {
    SelectorPath::parse(s).unwrap()
}

/// Build a desktop with a main_window that has a toolbar child.
fn make_desktop() -> (MockDesktop, MockElement, MockElement) {
    let toolbar = MockElement::leaf("ToolBar", "Main");
    let window = MockElement::parent("Window", "App", vec![toolbar.clone()]);
    let desktop = MockDesktop::new(vec![window.clone()]);
    (desktop, window, toolbar)
}

// ── Basic get ─────────────────────────────────────────────────────────────────

#[test]
fn get_root_anchor() {
    let (desktop, _window, _toolbar) = make_desktop();
    let mut dom = ShadowDom::new();
    dom.mount(
        vec![AnchorDef::root("main_window", sel("Window[name=App]"))],
        &desktop,
    )
    .unwrap();

    let el = dom.get("main_window", &desktop).unwrap();
    assert_eq!(el.name(), Some("App".into()));
}

#[test]
fn get_stable_anchor_via_parent() {
    let (desktop, _window, _toolbar) = make_desktop();
    let mut dom = ShadowDom::new();
    dom.mount(
        vec![
            AnchorDef::root("main_window", sel("Window[name=App]")),
            AnchorDef::stable("toolbar", "main_window", sel("Window > ToolBar[name=Main]")),
        ],
        &desktop,
    )
    .unwrap();

    let el = dom.get("toolbar", &desktop).unwrap();
    assert_eq!(el.name(), Some("Main".into()));
}

#[test]
fn get_unregistered_name_returns_err() {
    let desktop = MockDesktop::new(vec![]);
    let mut dom = ShadowDom::<MockDesktop>::new();
    assert!(dom.get("nonexistent", &desktop).is_err());
}

#[test]
fn root_not_found_returns_err() {
    let desktop = MockDesktop::new(vec![]); // no windows
    let mut dom = ShadowDom::new();
    let result = dom.mount(
        vec![AnchorDef::root("main_window", sel("Window[name=App]"))],
        &desktop,
    );
    assert!(result.is_err());
}

// ── Stale handle re-query ─────────────────────────────────────────────────────

#[test]
fn stale_stable_re_queries_from_parent() {
    let toolbar = MockElement::leaf("ToolBar", "Main");
    let window = MockElement::parent("Window", "App", vec![toolbar.clone()]);
    let desktop = MockDesktop::new(vec![window.clone()]);

    let mut dom = ShadowDom::new();
    dom.mount(
        vec![
            AnchorDef::root("main_window", sel("Window[name=App]")),
            AnchorDef::stable("toolbar", "main_window", sel("Window > ToolBar[name=Main]")),
        ],
        &desktop,
    )
    .unwrap();

    dom.get("toolbar", &desktop).unwrap();

    // Kill the toolbar handle — simulates a stale COM pointer
    toolbar.kill();

    // Replace child with a fresh handle so re-query finds something live.
    let fresh_toolbar = MockElement::leaf("ToolBar", "Main");
    window.set_children(vec![fresh_toolbar]);

    let result = dom.get("toolbar", &desktop);
    assert!(
        result.is_ok(),
        "expected re-query to succeed: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().name(), Some("Main".into()));
}

#[test]
fn stale_stable_walks_up_to_grandparent() {
    let button = MockElement::leaf("Button", "Open");
    let toolbar = MockElement::parent("ToolBar", "Main", vec![button.clone()]);
    let window = MockElement::parent("Window", "App", vec![toolbar.clone()]);
    let desktop = MockDesktop::new(vec![window.clone()]);

    let mut dom = ShadowDom::new();
    dom.mount(
        vec![
            AnchorDef::root("main_window", sel("Window[name=App]")),
            AnchorDef::stable("toolbar", "main_window", sel("Window > ToolBar[name=Main]")),
            AnchorDef::stable("open_btn", "toolbar", sel("ToolBar > Button[name=Open]")),
        ],
        &desktop,
    )
    .unwrap();

    dom.get("toolbar", &desktop).unwrap();
    dom.get("open_btn", &desktop).unwrap();

    toolbar.kill();
    button.kill();

    let fresh_btn = MockElement::leaf("Button", "Open");
    let fresh_toolbar = MockElement::parent("ToolBar", "Main", vec![fresh_btn]);
    window.set_children(vec![fresh_toolbar]);

    let result = dom.get("open_btn", &desktop);
    assert!(
        result.is_ok(),
        "walk-up re-query failed: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().name(), Some("Open".into()));
}

// ── Session invalidation ──────────────────────────────────────────────────────

#[test]
fn invalidate_session_removes_dependents() {
    let info_grid = MockElement::leaf("Tree", "Info");
    let sim_window = MockElement::parent("Window", "Simulator", vec![info_grid]);
    let main_window = MockElement::parent("Window", "App", vec![]);
    let desktop = MockDesktop::new(vec![main_window, sim_window]);

    let mut dom = ShadowDom::new();
    dom.mount(
        vec![
            AnchorDef::root("main_window", sel("Window[name=App]")),
            AnchorDef::session("simulator", sel("Window[name=Simulator]")),
            AnchorDef::stable("info_grid", "simulator", sel("Window >> Tree[name=Info]")),
        ],
        &desktop,
    )
    .unwrap();

    dom.get("simulator", &desktop).unwrap();
    dom.get("info_grid", &desktop).unwrap();
    assert!(dom.is_live("simulator"));
    assert!(dom.is_live("info_grid"));

    dom.invalidate_session("simulator");

    assert!(!dom.is_live("simulator"));
    assert!(!dom.is_live("info_grid"));
}

// ── Unmount ───────────────────────────────────────────────────────────────────

#[test]
fn unmount_removes_anchor_completely() {
    let dialog = MockElement::leaf("Dialog", "Open File");
    let window = MockElement::parent("Window", "App", vec![dialog.clone()]);
    let desktop = MockDesktop::new(vec![window]);

    let mut dom = ShadowDom::new();
    dom.mount(
        vec![AnchorDef::root("main_window", sel("Window[name=App]"))],
        &desktop,
    )
    .unwrap();

    dom.insert("dialog", dialog);
    assert!(dom.is_live("dialog"));

    dom.unmount(&["dialog"], &desktop);
    assert!(!dom.is_live("dialog"));
}

// ── Direct insert ─────────────────────────────────────────────────────────────

#[test]
fn insert_and_get_returns_element() {
    let el = MockElement::leaf("Dialog", "Confirm");
    let desktop = MockDesktop::new(vec![]);
    let mut dom = ShadowDom::<MockDesktop>::new();

    dom.insert("confirm_dialog", el.clone());
    let got = dom.get("confirm_dialog", &desktop).unwrap();
    assert_eq!(got.name(), Some("Confirm".into()));
}
