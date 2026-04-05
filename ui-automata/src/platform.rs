use schemars::JsonSchema;
use serde::Deserialize;

use crate::AutomataError;

// ── Browser / TabInfo ─────────────────────────────────────────────────────────

/// Basic info about a browser tab.
#[derive(Debug, Clone)]
pub struct TabInfo {
    pub title: String,
    pub url: String,
}

/// CDP browser: operations for controlling browser sessions and tabs.
pub trait Browser: Send + Sync + 'static {
    /// Ensure the browser is running with CDP enabled.
    fn ensure(&self) -> Result<(), AutomataError>;
    /// Open a new tab at `url` (or about:blank). Returns the CDP tab ID.
    fn open_tab(&self, url: Option<&str>) -> Result<String, AutomataError>;
    /// Close a tab by CDP target ID.
    fn close_tab(&self, tab_id: &str) -> Result<(), AutomataError>;
    /// Bring a tab to the foreground (switch to it).
    fn activate_tab(&self, tab_id: &str) -> Result<(), AutomataError>;
    /// Navigate a tab to a URL.
    fn navigate(&self, tab_id: &str, url: &str) -> Result<(), AutomataError>;
    /// Evaluate a JS expression in a tab. Returns the string result.
    fn eval(&self, tab_id: &str, expr: &str) -> Result<String, AutomataError>;
    /// Title + URL of a specific tab.
    fn tab_info(&self, tab_id: &str) -> Result<TabInfo, AutomataError>;
    /// All open tabs: (tab_id, TabInfo).
    fn tabs(&self) -> Result<Vec<(String, TabInfo)>, AutomataError>;
}

/// Type of mouse click to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClickType {
    Left,
    Double,
    Triple,
    Right,
    Middle,
}

/// A UI element handle. Implementations wrap platform-specific COM/UIA objects.
///
/// All property methods return `None` / empty on a stale handle; interaction
/// methods return `Err(AutomataError::Platform(...))` on staleness or failure.
pub trait Element: Clone + 'static {
    // ── Properties ───────────────────────────────────────────────────────────

    /// Localized name / label of the element. `None` if empty or unavailable.
    fn name(&self) -> Option<String>;

    /// Localized role string from `GetLocalizedControlType` (e.g. "button", "pane", "tool bar").
    fn role(&self) -> String;

    /// Value text (ValuePattern) or name fallback.
    fn text(&self) -> Result<String, AutomataError>;

    /// Direct children's names joined by newlines, excluding the element's own name.
    fn inner_text(&self) -> Result<String, AutomataError>;

    fn is_enabled(&self) -> Result<bool, AutomataError>;
    fn is_visible(&self) -> Result<bool, AutomataError>;
    fn process_id(&self) -> Result<u32, AutomataError>;
    /// Process name (without .exe) for this element's owning process.
    /// Returns `None` on non-Windows or if the lookup fails.
    fn process_name(&self) -> Option<String> {
        None
    }
    /// Native window handle (HWND as u64) for this element's owning window.
    /// Returns `None` on non-Windows or if the lookup fails.
    fn hwnd(&self) -> Option<u64> {
        None
    }

    /// UIA AutomationId property. `None` if empty or unavailable.
    fn automation_id(&self) -> Option<String> {
        None
    }

    /// Bounding box as `(x, y, width, height)`.
    fn bounds(&self) -> Result<(i32, i32, i32, i32), AutomataError>;

    /// Direct children. Returns `Err` on a stale handle.
    fn children(&self) -> Result<Vec<Self>, AutomataError>;

    /// Returns `false` when the element has been detached from the accessibility
    /// tree (e.g. a dismissed dialog). Root windows have the desktop as parent so
    /// they return `true`. Default returns `true` for platforms that don't implement it.
    fn has_parent(&self) -> bool {
        true
    }

    /// Navigate to this element's parent. Returns `None` at the root or on error.
    fn parent(&self) -> Option<Self> {
        None
    }

    // ── Interactions ─────────────────────────────────────────────────────────

    fn click(&self) -> Result<(), AutomataError>;
    fn double_click(&self) -> Result<(), AutomataError>;

    /// Move the mouse cursor to the centre of the element without clicking.
    fn hover(&self) -> Result<(), AutomataError>;

    /// Click at a position expressed as fractions of the element's bounding box.
    fn click_at(&self, x_pct: f64, y_pct: f64, kind: ClickType) -> Result<(), AutomataError>;

    fn type_text(&self, text: &str) -> Result<(), AutomataError>;
    fn press_key(&self, key: &str) -> Result<(), AutomataError>;

    /// Set a field's value directly via IValuePattern (avoids needing to
    /// select-all + type). Preferred over `type_text` for pre-filled fields.
    fn set_value(&self, value: &str) -> Result<(), AutomataError>;

    fn focus(&self) -> Result<(), AutomataError>;

    /// Activate this element via UIA's `IInvokePattern::Invoke()`.
    ///
    /// Unlike `click()`, `invoke()` does not require a valid bounding rect —
    /// it works on off-screen elements whose bounds are `(0,0,1,1)` because they
    /// are scrolled out of view.  Prefer this over `Click` + `ScrollIntoView`
    /// for items in virtualised or scrollable lists (e.g. Settings nav items,
    /// WinUI ListView rows) where mouse-wheel scrolling causes elastic snap-back.
    ///
    /// Also tries `SelectionItemPattern` when `InvokePattern` is unavailable.
    /// Returns an error if neither pattern is supported — does not fall back to `click()`.
    fn invoke(&self) -> Result<(), AutomataError> {
        // Default: fall back to click for platforms that don't override this.
        self.click()
    }

    /// Scroll ancestor containers until this element is within their visible
    /// viewport. Uses `ScrollItemPattern` when supported; falls back to a
    /// geometric ancestor walk with `ScrollPattern`.
    fn scroll_into_view(&self) -> Result<(), AutomataError>;

    fn activate_window(&self) -> Result<(), AutomataError>;
    fn minimize_window(&self) -> Result<(), AutomataError>;
    fn close(&self) -> Result<(), AutomataError>;
}

/// Platform desktop: discovers windows and provides foreground state.
pub trait Desktop: Send + 'static {
    type Elem: Element;
    type Browser: Browser;

    /// CDP browser handle. Used by the workflow engine for browser automation.
    fn browser(&self) -> &Self::Browser;

    /// All top-level application windows currently visible.
    fn application_windows(&self) -> Result<Vec<Self::Elem>, AutomataError>;

    /// Launch an executable by name or full path. Returns the process ID.
    fn open_application(&self, exe: &str) -> Result<u32, AutomataError>;

    /// The element for the current OS foreground window (`GetForegroundWindow`).
    /// Returns `None` if there is no foreground window or the lookup fails.
    fn foreground_window(&self) -> Option<Self::Elem>;

    /// Raw HWND as `u64` for process-ownership checks without a full element
    /// query. Returns `None` on non-Windows or when nothing is focused.
    fn foreground_hwnd(&self) -> Option<u64>;

    /// All currently visible tooltip windows on the desktop.
    /// Returns an empty vec on non-Windows or if none are present.
    fn tooltip_windows(&self) -> Vec<Self::Elem> {
        vec![]
    }

    /// Top-level window handles in Z-order (topmost first).
    ///
    /// Used by the anchor resolver to prefer the topmost window of a process
    /// when multiple windows match the same filter. Returns an empty vec on
    /// non-Windows or if the enumeration fails.
    fn hwnd_z_order(&self) -> Vec<u64> {
        vec![]
    }
}
