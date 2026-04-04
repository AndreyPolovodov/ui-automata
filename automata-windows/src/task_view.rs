/// Activate Task View (Win+Tab equivalent) via the taskbar.
///
/// If the Task View button is already visible on the taskbar, clicks it directly.
/// If not, right-clicks "Notification Chevron" to open the taskbar context menu,
/// enables the button via "Show Task View button", then clicks it.

#[cfg(target_os = "windows")]
pub use windows_impl::*;

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::thread;
    use std::time::{Duration, Instant};

    use uiautomation::UIAutomation;
    use uiautomation::controls::ControlType;
    use uiautomation::types::{TreeScope, UIProperty};
    use uiautomation::variants::Variant;

    use crate::util::invoke_or_click;

    /// Return the titles of all windows currently shown in the Task View switcher.
    ///
    /// Opens Task View first if it is not already open (clicking it twice would close it).
    pub fn list_task_view_windows() -> Result<Vec<String>, String> {
        let Ok(auto) = UIAutomation::new_direct() else {
            return Err("UIAutomation init failed".into());
        };
        let Ok(root) = auto.get_root_element() else {
            return Err("could not get desktop root".into());
        };

        let id_cond = auto
            .create_property_condition(
                UIProperty::AutomationId,
                Variant::from("SwitchItemListControl"),
                None,
            )
            .map_err(|e| format!("could not create condition: {e}"))?;

        // Only activate Task View if it isn't already open.
        let list = if let Ok(el) = root.find_first(TreeScope::Descendants, &id_cond) {
            el
        } else {
            activate_task_view()?;

            // Rebuild the condition after activation (conditions are consumed).
            let id_cond2 = auto
                .create_property_condition(
                    UIProperty::AutomationId,
                    Variant::from("SwitchItemListControl"),
                    None,
                )
                .map_err(|e| format!("could not create condition: {e}"))?;

            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok(el) = root.find_first(TreeScope::Descendants, &id_cond2) {
                    break el;
                }
                if Instant::now() >= deadline {
                    return Err("timed out waiting for Task View to open".into());
                }
                thread::sleep(Duration::from_millis(50));
            }
        };

        let item_cond = auto
            .create_property_condition(
                UIProperty::ControlType,
                Variant::from(ControlType::ListItem as i32),
                None,
            )
            .map_err(|e| format!("could not create list item condition: {e}"))?;

        let items = list
            .find_all(TreeScope::Children, &item_cond)
            .map_err(|e| format!("could not enumerate Task View items: {e}"))?;

        Ok(items
            .iter()
            .filter_map(|item| item.get_name().ok().filter(|s| !s.is_empty()))
            .collect())
    }

    /// Click the Task View thumbnail matching `title` (case-insensitive substring).
    ///
    /// Returns an error if Task View is not already open.
    pub fn activate_task_view_window(title: &str) -> Result<(), String> {
        let Ok(auto) = UIAutomation::new_direct() else {
            return Err("UIAutomation init failed".into());
        };
        let Ok(root) = auto.get_root_element() else {
            return Err("could not get desktop root".into());
        };

        let id_cond = auto
            .create_property_condition(
                UIProperty::AutomationId,
                Variant::from("SwitchItemListControl"),
                None,
            )
            .map_err(|e| format!("could not create condition: {e}"))?;

        let list = root
            .find_first(TreeScope::Descendants, &id_cond)
            .map_err(|_| "Task View is not open".to_string())?;

        let item_cond = auto
            .create_property_condition(
                UIProperty::ControlType,
                Variant::from(ControlType::ListItem as i32),
                None,
            )
            .map_err(|e| format!("could not create list item condition: {e}"))?;

        let items = list
            .find_all(TreeScope::Children, &item_cond)
            .map_err(|e| format!("could not enumerate Task View items: {e}"))?;

        let title_lower = title.to_lowercase();
        let target = items
            .iter()
            .find(|item| {
                item.get_name()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&title_lower)
            })
            .ok_or_else(|| format!("no Task View window matching '{title}'"))?;

        invoke_or_click(target)
    }

    fn activate_task_view() -> Result<(), String> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
            VK_LWIN, VK_TAB,
        };

        let inputs = [
            // Win down
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_LWIN,
                        wScan: 0,
                        dwFlags: KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            // Tab down
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_TAB,
                        wScan: 0,
                        dwFlags: KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            // Tab up
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_TAB,
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            // Win up
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_LWIN,
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent != inputs.len() as u32 {
            return Err(format!("SendInput sent {sent}/{} events", inputs.len()));
        }
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn list_task_view_windows() -> Result<Vec<String>, String> {
    Err("list_task_view_windows is only supported on Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn activate_task_view_window(_title: &str) -> Result<(), String> {
    Err("activate_task_view_window is only supported on Windows".into())
}
