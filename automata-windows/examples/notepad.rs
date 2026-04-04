/// Notepad smoke test — exercises the ui-automata executor end-to-end.
///
/// Windows 10 only.
///
/// What it does:
///   1. Launches Notepad
///   2. Types "Hello World"
///   3. Opens Format > Font dialog (via menu clicks)
///   4. Reads the current font size, increments by 2, cycling back to 12 after 22
///      using the formula: (size + 2) % 12 + 12
///   5. Confirms with OK
///   6. Saves the file to the default location (triggers Save As dialog)
///   7. Confirms the filename and saves
///   8. Types more text ("Goodbye")
///   9. Closes the window — Notepad shows an unsaved-changes dialog
///  10. Clicks "Don't Save"
///
/// Run from the repo root:
///   cargo run -p automata-windows --example notepad

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("This example only runs on Windows.");
}

#[cfg(target_os = "windows")]
fn main() {
    use std::time::Duration;
    use ui_automata::*;

    automata_windows::init_logging(Some(std::path::Path::new("notepad.log")));
    automata_windows::init_com();
    let desktop = automata_windows::Desktop::new();

    let mut executor = Executor::new(desktop);
    let mut state = ui_automata::WorkflowState::new(false);

    let s = |text: &str| SelectorPath::parse(text).expect("bad selector");

    // ── Phase 1: launch Notepad ───────────────────────────────────────────────
    let notepad_pid = executor
        .desktop
        .open_application("notepad.exe")
        .expect("failed to launch Notepad");
    eprintln!("[notepad] launched with pid={notepad_pid}");

    let launch_steps = vec![Step {
        intent: "wait for Notepad window".into(),
        precondition: None,
        action: Action::NoOp,
        expect: Condition::WindowWithAttribute {
            pid: Some(notepad_pid),
            title: None,
            automation_id: None,
            process: None,
        },
        timeout: Some(Duration::from_secs(15)),
        fallback: None,
        retry: RetryPolicy::None,
        on_failure: OnFailure::Abort,
        on_success: OnSuccess::Continue,
    }];
    executor
        .run(
            &Plan {
                name: "launch_notepad",
                steps: &launch_steps,
                recovery_handlers: vec![],
                max_recoveries: 0,
                unmount: &[],
                default_timeout: DEFAULT_TIMEOUT,
                default_retry: RetryPolicy::None,
            },
            &mut state,
        )
        .expect("Notepad window did not appear");

    executor
        .mount(vec![
            AnchorDef::session("notepad", s("[name~=Notepad]")).with_pid(notepad_pid),
            AnchorDef::stable(
                "editor",
                "notepad",
                s("[name~=Notepad] >> [role=edit][name='Text Editor']"),
            ),
            AnchorDef::stable(
                "menubar",
                "notepad",
                s("[name~=Notepad] >> [role='menu bar'][name=Application]"),
            ),
        ])
        .expect("failed to mount anchors");

    // ── Phase 2: type text ────────────────────────────────────────────────────
    let type_steps = vec![Step {
        intent: "type Hello World".into(),
        precondition: None,
        action: Action::TypeText {
            scope: "editor".into(),
            selector: s("[role=edit][name='Text Editor']"),
            text: "Hello World".into(),
        },
        expect: Condition::ElementHasText {
            scope: "editor".into(),
            selector: s("[role=edit][name='Text Editor']"),
            pattern: TextMatch::contains("Hello World"),
        },
        timeout: Some(Duration::from_secs(5)),
        fallback: None,
        retry: RetryPolicy::None,
        on_failure: OnFailure::Abort,
        on_success: OnSuccess::Continue,
    }];
    executor
        .run(
            &Plan {
                name: "type_hello_world",
                steps: &type_steps,
                recovery_handlers: vec![],
                max_recoveries: 0,
                unmount: &[],
                default_timeout: DEFAULT_TIMEOUT,
                default_retry: RetryPolicy::None,
            },
            &mut state,
        )
        .expect("typing failed");

    // ── Phase 3: open Format > Font dialog ───────────────────────────────────
    let font_sel = s("[name~=Notepad] >> [role='menu item'][name~=Font]");

    let font_dialog_steps = vec![
        Step {
            intent: "click Format menu and wait for popup".into(),
            precondition: None,
            action: Action::Click {
                scope: "menubar".into(),
                selector: s("[role='menu bar'] > [role='menu item'][name=Format]"),
            },
            expect: Condition::ElementFound {
                scope: "notepad".into(),
                selector: font_sel.clone(),
            },
            timeout: Some(Duration::from_secs(5)),
            fallback: None,
            retry: RetryPolicy::None,
            on_failure: OnFailure::Abort,
            on_success: OnSuccess::Continue,
        },
        Step {
            intent: "click Font menu item in popup".into(),
            precondition: None,
            action: Action::Click {
                scope: "notepad".into(),
                selector: font_sel,
            },
            expect: Condition::DialogPresent {
                scope: "notepad".into(),
            },
            timeout: Some(Duration::from_secs(5)),
            fallback: None,
            retry: RetryPolicy::Fixed {
                count: 1,
                delay: Duration::from_millis(300),
            },
            on_failure: OnFailure::Abort,
            on_success: OnSuccess::Continue,
        },
    ];
    executor
        .run(
            &Plan {
                name: "open_font_dialog",
                steps: &font_dialog_steps,
                recovery_handlers: vec![],
                max_recoveries: 0,
                unmount: &[],
                default_timeout: DEFAULT_TIMEOUT,
                default_retry: RetryPolicy::None,
            },
            &mut state,
        )
        .expect("Font dialog did not open");

    // ── Phase 4: increment font size by 2 (cycling back to 12 after 22) ────────
    executor
        .mount(vec![AnchorDef::ephemeral(
            "font_dialog",
            "notepad",
            s("[name~=Notepad] >> [role=dialog][name=Font]"),
        )])
        .expect("failed to mount font_dialog anchor");

    let size_sel = s(
        "[role=dialog][name=Font] >> [role='combo box'][name='Size:'] > [role=edit][name='Size:']",
    );
    let set_size_steps = vec![
        Step {
            intent: "read current font size".into(),
            precondition: None,
            action: Action::Extract {
                key: "size".into(),
                scope: "font_dialog".into(),
                selector: size_sel.clone(),
                attribute: ExtractAttribute::Text,
                multiple: false,
                local: true,
            },
            expect: Condition::Always,
            timeout: Some(Duration::from_secs(5)),
            fallback: None,
            retry: RetryPolicy::None,
            on_failure: OnFailure::Abort,
            on_success: OnSuccess::Continue,
        },
        Step {
            intent: "compute next font size".into(),
            precondition: None,
            action: Action::Eval {
                key: "new_size".into(),
                expr: "(size + 2) % 12 + 12".into(),
                output: None,
            },
            expect: Condition::Always,
            timeout: Some(Duration::from_secs(5)),
            fallback: None,
            retry: RetryPolicy::None,
            on_failure: OnFailure::Abort,
            on_success: OnSuccess::Continue,
        },
        Step {
            intent: "set font size to computed value".into(),
            precondition: None,
            action: Action::SetValue {
                scope: "font_dialog".into(),
                selector: size_sel.clone(),
                value: "{output.new_size}".into(),
            },
            expect: Condition::ElementHasText {
                scope: "font_dialog".into(),
                selector: size_sel.clone(),
                pattern: TextMatch::non_empty(),
            },
            timeout: Some(Duration::from_secs(5)),
            fallback: None,
            retry: RetryPolicy::None,
            on_failure: OnFailure::Abort,
            on_success: OnSuccess::Continue,
        },
        Step {
            intent: "click OK to confirm font".into(),
            precondition: None,
            action: Action::Click {
                scope: "font_dialog".into(),
                selector: s("[role=dialog][name=Font] > [role=button][name=OK]"),
            },
            expect: Condition::DialogAbsent {
                scope: "notepad".into(),
            },
            timeout: Some(Duration::from_secs(5)),
            fallback: None,
            retry: RetryPolicy::None,
            on_failure: OnFailure::Abort,
            on_success: OnSuccess::Continue,
        },
    ];
    let set_size_unmount = vec!["font_dialog".to_owned()];
    executor
        .run(
            &Plan {
                name: "set_font_size",
                steps: &set_size_steps,
                recovery_handlers: vec![],
                max_recoveries: 0,
                unmount: &set_size_unmount,
                default_timeout: DEFAULT_TIMEOUT,
                default_retry: RetryPolicy::None,
            },
            &mut state,
        )
        .expect("failed to set font size");

    // ── Phase 5: save via File > Save ─────────────────────────────────────────
    executor
        .mount(vec![AnchorDef::ephemeral(
            "saveas_dialog",
            "notepad",
            s("[name~=Notepad] >> [role=dialog][name='Save As']"),
        )])
        .expect("failed to register saveas anchor");

    let save_steps = vec![
        Step {
            intent: "click File menu and wait for popup".into(),
            precondition: None,
            action: Action::Click {
                scope: "menubar".into(),
                selector: s("[role='menu bar'] > [role='menu item'][name=File]"),
            },
            expect: Condition::ElementFound {
                scope: "notepad".into(),
                selector: s("[name~=Notepad] >> [role='menu item'][name^=Save]"),
            },
            timeout: Some(Duration::from_secs(5)),
            fallback: None,
            retry: RetryPolicy::None,
            on_failure: OnFailure::Abort,
            on_success: OnSuccess::Continue,
        },
        Step {
            intent: "click Save in File menu".into(),
            precondition: None,
            action: Action::Click {
                scope: "notepad".into(),
                selector: s("[name~=Notepad] >> [role='menu item'][name^=Save]"),
            },
            expect: Condition::DialogPresent {
                scope: "notepad".into(),
            },
            timeout: Some(Duration::from_secs(5)),
            fallback: None,
            retry: RetryPolicy::Fixed {
                count: 1,
                delay: Duration::from_millis(300),
            },
            on_failure: OnFailure::Abort,
            on_success: OnSuccess::Continue,
        },
        Step {
            intent: "type filename in Save As dialog".into(),
            precondition: None,
            action: Action::SetValue {
                scope: "saveas_dialog".into(),
                selector: s(">> [role=combo box][name='File name:'] >> [role=edit]"),
                value: "hello-world.txt".into(),
            },
            expect: Condition::ElementHasText {
                scope: "saveas_dialog".into(),
                selector: s(">> [role=combo box][name='File name:'] >> [role=edit]"),
                pattern: TextMatch::contains("hello-world.txt"),
            },
            timeout: Some(Duration::from_secs(5)),
            fallback: None,
            retry: RetryPolicy::None,
            on_failure: OnFailure::Abort,
            on_success: OnSuccess::Continue,
        },
        Step {
            intent: "click Save button".into(),
            precondition: None,
            action: Action::Invoke {
                scope: "saveas_dialog".into(),
                selector: s(">> [role=button][name=Save]"),
            },
            expect: Condition::DialogAbsent {
                scope: "notepad".into(),
            },
            timeout: Some(Duration::from_secs(5)),
            fallback: None,
            retry: RetryPolicy::None,
            on_failure: OnFailure::Abort,
            on_success: OnSuccess::Continue,
        },
    ];
    let save_handlers = vec![RecoveryHandler {
        name: "confirm_overwrite".into(),
        trigger: Condition::ElementFound {
            scope: "notepad".into(),
            selector: s(">> [role=dialog][name='Confirm Save As']"),
        },
        actions: vec![Action::Click {
            scope: "notepad".into(),
            selector: s(">> [role=dialog][name='Confirm Save As'] >> [role=button][name=Yes]"),
        }],
        resume: ResumeStrategy::RetryStep,
    }];
    let save_unmount = vec!["saveas_dialog".to_owned()];
    executor
        .run(
            &Plan {
                name: "save_file",
                steps: &save_steps,
                recovery_handlers: save_handlers,
                max_recoveries: 1,
                unmount: &save_unmount,
                default_timeout: DEFAULT_TIMEOUT,
                default_retry: RetryPolicy::None,
            },
            &mut state,
        )
        .expect("save failed");

    // ── Phase 6: edit more text ───────────────────────────────────────────────
    let edit_steps = vec![Step {
        intent: "append Goodbye".into(),
        precondition: None,
        action: Action::TypeText {
            scope: "editor".into(),
            selector: s("[role=edit][name='Text Editor']"),
            text: "\nGoodbye".into(),
        },
        expect: Condition::ElementHasText {
            scope: "editor".into(),
            selector: s("[role=edit][name='Text Editor']"),
            pattern: TextMatch::contains("Goodbye"),
        },
        timeout: Some(Duration::from_secs(5)),
        fallback: None,
        retry: RetryPolicy::None,
        on_failure: OnFailure::Abort,
        on_success: OnSuccess::Continue,
    }];
    executor
        .run(
            &Plan {
                name: "edit_more_text",
                steps: &edit_steps,
                recovery_handlers: vec![],
                max_recoveries: 0,
                unmount: &[],
                default_timeout: DEFAULT_TIMEOUT,
                default_retry: RetryPolicy::None,
            },
            &mut state,
        )
        .expect("editing failed");

    // ── Phase 7: close and dismiss unsaved-changes dialog ────────────────────
    let close_steps = vec![
        Step {
            intent: "click title bar Close button".into(),
            precondition: None,
            action: Action::Click {
                scope: "notepad".into(),
                selector: s("[name~=Notepad] >> [role='title bar'] > [role=button][name=Close]"),
            },
            expect: Condition::DialogPresent {
                scope: "notepad".into(),
            },
            timeout: Some(Duration::from_secs(5)),
            fallback: None,
            retry: RetryPolicy::None,
            on_failure: OnFailure::Abort,
            on_success: OnSuccess::Continue,
        },
        Step {
            intent: "click Don't Save".into(),
            precondition: None,
            action: Action::Click {
                scope: "notepad".into(),
                selector: s(">> [role=button][name^=Don][name$=Save]"),
            },
            expect: Condition::WindowClosed {
                anchor: "notepad".into(),
            },
            timeout: Some(Duration::from_secs(5)),
            fallback: None,
            retry: RetryPolicy::None,
            on_failure: OnFailure::Abort,
            on_success: OnSuccess::Continue,
        },
    ];
    executor
        .run(
            &Plan {
                name: "close_notepad",
                steps: &close_steps,
                recovery_handlers: vec![],
                max_recoveries: 0,
                unmount: &[],
                default_timeout: DEFAULT_TIMEOUT,
                default_retry: RetryPolicy::None,
            },
            &mut state,
        )
        .expect("close/dismiss failed");

    eprintln!("Notepad smoke test completed successfully.");
}
