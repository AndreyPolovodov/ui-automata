/// Taskbar inspection and focus-recovery utilities.
///
/// Win10: running app buttons live under a toolbar named "Running applications"
///        inside `Shell_TrayWnd`.
///
/// Win11: the taskbar root is a pane named "Taskbar".  System buttons (Start,
///        Task View, Search, …) live under a child pane with `id=TaskbarFrame`.
///        App buttons (pinned / running) live in a sibling container and carry
///        name suffixes:
///          - "File Explorer pinned"                    → pinned, 0 windows
///          - "File Explorer - 1 running window pinned" → pinned, 1 window
///          - "Notepad - 2 running windows"             → not pinned, 2 windows
///
/// The multi-window thumbnail picker on Win11 is activated by the mouse click,
/// then navigated with Tab×2 (focus first thumbnail), Right×N, Enter.

#[cfg(target_os = "windows")]
pub use windows_impl::*;

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::thread;
    use std::time::{Duration, Instant};

    use uiautomation::controls::ControlType;
    use uiautomation::inputs::Mouse;
    use uiautomation::patterns::UIInvokePattern;
    use uiautomation::types::{Point, TreeScope, UIProperty};
    use uiautomation::variants::Variant;
    use uiautomation::{UIAutomation, UIElement};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetForegroundWindow, GetWindowThreadProcessId,
    };

    use crate::process::get_process_name;
    use crate::util::{find_named_timeout, invoke_or_click};

    /// A button in the taskbar app area.
    #[derive(Debug, Clone)]
    pub struct TaskbarButton {
        /// Raw UIA button name (e.g. `"Notepad - 1 running window"`).
        pub name: String,
        /// Display name with running/pinned suffixes stripped.
        pub display_name: String,
        /// Number of open windows (0 = pinned shortcut with no open windows).
        pub window_count: u32,
        /// PID of the process that owns this button element.
        pub pid: u32,
        /// Button centre — used as fallback for mouse click.
        pub cx: i32,
        pub cy: i32,
        /// True when discovered via the Win11 TaskbarFrame path.
        /// Controls which thumbnail picker strategy is used on click.
        pub win11: bool,
        /// The underlying UIA element — used for `Invoke`.
        pub element: UIElement,
    }

    /// Enumerate all app buttons in the taskbar (pinned + running).
    ///
    /// Tries the Win10 "Running applications" toolbar first; falls back to the
    /// Win11 `name=Taskbar` pane approach if that toolbar is not found.
    ///
    /// Returns an empty vec if the taskbar is not found (e.g. headless machine).
    pub fn list_taskbar_buttons() -> Vec<TaskbarButton> {
        let Ok(auto) = UIAutomation::new_direct() else {
            return vec![];
        };
        let Ok(root) = auto.get_root_element() else {
            return vec![];
        };

        // Win11 also has a "Running applications" pane but it is always empty —
        // only use the Win10 result if it actually contains buttons.
        if let Some(buttons) = collect_buttons_win10(&auto, &root) {
            if !buttons.is_empty() {
                return buttons;
            }
        }
        collect_buttons_win11(&auto, &root).unwrap_or_default()
    }

    // ── Win10 ──────────────────────────────────────────────────────────────────

    /// Find the "Running applications" toolbar and return its direct button children.
    fn collect_buttons_win10(auto: &UIAutomation, root: &UIElement) -> Option<Vec<TaskbarButton>> {
        let name_cond = auto
            .create_property_condition(
                UIProperty::Name,
                Variant::from("Running applications"),
                None,
            )
            .ok()?;
        let toolbar = root.find_first(TreeScope::Descendants, &name_cond).ok()?;
        let btn_cond = button_or_split_or_menu(auto)?;
        let buttons = toolbar.find_all(TreeScope::Children, &btn_cond).ok()?;
        Some(to_taskbar_buttons(buttons, false))
    }

    // ── Win11 ──────────────────────────────────────────────────────────────────

    /// Win11 structure:
    ///   desktop → pane "Taskbar" → pane "" → pane id=TaskbarFrame → buttons
    ///
    /// Find `id=TaskbarFrame`, take its direct Button children, and exclude
    /// known system buttons (Start, Task View, Search, …).
    fn collect_buttons_win11(auto: &UIAutomation, root: &UIElement) -> Option<Vec<TaskbarButton>> {
        let frame_cond = auto
            .create_property_condition(
                UIProperty::AutomationId,
                Variant::from("TaskbarFrame"),
                None,
            )
            .ok()?;
        let frame = root.find_first(TreeScope::Descendants, &frame_cond).ok()?;

        let btn_cond = button_or_split_or_menu(auto)?;
        let children = frame.find_all(TreeScope::Children, &btn_cond).ok()?;

        let system_names: &[&str] = &["start", "search", "task view", "chat", "copilot"];

        let app_buttons: Vec<UIElement> = children
            .into_iter()
            .filter(|el| {
                let name = el.get_name().unwrap_or_default().to_lowercase();
                !name.is_empty()
                    && !system_names.iter().any(|s| name == *s)
                    && !name.starts_with("widgets")
            })
            .collect();

        Some(to_taskbar_buttons(app_buttons, true))
    }

    // ── Shared helpers ─────────────────────────────────────────────────────────

    fn to_taskbar_buttons(elements: Vec<UIElement>, win11: bool) -> Vec<TaskbarButton> {
        elements
            .iter()
            .filter_map(|btn| {
                let name = btn.get_name().unwrap_or_default();
                if name.is_empty() {
                    return None;
                }
                let rect = btn.get_bounding_rectangle().ok()?;
                // Elements with degenerate bounds are off-screen / invisible — skip.
                if rect.get_width() == 0 && rect.get_height() == 0 {
                    return None;
                }
                let cx = rect.get_left() + rect.get_width() / 2;
                let cy = rect.get_top() + rect.get_height() / 2;
                let display_name = strip_app_suffixes(&name);
                let window_count = parse_window_count(&name);
                let pid = btn.get_process_id().unwrap_or(0);
                Some(TaskbarButton {
                    name,
                    display_name,
                    window_count,
                    pid,
                    cx,
                    cy,
                    win11,
                    element: btn.clone(),
                })
            })
            .collect()
    }

    fn button_or_split_or_menu(auto: &UIAutomation) -> Option<uiautomation::core::UICondition> {
        let c_btn = auto
            .create_property_condition(
                UIProperty::ControlType,
                Variant::from(ControlType::Button as i32),
                None,
            )
            .ok()?;
        let c_split = auto
            .create_property_condition(
                UIProperty::ControlType,
                Variant::from(ControlType::SplitButton as i32),
                None,
            )
            .ok()?;
        let c_menu = auto
            .create_property_condition(
                UIProperty::ControlType,
                Variant::from(ControlType::MenuItem as i32),
                None,
            )
            .ok()?;
        let or1 = auto.create_or_condition(c_btn, c_split).ok()?;
        auto.create_or_condition(or1, c_menu).ok()
    }

    // ── Focus ──────────────────────────────────────────────────────────────────

    /// Click the taskbar button by display name and confirm the target process
    /// is in the foreground afterwards.
    pub fn focus_app_by_name(
        button: &str,
        process_name: &str,
        window_index: usize,
    ) -> Result<(), String> {
        let button_lower = button.to_lowercase();

        let buttons = list_taskbar_buttons();
        if buttons.is_empty() {
            return Err("taskbar not found or has no running app buttons".into());
        }

        let btn = buttons
            .iter()
            .find(|b| b.display_name.to_lowercase() == button_lower)
            .ok_or_else(|| {
                format!(
                    "no taskbar button named '{button}' \
                     (buttons visible: {})",
                    buttons
                        .iter()
                        .map(|b| b.display_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;

        // Mouse-click opens the thumbnail picker for multi-window apps.
        Mouse::new()
            .click(Point::new(btn.cx, btn.cy))
            .map_err(|e| format!("taskbar click failed: {e}"))?;

        // If a thumbnail picker appears, activate the item at window_index.
        // Use the picker strategy that matches where the button was discovered.
        let picker_timeout = Duration::from_secs(2);
        if btn.win11 {
            pick_win11_thumbnail(window_index, picker_timeout);
        } else {
            pick_win10_task_switcher(window_index, picker_timeout);
        }

        // Confirm the target process is now foreground.
        let process_lower = process_name.to_lowercase();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let fg_pid = unsafe {
                let fg = GetForegroundWindow();
                let mut pid = 0u32;
                GetWindowThreadProcessId(fg, Some(&mut pid));
                pid
            };
            let fg_proc = get_process_name(fg_pid as i32)
                .unwrap_or_default()
                .to_lowercase();
            if fg_proc == process_lower {
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }

        Err(format!(
            "focus not confirmed: process '{process_name}' is not in the foreground"
        ))
    }

    fn pick_win10_task_switcher(index: usize, timeout: Duration) -> bool {
        let Ok(auto) = UIAutomation::new_direct() else {
            return false;
        };
        let Ok(root) = auto.get_root_element() else {
            return false;
        };
        let Some(switcher) = find_named_timeout(&auto, &root, "Task Switcher", timeout) else {
            return false;
        };
        let Ok(item_cond) = auto.create_property_condition(
            UIProperty::ControlType,
            Variant::from(ControlType::ListItem as i32),
            None,
        ) else {
            return false;
        };
        let Ok(items) = switcher.find_all(TreeScope::Descendants, &item_cond) else {
            return false;
        };
        let target = items.get(index).or_else(|| items.iter().next());
        if let Some(el) = target {
            let _ = invoke_or_click(el);
            true
        } else {
            false
        }
    }

    /// Win11: the thumbnail picker is already open from the mouse click.
    /// Tab×2 focuses the first thumbnail, Right×index navigates to the target,
    /// Enter activates it.
    fn pick_win11_thumbnail(index: usize, timeout: Duration) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
            VK_RETURN, VK_RIGHT, VK_TAB,
        };

        // Wait for the thumbnail popup (Xaml_WindowedPopupClass / "PopupHost") to appear.
        let class_wide: Vec<u16> = "Xaml_WindowedPopupClass\0".encode_utf16().collect();
        let title_wide: Vec<u16> = "PopupHost\0".encode_utf16().collect();
        let deadline = Instant::now() + timeout;
        loop {
            let found = unsafe {
                FindWindowExW(
                    None,
                    None,
                    windows::core::PCWSTR(class_wide.as_ptr()),
                    windows::core::PCWSTR(title_wide.as_ptr()),
                )
            };
            if found.is_ok_and(|h| !h.is_invalid()) {
                break;
            }
            if Instant::now() >= deadline {
                return; // picker never appeared — single-window app, click already focused it
            }
            thread::sleep(Duration::from_millis(50));
        }

        fn key(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY, up: bool) -> INPUT {
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: 0,
                        dwFlags: if up {
                            KEYEVENTF_KEYUP
                        } else {
                            KEYBD_EVENT_FLAGS(0)
                        },
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            }
        }

        let mut inputs: Vec<INPUT> = Vec::new();
        // Tab×2 — focus the first thumbnail.
        for _ in 0..2 {
            inputs.push(key(VK_TAB, false));
            inputs.push(key(VK_TAB, true));
        }
        // Right×index — move to the target thumbnail.
        for _ in 0..index {
            inputs.push(key(VK_RIGHT, false));
            inputs.push(key(VK_RIGHT, true));
        }
        // Enter — activate.
        inputs.push(key(VK_RETURN, false));
        inputs.push(key(VK_RETURN, true));

        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    // ── Show Desktop ───────────────────────────────────────────────────────────

    pub fn show_desktop() -> Result<(), String> {
        let Ok(auto) = UIAutomation::new_direct() else {
            return Err("UIAutomation init failed".into());
        };
        let Ok(root) = auto.get_root_element() else {
            return Err("could not get desktop root".into());
        };
        let Ok(name_cond) =
            auto.create_property_condition(UIProperty::Name, Variant::from("Show desktop"), None)
        else {
            return Err("could not create name condition".into());
        };
        let btn = root
            .find_first(TreeScope::Descendants, &name_cond)
            .map_err(|_| "Show desktop button not found".to_string())?;
        btn.get_pattern::<UIInvokePattern>()
            .and_then(|p| p.invoke())
            .map_err(|e| format!("Show desktop invoke failed: {e}"))
    }

    // ── Name parsing ───────────────────────────────────────────────────────────

    /// Strip Win10/Win11 suffixes and return the plain app display name.
    ///
    /// Win11 examples:
    ///   "File Explorer pinned"                    → "File Explorer"
    ///   "File Explorer - 1 running window pinned" → "File Explorer"
    ///   "Notepad - 2 running windows"             → "Notepad"
    ///
    /// Win10 example:
    ///   "Notepad - 1 running window"              → "Notepad"
    fn strip_app_suffixes(name: &str) -> String {
        // Strip " pinned" suffix (Win11 only).
        let name = name.strip_suffix(" pinned").unwrap_or(name);
        // Strip " - N running window[s]" suffix.
        if let Some(pos) = name.rfind(" - ") {
            let suffix = &name[pos + 3..];
            if suffix.ends_with("running window") || suffix.ends_with("running windows") {
                return name[..pos].to_string();
            }
        }
        name.to_string()
    }

    /// Parse the open-window count from a raw button name.
    ///
    ///   "File Explorer pinned"                    → 0
    ///   "File Explorer - 1 running window pinned" → 1
    ///   "Notepad - 2 running windows"             → 2
    ///   "Terminal" (no suffix)                    → 1
    pub fn parse_window_count(name: &str) -> u32 {
        let was_pinned_only = name.ends_with(" pinned") && !name.contains(" running window");
        // Strip " pinned" suffix before parsing the running-window count.
        let name = name.strip_suffix(" pinned").unwrap_or(name);
        if let Some(pos) = name.rfind(" - ") {
            let suffix = &name[pos + 3..];
            if suffix.ends_with("running window") || suffix.ends_with("running windows") {
                if let Some(n_str) = suffix.splitn(2, ' ').next() {
                    if let Ok(n) = n_str.parse::<u32>() {
                        return n;
                    }
                }
            }
        }
        // "File Explorer pinned" (no running-window clause) → 0 open windows.
        if was_pinned_only { 0 } else { 1 }
    }
}

// ── Non-Windows stubs ──────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
#[derive(Debug, Clone)]
pub struct TaskbarButton {
    pub name: String,
    pub display_name: String,
    pub window_count: u32,
    pub pid: u32,
    pub cx: i32,
    pub cy: i32,
    pub win11: bool,
}

#[cfg(not(target_os = "windows"))]
pub fn list_taskbar_buttons() -> Vec<TaskbarButton> {
    vec![]
}

#[cfg(not(target_os = "windows"))]
pub fn parse_window_count(_name: &str) -> u32 {
    0
}

#[cfg(not(target_os = "windows"))]
pub fn focus_app_by_name(
    _button: &str,
    _process_name: &str,
    _window_index: usize,
) -> Result<(), String> {
    Err("ListTaskbar / FocusApp is only supported on Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn show_desktop() -> Result<(), String> {
    Err("ShowDesktop is only supported on Windows".into())
}
