// ── Non-Windows stub ──────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
use crate::Result;

#[cfg(not(target_os = "windows"))]
pub struct MouseHook;

#[cfg(not(target_os = "windows"))]
impl MouseHook {
    pub fn start() -> Result<Self> {
        anyhow::bail!("Windows only")
    }
    pub fn receiver(&self) -> &std::sync::mpsc::Receiver<(i32, i32)> {
        unimplemented!()
    }
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub use win::MouseHook;

#[cfg(target_os = "windows")]
mod win {
    use std::cell::RefCell;
    use std::sync::mpsc;
    use std::thread;

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetCursorPos, GetMessageW, HC_ACTION, HHOOK, MSG,
        SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL, WM_MOUSEMOVE,
    };

    use crate::Result;

    thread_local! {
        static HOOK_SENDER: RefCell<Option<mpsc::SyncSender<(i32, i32)>>> =
            RefCell::new(None);
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 && wparam.0 as u32 == WM_MOUSEMOVE {
            let mut pt = windows::Win32::Foundation::POINT::default();
            if unsafe { GetCursorPos(&mut pt) }.is_ok() {
                HOOK_SENDER.with(|cell| {
                    if let Some(tx) = cell.borrow().as_ref() {
                        let _ = tx.try_send((pt.x, pt.y));
                    }
                });
            }
        }
        unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) }
    }

    /// Installs a low-level mouse hook on a background thread.
    /// Sends cursor `(x, y)` on each mouse move; bounded to 1 so the consumer always
    /// sees the latest position.
    pub struct MouseHook {
        receiver: mpsc::Receiver<(i32, i32)>,
        _thread: thread::JoinHandle<()>,
    }

    impl MouseHook {
        pub fn start() -> Result<Self> {
            let (tx, rx) = mpsc::sync_channel::<(i32, i32)>(1);

            let handle = thread::spawn(move || unsafe {
                HOOK_SENDER.with(|cell| *cell.borrow_mut() = Some(tx));

                let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), None, 0)
                    .expect("SetWindowsHookExW");

                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = DispatchMessageW(&msg);
                }
                UnhookWindowsHookEx(hook).ok();
            });

            Ok(Self {
                receiver: rx,
                _thread: handle,
            })
        }

        /// Iterate with `.receiver().iter()` to receive `(x, y)` on each mouse move.
        pub fn receiver(&self) -> &mpsc::Receiver<(i32, i32)> {
            &self.receiver
        }
    }
}
