/// Raw mouse and keyboard input — no UIA element required.

#[cfg(target_os = "windows")]
pub use windows_impl::*;

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::mem::size_of;

    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
        MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
    };
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;

    use crate::mouse::ClickType;

    fn send_mouse_events(events: &[MOUSEINPUT]) {
        let inputs: Vec<INPUT> = events
            .iter()
            .map(|mi| INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 { mi: *mi },
            })
            .collect();
        unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    }

    fn mi(flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) -> MOUSEINPUT {
        MOUSEINPUT {
            dx: 0,
            dy: 0,
            mouseData: 0,
            dwFlags: flags,
            time: 0,
            dwExtraInfo: 0,
        }
    }

    /// Move the cursor to screen coordinates `(x, y)`.
    pub fn mouse_move(x: i32, y: i32) -> Result<(), String> {
        unsafe { SetCursorPos(x, y) }.map_err(|e| format!("SetCursorPos failed: {e}"))
    }

    /// Press the left button at `(x1, y1)`, move to `(x2, y2)`, then release.
    pub fn mouse_drag(x1: i32, y1: i32, x2: i32, y2: i32) -> Result<(), String> {
        mouse_move(x1, y1)?;
        send_mouse_events(&[mi(MOUSEEVENTF_LEFTDOWN)]);
        std::thread::sleep(std::time::Duration::from_millis(500));
        mouse_move(x2, y2)?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        send_mouse_events(&[mi(MOUSEEVENTF_LEFTUP)]);
        Ok(())
    }

    /// Move the cursor to `(x, y)` and send a click.
    pub fn mouse_click(x: i32, y: i32, button: ClickType) -> Result<(), String> {
        mouse_move(x, y)?;
        match button {
            ClickType::Left => {
                send_mouse_events(&[mi(MOUSEEVENTF_LEFTDOWN), mi(MOUSEEVENTF_LEFTUP)]);
            }
            ClickType::Right => {
                send_mouse_events(&[mi(MOUSEEVENTF_RIGHTDOWN), mi(MOUSEEVENTF_RIGHTUP)]);
            }
            ClickType::Double => {
                send_mouse_events(&[
                    mi(MOUSEEVENTF_LEFTDOWN),
                    mi(MOUSEEVENTF_LEFTUP),
                    mi(MOUSEEVENTF_LEFTDOWN),
                    mi(MOUSEEVENTF_LEFTUP),
                ]);
            }
            ClickType::Triple => {
                send_mouse_events(&[
                    mi(MOUSEEVENTF_LEFTDOWN),
                    mi(MOUSEEVENTF_LEFTUP),
                    mi(MOUSEEVENTF_LEFTDOWN),
                    mi(MOUSEEVENTF_LEFTUP),
                    mi(MOUSEEVENTF_LEFTDOWN),
                    mi(MOUSEEVENTF_LEFTUP),
                ]);
            }
            ClickType::Middle => {
                send_mouse_events(&[mi(MOUSEEVENTF_MIDDLEDOWN), mi(MOUSEEVENTF_MIDDLEUP)]);
            }
        }
        Ok(())
    }

    /// Move the mouse to `(x, y)` then scroll the wheel.
    /// `delta_x` scrolls horizontally (positive = right), `delta_y` vertically (positive = up).
    /// Each unit is one wheel detent (120 Windows scroll units).
    pub fn mouse_scroll(x: i32, y: i32, delta_x: i32, delta_y: i32) -> Result<(), String> {
        mouse_move(x, y)?;
        if delta_y != 0 {
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: (delta_y * 120) as u32,
                        dwFlags: MOUSEEVENTF_WHEEL,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        }
        if delta_x != 0 {
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: (delta_x * 120) as u32,
                        dwFlags: MOUSEEVENTF_HWHEEL,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        }
        Ok(())
    }

    /// Send key strokes using the uiautomation `send_keys` syntax.
    /// Modifiers: `{ctrl}v`, `{alt}(F4)`, `{shift}(AB)` (`()` groups keys held with the modifier).
    /// Special keys: `{enter}`, `{tab}`, `{esc}`, `{home}`, `{end}`, `{delete}`, `{backspace}`, `{up}`, `{down}`.
    /// Literal braces/parens: `{{` `}}` `{(` `{)`.
    pub fn key_press(keys: &str) -> Result<(), String> {
        uiautomation::inputs::Keyboard::new()
            .send_keys(&normalise_key(keys))
            .map_err(|e| format!("key_press failed: {e}"))
    }

    /// Convert `ctrl+shift+a` style combos to uiautomation `{ctrl}{shift}a` syntax.
    /// Passes through strings that already contain `{` unchanged.
    pub(crate) fn normalise_key(keys: &str) -> std::borrow::Cow<'_, str> {
        if keys.contains('{') {
            return std::borrow::Cow::Borrowed(keys);
        }
        const MODIFIERS: &[&str] = &["ctrl", "alt", "shift", "win"];
        let parts: Vec<&str> = keys.split('+').collect();
        let (mods, rest) = parts.split_at(
            parts
                .iter()
                .take_while(|p| MODIFIERS.contains(&p.to_lowercase().as_str()))
                .count(),
        );
        if mods.is_empty() {
            return std::borrow::Cow::Borrowed(keys);
        }
        let mut out = String::new();
        for m in mods {
            out.push('{');
            out.push_str(&m.to_lowercase());
            out.push('}');
        }
        out.push_str(&rest.join("+"));
        std::borrow::Cow::Owned(out)
    }

    /// Type Unicode text at the current keyboard focus, character by character.
    pub fn type_text(text: &str) -> Result<(), String> {
        uiautomation::inputs::Keyboard::new()
            .send_text(text)
            .map_err(|e| format!("type_text failed: {e}"))
    }
}

// ── Non-Windows stubs ──────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
pub fn mouse_move(_x: i32, _y: i32) -> Result<(), String> {
    Err("mouse_move is only supported on Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn mouse_drag(_x1: i32, _y1: i32, _x2: i32, _y2: i32) -> Result<(), String> {
    Err("mouse_drag is only supported on Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn mouse_click(_x: i32, _y: i32, _button: crate::ClickType) -> Result<(), String> {
    Err("mouse_click is only supported on Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn mouse_scroll(_x: i32, _y: i32, _delta_x: i32, _delta_y: i32) -> Result<(), String> {
    Err("mouse_scroll is only supported on Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn key_press(_keys: &str) -> Result<(), String> {
    Err("key_press is only supported on Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn type_text(_text: &str) -> Result<(), String> {
    Err("type_text is only supported on Windows".into())
}
