use uiautomation::UIAutomation;
use uiautomation::types::TreeScope;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::IsIconic;

use crate::Result;
use crate::process::get_process_name;
use crate::util::window_pane_condition;

/// Basic info about a top-level window.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub pid: u32,
    pub process_name: String,
    pub title: String,
    pub control_type: String,
    pub class: String,
    pub automation_id: String,
    pub hwnd: u64,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub is_minimized: bool,
}

/// Return all top-level Window/Pane elements visible to UIA, with their process info.
pub fn application_windows() -> Result<Vec<WindowInfo>> {
    let automation = UIAutomation::new_direct()?;
    let root = automation.get_root_element()?;
    let condition = window_pane_condition(&automation)?;
    let elements = root.find_all(TreeScope::Children, &condition)?;

    let mut windows = Vec::new();
    for element in &elements {
        let pid = match element.get_process_id() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let process_name =
            get_process_name(pid as i32).unwrap_or_else(|_| format!("unknown-{pid}"));

        let title = element.get_name().unwrap_or_default();
        let control_type = element
            .get_localized_control_type()
            .unwrap_or_else(|_| format!("{:?}", element.get_control_type().ok()));
        let automation_id = element.get_automation_id().unwrap_or_default();
        let class = element.get_classname().unwrap_or_default();
        let hwnd = element
            .get_native_window_handle()
            .map(|h| {
                let v: isize = h.into();
                v as u64
            })
            .unwrap_or(0);
        let (x, y, width, height) = element
            .get_bounding_rectangle()
            .map(|r| (r.get_left(), r.get_top(), r.get_width(), r.get_height()))
            .unwrap_or((0, 0, 0, 0));
        let is_minimized = hwnd != 0
            && unsafe { IsIconic(HWND(hwnd as isize as *mut core::ffi::c_void)).as_bool() };

        windows.push(WindowInfo {
            pid,
            process_name,
            title,
            control_type,
            class,
            automation_id,
            hwnd,
            x,
            y,
            width,
            height,
            is_minimized,
        });
    }

    Ok(windows)
}

/// Return windows belonging to the given process name (case-insensitive, without .exe).
pub fn find_windows(process_name: &str) -> Result<Vec<WindowInfo>> {
    let all = application_windows()?;
    Ok(all
        .into_iter()
        .filter(|w| w.process_name.eq_ignore_ascii_case(process_name))
        .collect())
}
