/// Type of mouse click to perform.
#[derive(Debug, Clone, Copy)]
pub enum ClickType {
    Left,
    Double,
    Triple,
    Right,
    Middle,
}

/// Direction for a scroll wheel event.
#[derive(Debug, Clone, Copy)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

/// Move the mouse cursor to the given screen coordinates.
/// Returns `false` if the system call failed (e.g. out of bounds).
#[cfg(target_os = "windows")]
pub fn move_cursor(x: i32, y: i32) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
    unsafe { SetCursorPos(x, y) }.is_ok()
}

#[cfg(not(target_os = "windows"))]
pub fn move_cursor(_x: i32, _y: i32) -> bool {
    false
}

/// Return the current screen coordinates of the mouse cursor.
#[cfg(target_os = "windows")]
pub fn get_cursor_pos() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut pt = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut pt) }.ok().map(|_| (pt.x, pt.y))
}

#[cfg(not(target_os = "windows"))]
pub fn get_cursor_pos() -> Option<(i32, i32)> {
    None
}

/// Send a single scroll wheel tick via `SendInput`.
///
/// `clicks` is the number of wheel detents: positive scrolls up/left,
/// negative scrolls down/right. Each detent is one `WHEEL_DELTA` (120 units).
#[cfg(target_os = "windows")]
pub fn scroll_wheel(axis: ScrollAxis, clicks: i32) {
    use std::mem::size_of;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
    };

    let flags = match axis {
        ScrollAxis::Vertical => MOUSEEVENTF_WHEEL,
        ScrollAxis::Horizontal => MOUSEEVENTF_HWHEEL,
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: (clicks * 120) as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
}

#[cfg(not(target_os = "windows"))]
pub fn scroll_wheel(_axis: ScrollAxis, _clicks: i32) {}
