/// Clipboard read/write using Win32 APIs.

#[cfg(target_os = "windows")]
pub use windows_impl::*;

#[cfg(target_os = "windows")]
mod windows_impl {
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::CF_UNICODETEXT;

    /// Read Unicode text from the clipboard. Returns an empty string if the
    /// clipboard contains no text data.
    pub fn clipboard_read() -> Result<String, String> {
        unsafe {
            OpenClipboard(None).map_err(|e| format!("OpenClipboard failed: {e}"))?;

            let result = read_unicode_text();

            let _ = CloseClipboard();
            result
        }
    }

    unsafe fn read_unicode_text() -> Result<String, String> {
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT.0 as u32) }
            .map_err(|_| "no text on clipboard (CF_UNICODETEXT not available)".to_string())?;

        let hglobal = HGLOBAL(handle.0);
        let ptr = unsafe { GlobalLock(hglobal) };
        if ptr.is_null() {
            return Err("GlobalLock failed".into());
        }

        let text = {
            let wide_ptr = ptr as *const u16;
            let mut len = 0usize;
            while unsafe { *wide_ptr.add(len) } != 0 {
                len += 1;
            }
            let slice = unsafe { std::slice::from_raw_parts(wide_ptr, len) };
            String::from_utf16_lossy(slice)
        };

        let _ = unsafe { GlobalUnlock(hglobal) };
        Ok(text)
    }

    /// Write Unicode text to the clipboard.
    pub fn clipboard_write(text: &str) -> Result<(), String> {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let byte_len = wide.len() * size_of::<u16>();

        unsafe {
            OpenClipboard(None).map_err(|e| format!("OpenClipboard failed: {e}"))?;

            let result = write_unicode_text(&wide, byte_len);

            let _ = CloseClipboard();
            result
        }
    }

    unsafe fn write_unicode_text(wide: &[u16], byte_len: usize) -> Result<(), String> {
        unsafe { EmptyClipboard() }.map_err(|e| format!("EmptyClipboard failed: {e}"))?;

        let hmem = unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_len) }
            .map_err(|e| format!("GlobalAlloc failed: {e}"))?;

        let ptr = unsafe { GlobalLock(hmem) };
        if ptr.is_null() {
            return Err("GlobalLock failed".into());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr as *mut u16, wide.len());
            let _ = GlobalUnlock(hmem);
        }

        unsafe { SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hmem.0))) }
            .map_err(|e| format!("SetClipboardData failed: {e}"))?;

        Ok(())
    }

    fn size_of<T>() -> usize {
        std::mem::size_of::<T>()
    }
}

// ── Non-Windows stubs ──────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
pub fn clipboard_read() -> Result<String, String> {
    Err("clipboard_read is only supported on Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn clipboard_write(_text: &str) -> Result<(), String> {
    Err("clipboard_write is only supported on Windows".into())
}
