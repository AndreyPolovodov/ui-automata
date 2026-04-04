// ── Non-Windows stub ──────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
use crate::Result;

#[cfg(not(target_os = "windows"))]
pub struct Overlay;

#[cfg(not(target_os = "windows"))]
impl Overlay {
    pub fn new() -> Result<Self> {
        anyhow::bail!("Windows only")
    }
    pub fn show_at(&self, _x: i32, _y: i32, _w: i32, _h: i32) {}
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub use win::Overlay;

#[cfg(target_os = "windows")]
mod win {
    use std::sync::{Arc, Mutex};
    use std::thread;

    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        CreatePen, CreateSolidBrush, DeleteObject, FillRect, GetDC, HGDIOBJ, PS_SOLID, Rectangle,
        ReleaseDC, SelectObject,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, IDC_ARROW, LWA_COLORKEY, LoadCursorW,
        MSG, MoveWindow, PM_REMOVE, PeekMessageW, PostQuitMessage, RegisterClassExW,
        SW_SHOWNOACTIVATE, SetLayeredWindowAttributes, ShowWindow, WM_DESTROY, WNDCLASSEXW,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
        WS_POPUP,
    };
    use windows::core::PCWSTR;

    use crate::Result;

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    fn draw_border(hwnd: HWND, width: i32, height: i32) {
        const BORDER: i32 = 3;
        const COLOR: u32 = 0x00FF00; // green (BGR)

        unsafe {
            let hdc = GetDC(Some(hwnd));
            if hdc.is_invalid() {
                return;
            }

            let black_brush = CreateSolidBrush(COLORREF(0x000000));
            FillRect(
                hdc,
                &windows::Win32::Foundation::RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                },
                black_brush,
            );
            let _ = DeleteObject(black_brush.into());

            let pen = CreatePen(PS_SOLID, BORDER, COLORREF(COLOR));
            let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
            let fill = CreateSolidBrush(COLORREF(0x000000));
            let old_brush = SelectObject(hdc, HGDIOBJ(fill.0));

            let _ = Rectangle(hdc, 1, 1, width - 1, height - 1);

            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            let _ = DeleteObject(HGDIOBJ(pen.0));
            let _ = DeleteObject(fill.into());

            ReleaseDC(Some(hwnd), hdc);
        }
    }

    /// A transparent, always-on-top overlay window that draws a green border.
    /// Runs its own message loop on a background thread; call `show_at` to reposition.
    pub struct Overlay {
        bounds: Arc<Mutex<Option<(i32, i32, i32, i32)>>>,
        _thread: thread::JoinHandle<()>,
    }

    impl Overlay {
        pub fn new() -> Result<Self> {
            let bounds: Arc<Mutex<Option<(i32, i32, i32, i32)>>> = Arc::new(Mutex::new(None));
            let bounds_clone = bounds.clone();

            let handle = thread::spawn(move || unsafe {
                use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

                const CLASS: PCWSTR = windows::core::w!("UiCoreOverlay");
                let instance = GetModuleHandleW(None).expect("GetModuleHandleW");

                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(wnd_proc),
                    hInstance: instance.into(),
                    hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                    lpszClassName: CLASS,
                    ..Default::default()
                };
                let _ = RegisterClassExW(&wc);

                let hwnd = CreateWindowExW(
                    WS_EX_LAYERED
                        | WS_EX_TRANSPARENT
                        | WS_EX_TOPMOST
                        | WS_EX_TOOLWINDOW
                        | WS_EX_NOACTIVATE,
                    CLASS,
                    windows::core::w!("overlay"),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                )
                .expect("CreateWindowExW");

                SetLayeredWindowAttributes(hwnd, COLORREF(0x000000), 255, LWA_COLORKEY)
                    .expect("SetLayeredWindowAttributes");
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

                let mut msg = MSG::default();
                let mut last: Option<(i32, i32, i32, i32)> = None;

                loop {
                    while PeekMessageW(&mut msg, Some(hwnd), 0, 0, PM_REMOVE).as_bool() {
                        if msg.message == WM_DESTROY {
                            return;
                        }
                        let _ = DispatchMessageW(&msg);
                    }

                    let current = *bounds_clone.lock().unwrap();
                    if current != last {
                        if let Some((x, y, w, h)) = current {
                            if w > 0 && h > 0 {
                                let _ = MoveWindow(hwnd, x, y, w, h, true);
                                draw_border(hwnd, w, h);
                            }
                        }
                        last = current;
                    }

                    thread::sleep(std::time::Duration::from_millis(33));
                }
            });

            Ok(Self {
                bounds,
                _thread: handle,
            })
        }

        /// Move the overlay border to cover `(x, y, w, h)`.
        pub fn show_at(&self, x: i32, y: i32, w: i32, h: i32) {
            *self.bounds.lock().unwrap() = Some((x, y, w, h));
        }
    }
}
