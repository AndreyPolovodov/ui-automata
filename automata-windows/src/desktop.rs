use super::{UIElement, UiError};

/// Entry point for UI automation: enumerate windows and launch applications.
pub struct Desktop {
    #[cfg(target_os = "windows")]
    browser: crate::browser::WindowsBrowser,
}

impl Desktop {
    pub fn new() -> Self {
        Desktop {
            #[cfg(target_os = "windows")]
            browser: crate::browser::WindowsBrowser::new(automata_browser::DEFAULT_PORT),
        }
    }
}

impl Default for Desktop {
    fn default() -> Self {
        Self::new()
    }
}

// ── Non-Windows stub ──────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
impl Desktop {
    pub fn application_windows(&self) -> Result<Vec<UIElement>, UiError> {
        Err(UiError::Platform("Windows only".into()))
    }

    pub fn open_application(&self, _exe: &str) -> Result<u32, UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
}

#[cfg(not(target_os = "windows"))]
impl ui_automata::Desktop for Desktop {
    type Elem = UIElement;
    type Browser = crate::element::NoBrowser;

    fn browser(&self) -> &crate::element::NoBrowser {
        unimplemented!("Browser not supported on this platform")
    }

    fn application_windows(&self) -> Result<Vec<UIElement>, ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }

    fn open_application(&self, _exe: &str) -> Result<u32, ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }

    fn foreground_window(&self) -> Option<UIElement> {
        None
    }

    fn foreground_hwnd(&self) -> Option<u64> {
        None
    }
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
impl Desktop {
    /// Return all top-level Window/Pane elements visible to UIA.
    pub fn application_windows(&self) -> Result<Vec<UIElement>, UiError> {
        use crate::util::window_pane_condition;
        use uiautomation::UIAutomation;
        use uiautomation::types::TreeScope;

        let auto = UIAutomation::new_direct().map_err(|e| UiError::Platform(e.to_string()))?;
        let root = auto
            .get_root_element()
            .map_err(|e| UiError::Platform(e.to_string()))?;
        let cond = window_pane_condition(&auto).map_err(|e| UiError::Platform(e.to_string()))?;
        root.find_all(TreeScope::Children, &cond)
            .map_err(|e| UiError::Platform(e.to_string()))
            .map(|v| v.into_iter().map(UIElement::new).collect())
    }

    /// Launch an application and return its process ID.
    ///
    /// Launch strategies tried in order:
    ///
    /// 1. **Start Menu AppID** (`{GUID}\path\to\App.exe`):
    ///    `IApplicationActivationManager::ActivateApplication`, then `shell:AppsFolder\{id}`.
    ///
    /// 2. **Bare exe name** (no path separators, ends with `.exe`):
    ///    a. `App Paths` registry lookup → `CreateProcessW` with resolved path.
    ///    b. Start Menu enumeration — resolve `.lnk` shortcut to full path → `CreateProcessW`.
    ///
    /// 3. **Full path or URI**:
    ///    `CreateProcessW` directly.
    pub fn open_application(&self, app_id: &str) -> Result<u32, UiError> {
        // ── Strategy 1a: GUID-style Start Menu AppID ({GUID}\...) ─────────────
        if app_id.starts_with('{') && app_id.contains('\\') {
            if let Ok(pid) = self.activate_via_com(app_id) {
                return Ok(pid);
            }
            let shell_path = format!("shell:AppsFolder\\{app_id}");
            return self.shell_open(&shell_path);
        }

        // ── Strategy 1b: UWP Package!App format ───────────────────────────────
        // e.g. "Microsoft.WindowsStore_8wekyb3d8bbwe!App"
        if app_id.contains('!') && !app_id.contains(['/', '\\']) {
            if let Ok(pid) = self.activate_via_com(app_id) {
                return Ok(pid);
            }
            // Fallback: try shell:AppsFolder\ prefix
            let shell_path = format!("shell:AppsFolder\\{app_id}");
            return self.shell_open(&shell_path);
        }

        // ── Strategy 2: bare exe name ─────────────────────────────────────────
        let is_bare = !app_id.contains(['/', '\\']) && app_id.to_lowercase().ends_with(".exe");
        if is_bare {
            // 2a. App Paths registry
            if let Some(full_path) = self.resolve_app_path(app_id) {
                return self.shell_execute(&full_path);
            }
            // 2b. Start Menu: resolve .lnk to full path, then launch directly
            if let Some(full_path) = self.find_start_menu_app(app_id) {
                return self.shell_execute(&full_path);
            }
        }

        // ── Strategy 3: URI scheme ────────────────────────────────────────────
        // Explicit URI: colon present with 2+ char scheme prefix, no path separators.
        // Single-char prefix = drive letter (C:\...).
        let is_explicit_uri = app_id
            .find(':')
            .map(|i| i >= 2 && !app_id[..i].contains(['/', '\\']))
            .unwrap_or(false);
        if is_explicit_uri {
            return self.shell_open(app_id);
        }

        // Bare scheme name (e.g. "ms-windows-store", "ms-settings"): no colon,
        // no path separators, no dots, no .exe — treat as URI by appending ':'.
        let is_bare_scheme = !app_id.contains(['.', '/', '\\', ':'])
            && app_id
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
        if is_bare_scheme {
            let uri = format!("{app_id}:");
            return self.shell_open(&uri);
        }

        // ── Strategy 4: full executable path ─────────────────────────────────
        self.shell_execute(app_id)
    }

    /// Search Start Menu entries (both system and user) for one whose AppID
    /// file stem matches `exe_name` (case-insensitive). Returns the AppID.
    fn find_start_menu_app(&self, exe_name: &str) -> Option<String> {
        let needle = exe_name.to_lowercase();
        for root in self.start_menu_roots() {
            if let Some(id) = Self::search_start_menu_dir(&root, &needle) {
                return Some(id);
            }
        }
        None
    }

    /// Enumerate `.lnk` shortcuts under `dir` recursively, resolving each to
    /// its target path. Returns the AppID of the first entry whose filename
    /// (lowercased) matches `needle`.
    fn search_start_menu_dir(dir: &std::path::Path, needle: &str) -> Option<String> {
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(id) = Self::search_start_menu_dir(&path, needle) {
                    return Some(id);
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("lnk") {
                if let Some(target) = Self::resolve_lnk(&path) {
                    let stem = std::path::Path::new(&target)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    if stem == needle {
                        return Some(target);
                    }
                }
            }
        }
        None
    }

    /// Resolve a `.lnk` shell shortcut to its target path using `IShellLink`.
    fn resolve_lnk(lnk: &std::path::Path) -> Option<String> {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            IPersistFile,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
        use windows::core::{HRESULT, Interface, PCWSTR};

        unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            if hr.is_err() && hr != HRESULT(0x80010106u32 as i32) {
                return None;
            }
            let shell_link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            let persist: IPersistFile = shell_link.cast().ok()?;
            let path_wide: Vec<u16> = lnk
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            use windows::Win32::System::Com::STGM;
            persist.Load(PCWSTR(path_wide.as_ptr()), STGM(0)).ok()?;
            shell_link.Resolve(HWND(std::ptr::null_mut()), 0).ok()?;
            let mut buf = vec![0u16; 260];
            shell_link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
            let nul = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..nul]))
        }
    }

    /// Return Start Menu program folders to search (system then user).
    fn start_menu_roots(&self) -> Vec<std::path::PathBuf> {
        use windows::Win32::UI::Shell::{
            FOLDERID_CommonPrograms, FOLDERID_Programs, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
        };
        let mut roots = Vec::new();
        for folder_id in [&FOLDERID_CommonPrograms, &FOLDERID_Programs] {
            unsafe {
                if let Ok(p) = SHGetKnownFolderPath(folder_id, KF_FLAG_DEFAULT, None) {
                    let wide = p.as_wide();
                    let path = String::from_utf16_lossy(wide);
                    roots.push(std::path::PathBuf::from(path));
                }
            }
        }
        roots
    }

    /// Look up a bare exe name in `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths`.
    /// Returns the full path if registered, or `None`.
    fn resolve_app_path(&self, exe: &str) -> Option<String> {
        use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};

        let subkey: Vec<u16> =
            format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{exe}\0")
                .encode_utf16()
                .collect();

        let mut buf = vec![0u16; 512];
        let mut len = (buf.len() * 2) as u32;

        let ok = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                windows::core::PCWSTR(subkey.as_ptr()),
                None,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut len),
            )
        };

        if ok.is_ok() {
            let nul = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..nul]))
        } else {
            None
        }
    }

    /// Launch a Start Menu app via COM `IApplicationActivationManager`.
    fn activate_via_com(&self, app_id: &str) -> Result<u32, UiError> {
        use windows::Win32::System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        };
        use windows::Win32::UI::Shell::{
            ACTIVATEOPTIONS, ApplicationActivationManager, IApplicationActivationManager,
        };
        use windows::core::{HRESULT, HSTRING};

        unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            // 0x80010106 = RPC_E_CHANGED_MODE: COM already initialized on this thread
            if hr.is_err() && hr != HRESULT(0x80010106u32 as i32) {
                return Err(UiError::Platform(format!("CoInitializeEx failed: {hr}")));
            }

            let manager: IApplicationActivationManager =
                CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_ALL)
                    .map_err(|e| UiError::Platform(format!("CoCreateInstance failed: {e}")))?;

            let pid = manager
                .ActivateApplication(
                    &HSTRING::from(app_id),
                    &HSTRING::from(""),
                    ACTIVATEOPTIONS(0),
                )
                .map_err(|e| UiError::Platform(format!("ActivateApplication failed: {e}")))?;

            Ok(pid)
        }
    }

    /// Open a URI, shell path, or executable via `ShellExecuteW` with verb "open".
    /// Use this for URI schemes (`ms-windows-store:`, `ms-settings:`, `shell:AppsFolder\...`)
    /// and anything that needs the shell's association table. Returns 0 as PID (not available).
    fn shell_open(&self, target: &str) -> Result<u32, UiError> {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::PCWSTR;

        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let file: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();

        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(verb.as_ptr()),
                PCWSTR(file.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };

        // ShellExecuteW returns an HINSTANCE; values > 32 indicate success.
        let code = result.0 as usize;
        if code <= 32 {
            return Err(UiError::Platform(format!(
                "ShellExecuteW failed for '{target}': error code {code}"
            )));
        }
        Ok(0) // PID not available via ShellExecuteW
    }

    /// Launch an executable via `CreateProcessW` (searches PATH, no UAC prompt for normal apps).
    fn shell_execute(&self, target: &str) -> Result<u32, UiError> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::CREATE_NO_WINDOW;
        use windows::Win32::System::Threading::{
            CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
        };

        let mut cmdline: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
        let startup_info = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut process_info = PROCESS_INFORMATION::default();

        unsafe {
            CreateProcessW(
                None,
                Some(windows::core::PWSTR::from_raw(cmdline.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_NO_WINDOW,
                None,
                None,
                &startup_info,
                &mut process_info,
            )
            .map_err(|e| UiError::Platform(format!("CreateProcessW failed for '{target}': {e}")))?;

            let pid = process_info.dwProcessId;
            let _ = CloseHandle(process_info.hProcess);
            let _ = CloseHandle(process_info.hThread);
            Ok(pid)
        }
    }
}

#[cfg(target_os = "windows")]
impl ui_automata::Desktop for Desktop {
    type Elem = UIElement;
    type Browser = crate::browser::WindowsBrowser;

    fn browser(&self) -> &crate::browser::WindowsBrowser {
        &self.browser
    }

    fn application_windows(&self) -> Result<Vec<UIElement>, ui_automata::AutomataError> {
        self.application_windows().map_err(Into::into)
    }

    fn open_application(&self, exe: &str) -> Result<u32, ui_automata::AutomataError> {
        self.open_application(exe).map_err(Into::into)
    }

    fn foreground_window(&self) -> Option<UIElement> {
        use uiautomation::UIAutomation;
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            return None;
        }
        let auto = UIAutomation::new_direct().ok()?;
        auto.element_from_handle(hwnd.into())
            .ok()
            .map(UIElement::new)
    }

    fn foreground_hwnd(&self) -> Option<u64> {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            None
        } else {
            Some(hwnd.0 as u64)
        }
    }

    fn hwnd_z_order(&self) -> Vec<u64> {
        use windows::Win32::UI::WindowsAndMessaging::{
            GW_HWNDNEXT, GetTopWindow, GetWindow, IsWindowVisible,
        };

        let mut result = Vec::new();
        unsafe {
            // None = desktop root; returns the topmost top-level window (Z-order head).
            let mut hwnd = GetTopWindow(None);
            while let Ok(h) = hwnd {
                if !h.is_invalid() {
                    if IsWindowVisible(h).as_bool() {
                        result.push(h.0 as usize as u64);
                    }
                    hwnd = GetWindow(h, GW_HWNDNEXT);
                } else {
                    break;
                }
            }
        }
        result
    }

    fn tooltip_windows(&self) -> Vec<UIElement> {
        use uiautomation::UIAutomation;
        use uiautomation::controls::ControlType;
        use uiautomation::types::{TreeScope, UIProperty};
        use uiautomation::variants::Variant;

        let Ok(auto) = UIAutomation::new_direct() else {
            return vec![];
        };
        let Ok(root) = auto.get_root_element() else {
            return vec![];
        };
        let Ok(cond) = auto.create_property_condition(
            UIProperty::ControlType,
            Variant::from(ControlType::ToolTip as i32),
            None,
        ) else {
            return vec![];
        };
        root.find_all(TreeScope::Children, &cond)
            .unwrap_or_default()
            .into_iter()
            .map(UIElement::new)
            .collect()
    }
}
