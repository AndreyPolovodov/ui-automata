#[cfg(target_os = "windows")]
pub use windows_impl::*;

#[cfg(target_os = "windows")]
mod windows_impl {
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsIconic, SW_RESTORE, SetForegroundWindow,
        ShowWindow, WindowFromPoint,
    };

    /// Bring `hwnd` to the foreground. Only restores (un-minimizes) the window if it
    /// is actually minimized — skipping SW_RESTORE on a snapped/normal window preserves
    /// its current size and position (SW_RESTORE would un-snap a Windows-Snap-tiled window).
    pub fn force_foreground(hwnd: HWND) -> Result<(), String> {
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            if !SetForegroundWindow(hwnd).as_bool() {
                return Err("SetForegroundWindow failed".into());
            }
        }
        Ok(())
    }

    /// Verify that the topmost window at screen position `(x, y)` belongs to `expected_pid`.
    ///
    /// Uses `WindowFromPoint` (Win32 Z-order, not UIA accessibility tree) so the check
    /// reflects exactly which window the mouse click will actually land on.
    pub fn check_click_point(x: i32, y: i32, expected_pid: u32) -> Result<(), String> {
        unsafe {
            let hwnd = WindowFromPoint(POINT { x, y });
            if hwnd.is_invalid() {
                return Err(format!("click point ({x},{y}): no window found"));
            }
            let mut actual = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut actual));
            if actual != expected_pid {
                return Err(format!(
                    "click point ({x},{y}) is over process {actual}, expected {expected_pid} — wrong window in foreground"
                ));
            }
        }
        Ok(())
    }

    /// Verify that the foreground window belongs to `expected_pid`.
    ///
    /// Called before every keyboard action to guard against typing into a window that
    /// stole focus while the workflow was running.
    ///
    /// UWP apps are hosted by `ApplicationFrameHost.exe`: the foreground HWND belongs
    /// to that process while the UIA element's PID is the content process. We allow
    /// this combination so keyboard actions work against hosted UWP apps (e.g. the
    /// Microsoft Store, Settings).
    pub fn check_keyboard_target(expected_pid: u32) -> Result<(), String> {
        unsafe {
            let fg = GetForegroundWindow();
            let mut actual = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut actual));
            if actual == expected_pid {
                return Ok(());
            }
            // Allow ApplicationFrameHost as the foreground process — it hosts UWP content
            // whose UIA elements report a different PID (the content process).
            if is_application_frame_host(actual) {
                return Ok(());
            }
            Err(format!(
                "foreground window is process {actual}, expected {expected_pid} — refusing keyboard input"
            ))
        }
    }

    fn is_application_frame_host(pid: u32) -> bool {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        };

        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok();
            let _ = CloseHandle(handle);
            if !ok {
                return false;
            }
            let name = String::from_utf16_lossy(&buf[..len as usize]);
            name.to_ascii_lowercase()
                .ends_with("applicationframehost.exe")
        }
    }

    /// Bring the window matching `process`/`selector`/`pid` to the foreground.
    ///
    /// Strategy:
    /// 1. Enumerate top-level windows; pick the first that matches `process` (and `pid`/`name` if given).
    /// 2. Restore if minimized; call `SetForegroundWindow`.
    /// 3. If not yet foreground, probe point `(window.x + 10, window.y + 10)` with `WindowFromPoint`:
    ///    if the topmost window there belongs to the target process, click that point to steal focus.
    /// 4. Verify foreground; return `Err` if still not active after all attempts.
    pub fn activate_window_preflight(
        process: &str,
        selector: Option<&str>,
        pid: Option<u32>,
    ) -> Result<(), String> {
        use crate::windows_info::application_windows;

        crate::init_com();
        let all = application_windows().map_err(|e| format!("activate_window: {e}"))?;

        // Filter by process name (without .exe, case-insensitive).
        let proc_lower = process.to_ascii_lowercase();
        let candidates: Vec<_> = all
            .into_iter()
            .filter(|w| {
                let name = w.process_name.to_ascii_lowercase();
                // match "notepad" against "notepad.exe" or "notepad"
                name == proc_lower || name.strip_suffix(".exe").unwrap_or(&name) == proc_lower
            })
            .filter(|w| pid.map_or(true, |p| w.pid == p))
            .filter(|w| {
                // Optional name filter from selector — handle the common `[name~=…]` pattern.
                if let Some(sel) = selector {
                    if sel == "*" {
                        return true;
                    }
                    // Extract value from `[name~=VALUE]`, `[name=VALUE]`, `[title~=VALUE]`, etc.
                    let rest_opt = sel
                        .strip_prefix("[name")
                        .or_else(|| sel.strip_prefix("[title"));
                    if let Some(rest) = rest_opt {
                        let value = rest
                            .trim_start_matches(|c| c == '~' || c == '^' || c == '$' || c == '=')
                            .trim_end_matches(']');
                        let title_lower = w.title.to_ascii_lowercase();
                        let val_lower = value.to_ascii_lowercase();
                        return title_lower.contains(&val_lower);
                    }
                }
                true
            })
            .collect();

        let win = candidates
            .first()
            .ok_or_else(|| format!("activate_window: no window found for process '{process}'"))?;

        let hwnd = HWND(win.hwnd as isize as *mut core::ffi::c_void);
        let target_pid = win.pid;
        let win_x = win.x;
        let win_y = win.y;

        // Step 1: restore if minimized, then try SetForegroundWindow.
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        let sfg_ok = unsafe { SetForegroundWindow(hwnd).as_bool() };

        // Check if we are now foreground.
        let is_foreground = || -> bool {
            let fg = unsafe { GetForegroundWindow() };
            let mut fg_pid = 0u32;
            unsafe { GetWindowThreadProcessId(fg, Some(&mut fg_pid)) };
            fg_pid == target_pid
        };

        if sfg_ok && is_foreground() {
            return Ok(());
        }

        // Step 2: probe point (x+10, y+10) from window top-left.
        // If our window is the topmost at that point, click there to steal focus.
        let probe_x = win_x + 10;
        let probe_y = win_y + 10;
        unsafe {
            let top_hwnd = WindowFromPoint(POINT {
                x: probe_x,
                y: probe_y,
            });
            if !top_hwnd.is_invalid() {
                let mut top_pid = 0u32;
                GetWindowThreadProcessId(top_hwnd, Some(&mut top_pid));
                if top_pid == target_pid {
                    // Our window is topmost but unfocused — click to activate.
                    crate::input::mouse_click(probe_x, probe_y, crate::mouse::ClickType::Left).ok();
                    std::thread::sleep(std::time::Duration::from_millis(150));
                } else {
                    // A foreign window is blocking our target — try clicking through the title bar.
                    // Pick the horizontal centre of the title bar (y+10 from top-left, x = centre).
                    let title_x = win_x + (win.width / 2).max(10);
                    let title_y = win_y + 10;
                    crate::input::mouse_click(title_x, title_y, crate::mouse::ClickType::Left).ok();
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
            }
        }

        if is_foreground() {
            return Ok(());
        }

        Err(format!(
            "activate_window: window for process '{process}' is not foreground after all attempts"
        ))
    }

    /// Parse a `"0x…"` hwnd string and return an `HWND`.
    fn parse_hwnd(hwnd_str: &str) -> Result<HWND, String> {
        let hex = hwnd_str
            .strip_prefix("0x")
            .or_else(|| hwnd_str.strip_prefix("0X"))
            .ok_or_else(|| format!("hwnd must be 0x-prefixed hex, got: {hwnd_str:?}"))?;
        let val =
            u64::from_str_radix(hex, 16).map_err(|_| format!("invalid hwnd: {hwnd_str:?}"))?;
        Ok(HWND(val as isize as *mut core::ffi::c_void))
    }

    /// Minimise, maximise, or restore a window by its `hwnd` string.
    pub fn set_window_state(hwnd_str: &str, state: crate::WindowAction) -> Result<(), String> {
        use crate::WindowAction;
        use windows::Win32::UI::WindowsAndMessaging::{
            SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, ShowWindow,
        };

        let hwnd = parse_hwnd(hwnd_str)?;

        if state == WindowAction::Close {
            use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
            unsafe {
                PostMessageW(
                    Some(hwnd),
                    WM_CLOSE,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                )
                .map_err(|e| format!("PostMessageW(WM_CLOSE) failed: {e}"))?;
            }
            return Ok(());
        }

        let cmd = match state {
            WindowAction::Minimize => SW_MINIMIZE,
            WindowAction::Maximize => SW_MAXIMIZE,
            WindowAction::Restore => SW_RESTORE,
            WindowAction::Close => unreachable!(),
        };
        unsafe {
            let _ = ShowWindow(hwnd, cmd);
        };
        Ok(())
    }

    /// Move and resize a window by its `hwnd` string.
    pub fn set_window_bounds(
        hwnd_str: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        use windows::Win32::UI::WindowsAndMessaging::{HWND_TOP, SWP_NOZORDER, SetWindowPos};

        let hwnd = parse_hwnd(hwnd_str)?;
        unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                x,
                y,
                width as i32,
                height as i32,
                SWP_NOZORDER,
            )
            .map_err(|e| format!("SetWindowPos failed: {e}"))?;
        }
        Ok(())
    }

    /// Return the PID of the foreground window, or `None` if there is no foreground window.
    pub fn foreground_pid() -> Option<u32> {
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_invalid() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            if pid == 0 { None } else { Some(pid) }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn foreground_pid() -> Option<u32> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn activate_window_preflight(
    _process: &str,
    _selector: Option<&str>,
    _pid: Option<u32>,
) -> Result<(), String> {
    Err("activate_window_preflight is only supported on Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn set_window_state(_hwnd: &str, _state: crate::WindowAction) -> Result<(), String> {
    Err("set_window_state is only supported on Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn set_window_bounds(
    _hwnd: &str,
    _x: i32,
    _y: i32,
    _width: u32,
    _height: u32,
) -> Result<(), String> {
    Err("set_window_bounds is only supported on Windows".into())
}
