use ui_automata::mock::MockElement;
use ui_automata::{Element, SelectorPath};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn sel(s: &str) -> SelectorPath {
    SelectorPath::parse(s).unwrap_or_else(|e| panic!("parse failed for '{s}': {e}"))
}

// ── Parsing ───────────────────────────────────────────────────────────────────

#[test]
fn parse_single_role() {
    assert!(SelectorPath::parse("Button").is_ok());
}

#[test]
fn parse_bracketed_predicate() {
    assert!(SelectorPath::parse("[role=Button]").is_ok());
    assert!(SelectorPath::parse("[name=Open]").is_ok());
}

#[test]
fn parse_role_with_name() {
    assert!(SelectorPath::parse("Button[name=Open]").is_ok());
}

#[test]
fn parse_contains_op() {
    assert!(SelectorPath::parse("Window[title~=Mastercam]").is_ok());
}

#[test]
fn parse_startswith_op() {
    assert!(SelectorPath::parse("Window[title^=Processing]").is_ok());
}

#[test]
fn parse_endswith_op() {
    assert!(SelectorPath::parse("[name$=Save]").is_ok());
}

#[test]
fn parse_leading_descendant() {
    assert!(SelectorPath::parse(">> [role=Button]").is_ok());
}

#[test]
fn parse_child_combinator() {
    assert!(SelectorPath::parse("Window > ToolBar").is_ok());
}

#[test]
fn parse_descendant_combinator() {
    assert!(SelectorPath::parse("Window >> Button[name=Open]").is_ok());
}

#[test]
fn parse_nth() {
    assert!(SelectorPath::parse("Button:nth(2)").is_ok());
}

#[test]
fn parse_wildcard_nth() {
    assert!(SelectorPath::parse("*:nth(3)").is_ok());
    assert!(SelectorPath::parse("Pane > *:nth(6)").is_ok());
}

#[test]
fn parse_complex_path() {
    assert!(
        SelectorPath::parse("Window[title~=Mastercam] >> ToolBar[name=Mastercam] > Group:nth(0)")
            .is_ok()
    );
}

#[test]
fn parse_empty_fails() {
    assert!(SelectorPath::parse("").is_err());
}

// ── Single-step matches ───────────────────────────────────────────────────────

#[test]
fn matches_exact_role() {
    let el = MockElement::leaf("Button", "Open");
    assert!(sel("Button").matches(&el));
    assert!(!sel("Edit").matches(&el));
}

#[test]
fn matches_exact_name() {
    let el = MockElement::leaf("Button", "Open");
    assert!(sel("[name=Open]").matches(&el));
    assert!(!sel("[name=Close]").matches(&el));
}

#[test]
fn matches_role_and_name() {
    let el = MockElement::leaf("Button", "Open");
    assert!(sel("Button[name=Open]").matches(&el));
    assert!(!sel("Edit[name=Open]").matches(&el));
}

#[test]
fn matches_contains() {
    let el = MockElement::leaf("Window", "Vector Designer");
    assert!(sel("Window[name~=Designer]").matches(&el));
    assert!(!sel("Window[name~=Fusion]").matches(&el));
}

#[test]
fn matches_startswith() {
    let el = MockElement::leaf("Window", "Processing operations...");
    assert!(sel("Window[name^=Processing]").matches(&el));
    assert!(!sel("Window[name^=Exporting]").matches(&el));
}

#[test]
fn matches_endswith() {
    let el = MockElement::leaf("Button", "Don\u{2019}t Save");
    assert!(sel("[name$=Save]").matches(&el));
    assert!(!sel("[name$=Cancel]").matches(&el));
}

#[test]
fn matches_startswith_and_endswith_combined() {
    // Both predicates must hold — simulates matching "Don't Save" regardless
    // of whether the apostrophe is straight (U+0027) or curly (U+2019).
    let curly = MockElement::leaf("Button", "Don\u{2019}t Save");
    let straight = MockElement::leaf("Button", "Don't Save");
    let save = MockElement::leaf("Button", "Save");
    let cancel = MockElement::leaf("Button", "Cancel");

    for el in [&curly, &straight] {
        assert!(
            sel("[name^=Don][name$=Save]").matches(el),
            "should match Don*Save"
        );
    }
    assert!(!sel("[name^=Don][name$=Save]").matches(&save));
    assert!(!sel("[name^=Don][name$=Save]").matches(&cancel));
}

#[test]
fn multi_predicate_all_must_match() {
    // An element matching role but not name should not be returned.
    let root = MockElement::parent(
        "Window",
        "App",
        vec![
            MockElement::leaf("Button", "Save"),
            MockElement::leaf("Button", "Cancel"),
        ],
    );
    let results = sel("Window > Button[name=Save]").find_all(&root);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name(), Some("Save".into()));
}

// ── Child combinator > ────────────────────────────────────────────────────────

#[test]
fn child_finds_direct_child() {
    let root = MockElement::parent(
        "Window",
        "App",
        vec![
            MockElement::leaf("ToolBar", "Main"),
            MockElement::leaf("Button", "Open"),
        ],
    );
    let result: Option<MockElement> = sel("Window > ToolBar[name=Main]").find_one(&root);
    assert!(result.is_some());
    assert_eq!(result.unwrap().name(), Some("Main".into()));
}

#[test]
fn child_does_not_find_grandchild() {
    let grandchild = MockElement::leaf("Button", "Open");
    let child = MockElement::parent("Pane", "Container", vec![grandchild]);
    let root = MockElement::parent("Window", "App", vec![child]);

    // > only goes one level deep
    let result = sel("Window > Button[name=Open]").find_one(&root);
    assert!(result.is_none());
}

#[test]
fn child_returns_none_when_no_match() {
    let root = MockElement::parent("Window", "App", vec![MockElement::leaf("Button", "Save")]);
    assert!(sel("Window > Button[name=Open]").find_one(&root).is_none());
}

// ── Descendant combinator >> ──────────────────────────────────────────────────

#[test]
fn descendant_finds_direct_child() {
    let root = MockElement::parent("Window", "App", vec![MockElement::leaf("Button", "Open")]);
    assert!(sel("Window >> Button[name=Open]").find_one(&root).is_some());
}

#[test]
fn descendant_finds_deep_element() {
    let btn = MockElement::leaf("Button", "Open");
    let pane = MockElement::parent("Pane", "Inner", vec![btn]);
    let toolbar = MockElement::parent("ToolBar", "Main", vec![pane]);
    let root = MockElement::parent("Window", "App", vec![toolbar]);

    assert!(sel("Window >> Button[name=Open]").find_one(&root).is_some());
}

#[test]
fn descendant_returns_none_when_absent() {
    let root = MockElement::parent("Window", "App", vec![MockElement::leaf("Button", "Save")]);
    assert!(sel("Window >> Button[name=Open]").find_one(&root).is_none());
}

// ── :nth ──────────────────────────────────────────────────────────────────────

#[test]
fn nth_selects_correct_sibling() {
    let root = MockElement::parent(
        "ToolBar",
        "Main",
        vec![
            MockElement::leaf("Button", "A"),
            MockElement::leaf("Button", "B"),
            MockElement::leaf("Button", "C"),
        ],
    );
    let result: Option<MockElement> = sel("ToolBar > Button:nth(1)").find_one(&root);
    assert!(result.is_some());
    assert_eq!(result.unwrap().name(), Some("B".into()));
}

#[test]
fn nth_zero_is_first() {
    let root = MockElement::parent(
        "ToolBar",
        "Main",
        vec![
            MockElement::leaf("Button", "First"),
            MockElement::leaf("Button", "Second"),
        ],
    );
    let result: Option<MockElement> = sel("ToolBar > Button:nth(0)").find_one(&root);
    assert_eq!(result.unwrap().name(), Some("First".into()));
}

#[test]
fn nth_out_of_bounds_returns_none() {
    let root = MockElement::parent("ToolBar", "Main", vec![MockElement::leaf("Button", "Only")]);
    assert!(sel("ToolBar > Button:nth(5)").find_one(&root).is_none());
}

/// `:nth` with `>>` (descendant combinator) — picks the nth matching descendant.
#[test]
fn nth_descendant() {
    let root = MockElement::parent(
        "Pane",
        "Root",
        vec![
            MockElement::parent("Group", "G1", vec![MockElement::leaf("Button", "A")]),
            MockElement::parent("Group", "G2", vec![MockElement::leaf("Button", "B")]),
            MockElement::parent("Group", "G3", vec![MockElement::leaf("Button", "C")]),
        ],
    );
    let result = sel("Pane >> Button:nth(2)").find_one(&root);
    assert_eq!(result.unwrap().name(), Some("C".into()));
}

/// `:nth` skips non-matching siblings — counts only elements that satisfy the predicate.
#[test]
fn nth_counts_only_matching_role() {
    let root = MockElement::parent(
        "Pane",
        "Root",
        vec![
            MockElement::leaf("Text", "label"),
            MockElement::leaf("Button", "X"), // Button :nth(0)
            MockElement::leaf("Text", "label2"),
            MockElement::leaf("Button", "Y"), // Button :nth(1)
            MockElement::leaf("Button", "Z"), // Button :nth(2)
        ],
    );
    let result = sel("Pane > Button:nth(1)").find_one(&root);
    assert_eq!(result.unwrap().name(), Some("Y".into()));
}

/// `*:nth(n)` — selects the nth child regardless of role.
#[test]
fn wildcard_nth_counts_all_children() {
    let root = MockElement::parent(
        "Pane",
        "Root",
        vec![
            MockElement::leaf("Button", "A"),
            MockElement::leaf("Button", "B"),
            MockElement::leaf("Button", "C"),
            MockElement::leaf("Button", "D"),
            MockElement::leaf("MenuItem", "sep1"),
            MockElement::leaf("Button", "E"),
            MockElement::leaf("MenuItem", "target"), // overall :nth(6)
            MockElement::leaf("Button", "F"),
        ],
    );
    let result = sel("Pane > *:nth(6)").find_one(&root);
    assert_eq!(result.unwrap().name(), Some("target".into()));
}

/// `:nth` in a non-final step — selects the nth match then continues the path.
#[test]
fn nth_in_non_final_step() {
    let root = MockElement::parent(
        "ToolBar",
        "Main",
        vec![
            MockElement::parent("Pane", "row0", vec![MockElement::leaf("Button", "wrong")]),
            MockElement::parent("Pane", "row1", vec![MockElement::leaf("Button", "correct")]),
        ],
    );
    // Navigate to the second Pane (:nth(1)), then find its Button child.
    let result = sel("ToolBar > Pane:nth(1) > Button").find_one(&root);
    assert_eq!(result.unwrap().name(), Some("correct".into()));
}

// ── Leading >> (scope-relative search) ───────────────────────────────────────

#[test]
fn leading_descendant_searches_within_root() {
    // Leading `>>` means "find anywhere under the scope root" without needing
    // to match the root element itself. Useful when the scope anchor IS the
    // container (e.g., scope = dialog, selector = >> [role=edit]).
    let edit = MockElement::leaf("Edit", "File name:");
    let pane = MockElement::parent("Pane", "", vec![edit]);
    let dialog = MockElement::parent("Dialog", "Save As", vec![pane]);

    let result = sel(">> [role=Edit][name='File name:']").find_one(&dialog);
    assert!(result.is_some(), "should find edit nested inside pane");
}

#[test]
fn leading_descendant_multi_step() {
    let edit = MockElement::leaf("Edit", "File name:");
    let combo = MockElement::parent("ComboBox", "File name:", vec![edit]);
    let pane = MockElement::parent("Pane", "", vec![combo]);
    let dialog = MockElement::parent("Dialog", "Save As", vec![pane]);

    let result = sel(">> [role=ComboBox][name='File name:'] >> [role=Edit]").find_one(&dialog);
    assert!(
        result.is_some(),
        "should find edit inside combo box inside pane"
    );
}

// ── Multi-step paths ──────────────────────────────────────────────────────────

#[test]
fn multi_step_descendant_then_descendant() {
    // A >> B >> C — all three steps must use Descendant (parser bug regression).
    let btn = MockElement::leaf("Button", "Open");
    let combo = MockElement::parent("ComboBox", "File", vec![btn]);
    let pane = MockElement::parent("Pane", "", vec![combo]);
    let dialog = MockElement::parent("Dialog", "Save As", vec![pane]);
    let window = MockElement::parent("Window", "App", vec![dialog]);

    let result = sel("Window >> Dialog >> Button[name=Open]").find_one(&window);
    assert!(
        result.is_some(),
        "A >> B >> C should find deeply nested element"
    );
}

#[test]
fn multi_step_child_then_descendant() {
    let btn = MockElement::leaf("Button", "Open");
    let inner_pane = MockElement::parent("Pane", "Inner", vec![btn]);
    let toolbar = MockElement::parent("ToolBar", "Main", vec![inner_pane]);
    let root = MockElement::parent("Window", "App", vec![toolbar]);

    // Window > ToolBar[name=Main] >> Button[name=Open]
    let result = sel("Window > ToolBar[name=Main] >> Button[name=Open]").find_one(&root);
    assert!(result.is_some());
}

// ── OR values (pipe syntax) ───────────────────────────────────────────────────

#[test]
fn parse_or_value() {
    assert!(SelectorPath::parse("[name=Editor|Designer]").is_ok());
    assert!(SelectorPath::parse("[name~=Editor|Designer]").is_ok());
}

#[test]
fn or_exact_matches_first_alternative() {
    let el = MockElement::leaf("Window", "Editor");
    assert!(sel("[name=Editor|Designer]").matches(&el));
}

#[test]
fn or_exact_matches_second_alternative() {
    let el = MockElement::leaf("Window", "Designer");
    assert!(sel("[name=Editor|Designer]").matches(&el));
}

#[test]
fn or_exact_rejects_neither_alternative() {
    let el = MockElement::leaf("Window", "Lathe");
    assert!(!sel("[name=Editor|Designer]").matches(&el));
}

#[test]
fn or_contains_matches_any_substring() {
    let mill = MockElement::leaf("Window", "Vector Editor");
    let design = MockElement::leaf("Window", "Vector Designer");
    let lathe = MockElement::leaf("Window", "Nothing");
    assert!(sel("[name~=Editor|Designer]").matches(&mill));
    assert!(sel("[name~=Editor|Designer]").matches(&design));
    assert!(!sel("[name~=Editor|Designer]").matches(&lathe));
}

#[test]
fn or_startswith_any_prefix() {
    let processing = MockElement::leaf("Window", "Processing NCI data");
    let exporting = MockElement::leaf("Window", "Exporting toolpaths");
    let other = MockElement::leaf("Window", "Something else");
    assert!(sel("[name^=Processing|Exporting]").matches(&processing));
    assert!(sel("[name^=Processing|Exporting]").matches(&exporting));
    assert!(!sel("[name^=Processing|Exporting]").matches(&other));
}

#[test]
fn or_finds_multiple_via_find_all() {
    let root = MockElement::parent(
        "Window",
        "App",
        vec![
            MockElement::leaf("Window", "Vector Designer"),
            MockElement::leaf("Window", "Designer Studio"),
            MockElement::leaf("Window", "Nothing"),
        ],
    );
    let results = sel("Window > [name~=Editor|Designer]").find_all(&root);
    assert_eq!(results.len(), 2);
}

// ── automation_id / id= ───────────────────────────────────────────────────────

#[test]
fn parse_id_predicate() {
    assert!(SelectorPath::parse("[id=SplashOverlay]").is_ok());
    assert!(SelectorPath::parse("[automation_id=SplashOverlay]").is_ok());
}

#[test]
fn id_exact_matches() {
    let el = MockElement::leaf("Window", "").with_automation_id("SplashOverlay");
    assert!(sel("[id=SplashOverlay]").matches(&el));
    assert!(!sel("[id=OtherOverlay]").matches(&el));
}

#[test]
fn automation_id_alias_matches() {
    // Both spellings should produce the same result.
    let el = MockElement::leaf("Window", "").with_automation_id("SplashOverlay");
    assert!(sel("[automation_id=SplashOverlay]").matches(&el));
}

#[test]
fn id_missing_does_not_match() {
    // Element with no automation_id never matches an [id=...] predicate.
    let el = MockElement::leaf("Window", "SplashOverlay");
    assert!(!sel("[id=SplashOverlay]").matches(&el));
}

#[test]
fn id_combined_with_process_predicate() {
    // Simulate desktop scan: only the window with the right id is matched.
    let splash = MockElement::leaf("Window", "").with_automation_id("SplashOverlay");
    let main = MockElement::leaf("Window", "Mastercam 2025");
    let desktop = MockElement::parent("Pane", "Desktop", vec![splash, main]);

    let results = sel("Pane > [id=SplashOverlay]").find_all(&desktop);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].automation_id(), Some("SplashOverlay".into()));
}

#[test]
fn find_all_returns_multiple() {
    let root = MockElement::parent(
        "Window",
        "App",
        vec![
            MockElement::leaf("Button", "A"),
            MockElement::leaf("Button", "B"),
            MockElement::leaf("Edit", "C"),
        ],
    );
    let results = sel("Window > Button").find_all(&root);
    assert_eq!(results.len(), 2);
}

// ── :parent and :ancestor(n) ──────────────────────────────────────────────────

#[test]
fn parse_parent() {
    assert!(SelectorPath::parse("[role=button]:parent").is_ok());
    assert!(SelectorPath::parse(">> [role=button][name=X]:parent").is_ok());
}

#[test]
fn parse_ancestor() {
    assert!(SelectorPath::parse("[role=button]:ancestor(1)").is_ok());
    assert!(SelectorPath::parse(">> [role=button]:ancestor(3)").is_ok());
}

#[test]
fn parse_nth_and_parent_combined() {
    // :nth then :parent — find 2nd button then go up
    assert!(SelectorPath::parse(">> Button:nth(1):parent").is_ok());
    // :parent then :nth in next step
    assert!(SelectorPath::parse(">> Button:parent > *:nth(9)").is_ok());
}

#[test]
fn parent_returns_container() {
    // Tree: Root > Container > Button
    // Selecting Button:parent should return Container
    let btn = MockElement::leaf("Button", "Open");
    let container = MockElement::parent("Pane", "Container", vec![btn]);
    let root = MockElement::parent("Window", "App", vec![container]);

    let result = sel("Window >> Button[name=Open]:parent").find_one(&root);
    assert!(result.is_some(), "should find the parent of Button");
    assert_eq!(result.unwrap().name(), Some("Container".into()));
}

#[test]
fn parent_of_root_step_returns_none() {
    // When the matched element has no parent wired (it IS the root), returns None.
    let root = MockElement::leaf("Button", "Open");
    // root has no parent set — should return None
    let result = sel("Button[name=Open]:parent").find_one(&root);
    assert!(result.is_none(), "root element has no parent");
}

#[test]
fn ancestor_1_equals_parent() {
    let btn = MockElement::leaf("Button", "X");
    let pane = MockElement::parent("Pane", "P", vec![btn]);
    let root = MockElement::parent("Window", "App", vec![pane]);

    let via_parent = sel("Window >> Button:parent").find_one(&root);
    let via_ancestor = sel("Window >> Button:ancestor(1)").find_one(&root);
    assert_eq!(
        via_parent.map(|e| e.name()),
        via_ancestor.map(|e| e.name()),
        ":ancestor(1) should equal :parent"
    );
}

#[test]
fn ancestor_2_returns_grandparent() {
    let btn = MockElement::leaf("Button", "X");
    let inner = MockElement::parent("Pane", "Inner", vec![btn]);
    let outer = MockElement::parent("Group", "Outer", vec![inner]);
    let root = MockElement::parent("Window", "App", vec![outer]);

    let result = sel("Window >> Button:ancestor(2)").find_one(&root);
    assert!(result.is_some());
    assert_eq!(result.unwrap().name(), Some("Outer".into()));
}

#[test]
fn parent_mid_selector_then_child() {
    // The key use-case: find Performance button's parent, then select *:nth(9)
    // Tree: Window > Toolbar > [Perf, A, B, C, D, E, F, G, H, FastFwd, ...]
    //                           0     1  2  3  4  5  6  7  8  9
    let fast_fwd = MockElement::leaf("Button", "FastForward");
    let toolbar = MockElement::parent(
        "ToolBar",
        "Bottom",
        vec![
            MockElement::leaf("Button", "Performance"),
            MockElement::leaf("Button", "A"),
            MockElement::leaf("Button", "B"),
            MockElement::leaf("Button", "C"),
            MockElement::leaf("Button", "D"),
            MockElement::leaf("Button", "E"),
            MockElement::leaf("Button", "F"),
            MockElement::leaf("Button", "G"),
            MockElement::leaf("Button", "H"),
            fast_fwd,
        ],
    );
    let root = MockElement::parent("Window", "App", vec![toolbar]);

    let result = sel("Window >> [role=Button][name=Performance]:parent > *:nth(9)").find_one(&root);
    assert!(result.is_some(), "should find FastForward at index 9");
    assert_eq!(result.unwrap().name(), Some("FastForward".into()));
}

#[test]
fn parent_mid_selector_descendant_after() {
    // :parent then >> (descendant) in the next step
    let edit = MockElement::leaf("Edit", "value");
    let group = MockElement::parent("Group", "G", vec![edit]);
    let toolbar = MockElement::parent(
        "ToolBar",
        "T",
        vec![MockElement::leaf("Button", "Trigger"), group],
    );
    let root = MockElement::parent("Window", "App", vec![toolbar]);

    // Find Trigger:parent = ToolBar, then >> Edit within it
    let result = sel("Window >> [name=Trigger]:parent >> Edit").find_one(&root);
    assert!(result.is_some());
    assert_eq!(result.unwrap().name(), Some("value".into()));
}

#[test]
fn ancestor_out_of_bounds_returns_none() {
    // Only 2 levels deep; :ancestor(5) should return None
    let btn = MockElement::leaf("Button", "X");
    let pane = MockElement::parent("Pane", "P", vec![btn]);
    let root = MockElement::parent("Window", "App", vec![pane]);

    let result = sel("Window >> Button:ancestor(5)").find_one(&root);
    assert!(
        result.is_none(),
        "should return None when ancestors run out"
    );
}

#[test]
fn display_parent_roundtrip() {
    let s = ">> [role=button][name=Performance]:parent";
    let parsed = SelectorPath::parse(s).unwrap();
    assert!(parsed.to_string().contains(":parent"));
}

#[test]
fn display_ancestor_roundtrip() {
    let s = ">> [role=button][name=Performance]:ancestor(2)";
    let parsed = SelectorPath::parse(s).unwrap();
    assert!(parsed.to_string().contains(":ancestor(2)"));
}

#[test]
fn find_all_with_parent() {
    // Multiple buttons — find_all with :parent returns their shared parent (deduplicated or not)
    let btn_a = MockElement::leaf("Button", "A");
    let btn_b = MockElement::leaf("Button", "B");
    let pane = MockElement::parent("Pane", "Container", vec![btn_a, btn_b]);
    let root = MockElement::parent("Window", "App", vec![pane]);

    // Both buttons have the same parent; find_all returns both (even if same parent)
    let results = sel("Window >> Button:parent").find_all(&root);
    assert!(!results.is_empty(), "should find at least one parent");
    // All results should be named "Container"
    for r in &results {
        assert_eq!(r.name(), Some("Container".into()));
    }
}

#[test]
fn nth_then_parent() {
    // :nth(1):parent — take the 2nd button then go to its parent
    let toolbar = MockElement::parent(
        "ToolBar",
        "T",
        vec![
            MockElement::leaf("Button", "B0"),
            MockElement::leaf("Button", "B1"),
        ],
    );
    let root = MockElement::parent("Window", "App", vec![toolbar]);

    let result = sel("Window >> Button:nth(1):parent").find_one(&root);
    assert!(result.is_some());
    assert_eq!(result.unwrap().name(), Some("T".into()));
}

// ── Unbalanced bracket detection ─────────────────────────────────────────────

#[test]
fn unclosed_bracket_simple_fails() {
    // Simple unclosed '[' at segment level.
    assert!(SelectorPath::parse("[name=foo").is_err());
}

#[test]
fn unclosed_bracket_with_combinator_inside_fails() {
    // The original bug: '>>' inside '[...]' was treated as a combinator,
    // causing the bracket depth to never reach 0 before the segment boundary
    // was accepted. After the fix, depth != 0 at end-of-input is caught.
    assert!(SelectorPath::parse("> [role=pane][name=Toolpaths >> [role=tree]").is_err());
}

#[test]
fn unclosed_bracket_at_last_segment_fails() {
    // Last segment has an unclosed '[' — no combinator involved.
    assert!(SelectorPath::parse("> [role=pane][name=Toolpaths] >> [role=tree").is_err());
}

#[test]
fn balanced_brackets_with_space_in_name_ok() {
    // The intended correct form of the selector above should succeed.
    assert!(SelectorPath::parse("> [role=pane][name=Toolpaths] >> [role=tree]").is_ok());
}
