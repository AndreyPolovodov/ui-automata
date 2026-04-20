// Windows implementation of UIElement.
// Top-level module (element.rs); super:: resolves to the crate root.

use super::{
    Locator, Selector, UiError, check_click_point, check_keyboard_target, force_foreground,
};
use uiautomation::UIAutomation;
use uiautomation::inputs::{Keyboard, Mouse};
use uiautomation::patterns::{
    UIInvokePattern, UISelectionItemPattern, UITogglePattern, UIValuePattern, UIWindowPattern,
};
use uiautomation::types::{Point, ToggleState, TreeScope, WindowVisualState};

fn map_err(e: impl std::fmt::Display) -> UiError {
    UiError::Platform(e.to_string())
}

fn uia_rect(r: &uiautomation::types::Rect) -> visioncortex::BoundingRect {
    let left = r.get_left();
    let top = r.get_top();
    visioncortex::BoundingRect {
        left,
        top,
        right: left + r.get_width(),
        bottom: top + r.get_height(),
    }
}

/// Wraps a `uiautomation::UIElement` with a convenient automation API.
#[derive(Clone)]
pub struct UIElement {
    pub(crate) inner: uiautomation::UIElement,
}

impl UIElement {
    pub(crate) fn new(inner: uiautomation::UIElement) -> Self {
        Self { inner }
    }

    // ── Properties ───────────────────────────────────────────────────────

    pub fn name(&self) -> Option<String> {
        self.inner.get_name().ok().filter(|s| !s.is_empty())
    }

    pub fn name_or_empty(&self) -> String {
        self.inner.get_name().unwrap_or_default()
    }

    /// English role string (e.g. "button", "pane", "tool bar").
    pub fn role(&self) -> String {
        use uiautomation::types::ControlType;
        match self.inner.get_control_type() {
            Ok(ControlType::AppBar)      => "app bar".into(),
            Ok(ControlType::Button)      => "button".into(),
            Ok(ControlType::Calendar)    => "calendar".into(),
            Ok(ControlType::CheckBox)    => "check box".into(),
            Ok(ControlType::ComboBox)    => "combo box".into(),
            Ok(ControlType::Custom)      => "custom".into(),
            Ok(ControlType::DataGrid)    => "data grid".into(),
            Ok(ControlType::DataItem)    => "data item".into(),
            Ok(ControlType::Document)    => "document".into(),
            Ok(ControlType::Edit)        => "edit".into(),
            Ok(ControlType::Group)       => "group".into(),
            Ok(ControlType::Header)      => "header".into(),
            Ok(ControlType::HeaderItem)  => "header item".into(),
            Ok(ControlType::Hyperlink)   => "hyperlink".into(),
            Ok(ControlType::Image)       => "image".into(),
            Ok(ControlType::List)        => "list".into(),
            Ok(ControlType::ListItem)    => "list item".into(),
            Ok(ControlType::Menu)        => "menu".into(),
            Ok(ControlType::MenuBar)     => "menu bar".into(),
            Ok(ControlType::MenuItem)    => "menu item".into(),
            Ok(ControlType::Pane)        => "pane".into(),
            Ok(ControlType::ProgressBar) => "progress bar".into(),
            Ok(ControlType::RadioButton) => "radio button".into(),
            Ok(ControlType::ScrollBar)   => "scroll bar".into(),
            Ok(ControlType::SemanticZoom)=> "semantic zoom".into(),
            Ok(ControlType::Separator)   => "separator".into(),
            Ok(ControlType::Slider)      => "slider".into(),
            Ok(ControlType::Spinner)     => "spinner".into(),
            Ok(ControlType::SplitButton) => "split button".into(),
            Ok(ControlType::StatusBar)   => "status bar".into(),
            Ok(ControlType::Tab)         => "tab".into(),
            Ok(ControlType::TabItem)     => "tab item".into(),
            Ok(ControlType::Table)       => "table".into(),
            Ok(ControlType::Text)        => "text".into(),
            Ok(ControlType::Thumb)       => "thumb".into(),
            Ok(ControlType::TitleBar)    => "title bar".into(),
            Ok(ControlType::ToolBar)     => "tool bar".into(),
            Ok(ControlType::ToolTip)     => "tool tip".into(),
            Ok(ControlType::Tree)        => "tree".into(),
            Ok(ControlType::TreeItem)    => "tree item".into(),
            Ok(ControlType::Window) => {
                if self.inner.is_dialog().unwrap_or(false) { "dialog".into() } else { "window".into() }
            }
            _ => self.inner.get_localized_control_type().unwrap_or_default(),
        }
    }

    pub fn id(&self) -> Option<String> {
        self.inner
            .get_automation_id()
            .ok()
            .filter(|s| !s.is_empty())
    }

    pub fn id_or_empty(&self) -> String {
        self.inner.get_automation_id().unwrap_or_default()
    }

    pub fn process_id(&self) -> Result<u32, UiError> {
        self.inner.get_process_id().map_err(map_err)
    }

    /// Bounding box as `(x, y, width, height)`.
    pub fn bounds(&self) -> Result<(i32, i32, i32, i32), UiError> {
        self.inner
            .get_bounding_rectangle()
            .map_err(map_err)
            .map(|r| (r.get_left(), r.get_top(), r.get_width(), r.get_height()))
    }

    pub fn is_enabled(&self) -> Result<bool, UiError> {
        self.inner.is_enabled().map_err(map_err)
    }

    pub fn is_visible(&self) -> Result<bool, UiError> {
        self.inner.is_offscreen().map_err(map_err).map(|off| !off)
    }

    /// Lowercase control type string (e.g. "dialog", "property grid").
    pub fn control_type(&self) -> Result<String, UiError> {
        self.inner
            .get_localized_control_type()
            .map(|s| s.to_lowercase())
            .map_err(map_err)
    }

    /// The element's own name/label text.
    pub fn text(&self) -> Result<String, UiError> {
        text_of(&self.inner).map_err(Into::into)
    }

    /// Direct children's names joined by newlines, excluding the element's own name.
    pub fn inner_text(&self) -> Result<String, UiError> {
        let mut parts = Vec::new();
        for child in self.children()? {
            let child_name = child.inner.get_name().unwrap_or_default();
            if !child_name.is_empty() {
                parts.push(child_name);
            }
        }
        Ok(parts.join("\n"))
    }

    // ── Navigation ───────────────────────────────────────────────────────

    pub fn children(&self) -> Result<Vec<UIElement>, UiError> {
        let auto = UIAutomation::new_direct().map_err(map_err)?;
        let cond = auto.create_true_condition().map_err(map_err)?;
        self.inner
            .find_all(TreeScope::Children, &cond)
            .map_err(map_err)
            .map(|v| v.into_iter().map(UIElement::new).collect())
    }

    pub fn parent(&self) -> Result<Option<UIElement>, UiError> {
        let auto = UIAutomation::new_direct().map_err(map_err)?;
        let walker = auto.get_raw_view_walker().map_err(map_err)?;
        match walker.get_parent(&self.inner) {
            Ok(p) => Ok(Some(UIElement::new(p))),
            Err(_) => Ok(None), // at root or no parent
        }
    }

    // ── Focus ────────────────────────────────────────────────────────────

    pub fn focus(&self) -> Result<(), UiError> {
        self.inner.set_focus().map_err(map_err)
    }

    // ── Window management ────────────────────────────────────────────────

    /// Bring the window to the foreground, restoring it if minimized.
    pub fn activate_window(&self) -> Result<(), UiError> {
        let handle = self.inner.get_native_window_handle().map_err(map_err)?;
        force_foreground(handle.into()).map_err(|e| UiError::Platform(e))
    }

    pub fn minimize_window(&self) -> Result<(), UiError> {
        let wp = self
            .inner
            .get_pattern::<UIWindowPattern>()
            .map_err(|_| UiError::Internal("No WindowPattern".into()))?;
        wp.set_window_visual_state(WindowVisualState::Minimized)
            .map_err(map_err)
    }

    pub fn close(&self) -> Result<(), UiError> {
        let wp = self
            .inner
            .get_pattern::<UIWindowPattern>()
            .map_err(|_| UiError::Internal("No WindowPattern".into()))?;
        wp.close().map_err(map_err)
    }

    // ── Locator ──────────────────────────────────────────────────────────

    pub fn locator(&self, selector: Selector) -> Result<Locator, UiError> {
        Ok(Locator::new(self.clone(), selector))
    }
}

// ── ui_automata::Element impl ─────────────────────────────────────────────

impl ui_automata::Element for UIElement {
    fn name(&self) -> Option<String> {
        self.inner.get_name().ok().filter(|s| !s.is_empty())
    }

    fn role(&self) -> String {
        self.role()
    }

    fn text(&self) -> Result<String, ui_automata::AutomataError> {
        text_of(&self.inner).map_err(Into::into)
    }

    fn inner_text(&self) -> Result<String, ui_automata::AutomataError> {
        let mut parts = Vec::new();
        for child in self.children()? {
            let n = child.inner.get_name().unwrap_or_default();
            if !n.is_empty() {
                parts.push(n);
            }
        }
        Ok(parts.join("\n"))
    }

    fn is_enabled(&self) -> Result<bool, ui_automata::AutomataError> {
        self.inner.is_enabled().map_err(map_err).map_err(Into::into)
    }

    fn is_visible(&self) -> Result<bool, ui_automata::AutomataError> {
        self.inner
            .is_offscreen()
            .map_err(map_err)
            .map(|off| !off)
            .map_err(Into::into)
    }

    fn process_id(&self) -> Result<u32, ui_automata::AutomataError> {
        self.inner
            .get_process_id()
            .map_err(map_err)
            .map_err(Into::into)
    }

    fn process_name(&self) -> Option<String> {
        let pid = self.inner.get_process_id().ok()?;
        crate::get_process_name(pid as i32).ok()
    }

    fn bounds(&self) -> Result<(i32, i32, i32, i32), ui_automata::AutomataError> {
        self.inner
            .get_bounding_rectangle()
            .map_err(map_err)
            .map_err(Into::into)
            .map(|r| (r.get_left(), r.get_top(), r.get_width(), r.get_height()))
    }

    fn children(&self) -> Result<Vec<Self>, ui_automata::AutomataError> {
        let auto = UIAutomation::new_direct()
            .map_err(map_err)
            .map_err(Into::<ui_automata::AutomataError>::into)?;
        let cond = auto
            .create_true_condition()
            .map_err(map_err)
            .map_err(Into::<ui_automata::AutomataError>::into)?;
        self.inner
            .find_all(TreeScope::Children, &cond)
            .map_err(map_err)
            .map_err(Into::into)
            .map(|v| v.into_iter().map(UIElement::new).collect())
    }

    fn has_parent(&self) -> bool {
        let Ok(auto) = UIAutomation::new_direct() else {
            return false;
        };
        let Ok(walker) = auto.get_raw_view_walker() else {
            return false;
        };
        walker.get_parent(&self.inner).is_ok()
    }

    fn parent(&self) -> Option<Self> {
        let auto = UIAutomation::new_direct().ok()?;
        let walker = auto.get_raw_view_walker().ok()?;
        walker.get_parent(&self.inner).ok().map(UIElement::new)
    }

    fn click(&self) -> Result<(), ui_automata::AutomataError> {
        let (x, y, w, h) = ui_automata::Element::bounds(self)?;
        let cx = x + w / 2;
        let cy = y + h / 2;
        let pid = self
            .inner
            .get_process_id()
            .map_err(map_err)
            .map_err(Into::<ui_automata::AutomataError>::into)?;
        check_click_point(cx, cy, pid).map_err(ui_automata::AutomataError::Platform)?;
        Mouse::new()
            .click(Point::new(cx, cy))
            .map_err(map_err)
            .map_err(Into::into)
    }

    fn double_click(&self) -> Result<(), ui_automata::AutomataError> {
        let (x, y, w, h) = ui_automata::Element::bounds(self)?;
        let cx = x + w / 2;
        let cy = y + h / 2;
        let pid = self
            .inner
            .get_process_id()
            .map_err(map_err)
            .map_err(Into::<ui_automata::AutomataError>::into)?;
        check_click_point(cx, cy, pid).map_err(ui_automata::AutomataError::Platform)?;
        Mouse::new()
            .double_click(Point::new(cx, cy))
            .map_err(map_err)
            .map_err(Into::into)
    }

    fn hover(&self) -> Result<(), ui_automata::AutomataError> {
        let (x, y, w, h) = ui_automata::Element::bounds(self)?;
        let point = Point::new(x + w / 2, y + h / 2);
        Mouse::new()
            .move_to(point)
            .map_err(map_err)
            .map_err(Into::into)
    }

    fn click_at(
        &self,
        x_pct: f64,
        y_pct: f64,
        kind: ui_automata::ClickType,
    ) -> Result<(), ui_automata::AutomataError> {
        let (x, y, w, h) = ui_automata::Element::bounds(self)?;
        let px = (x as f64 + w as f64 * x_pct / 100.0) as i32;
        let py = (y as f64 + h as f64 * y_pct / 100.0) as i32;
        let pid = self
            .inner
            .get_process_id()
            .map_err(map_err)
            .map_err(Into::<ui_automata::AutomataError>::into)?;
        check_click_point(px, py, pid).map_err(ui_automata::AutomataError::Platform)?;
        let mouse = Mouse::new();
        if matches!(
            kind,
            ui_automata::ClickType::Triple | ui_automata::ClickType::Middle
        ) {
            let ct = match kind {
                ui_automata::ClickType::Triple => crate::ClickType::Triple,
                ui_automata::ClickType::Middle => crate::ClickType::Middle,
                _ => unreachable!(),
            };
            return crate::mouse_click(px, py, ct).map_err(ui_automata::AutomataError::Platform);
        }
        match kind {
            ui_automata::ClickType::Left => mouse.click(Point::new(px, py)),
            ui_automata::ClickType::Double => mouse.double_click(Point::new(px, py)),
            ui_automata::ClickType::Right => mouse.right_click(Point::new(px, py)),
            ui_automata::ClickType::Triple | ui_automata::ClickType::Middle => unreachable!(),
        }
        .map_err(map_err)
        .map_err(Into::into)
    }

    fn type_text(&self, text: &str) -> Result<(), ui_automata::AutomataError> {
        let pid = self
            .inner
            .get_process_id()
            .map_err(map_err)
            .map_err(Into::<ui_automata::AutomataError>::into)?;
        check_keyboard_target(pid).map_err(ui_automata::AutomataError::Platform)?;
        Keyboard::new()
            .send_text(text)
            .map_err(map_err)
            .map_err(Into::into)
    }

    fn press_key(&self, key: &str) -> Result<(), ui_automata::AutomataError> {
        let pid = self
            .inner
            .get_process_id()
            .map_err(map_err)
            .map_err(Into::<ui_automata::AutomataError>::into)?;
        check_keyboard_target(pid).map_err(ui_automata::AutomataError::Platform)?;
        Keyboard::new()
            .send_keys(&crate::input::normalise_key(key))
            .map_err(map_err)
            .map_err(Into::into)
    }

    fn set_value(&self, value: &str) -> Result<(), ui_automata::AutomataError> {
        self.inner
            .get_pattern::<UIValuePattern>()
            .and_then(|vp| vp.set_value(value))
            .map_err(map_err)
            .map_err(Into::into)
    }

    fn focus(&self) -> Result<(), ui_automata::AutomataError> {
        self.inner.set_focus().map_err(map_err).map_err(Into::into)
    }

    fn invoke(&self) -> Result<(), ui_automata::AutomataError> {
        // Try InvokePattern first — works without bounding rect or foreground.
        if let Ok(ip) = self.inner.get_pattern::<UIInvokePattern>() {
            return ip.invoke().map_err(map_err).map_err(Into::into);
        }
        // Try SelectionItemPattern::Select() — activates XAML list nav items
        // (e.g. Windows Settings nav, WinUI ListViews) without mouse or bounds.
        if let Ok(sp) = self.inner.get_pattern::<UISelectionItemPattern>() {
            return sp.select().map_err(map_err).map_err(Into::into);
        }
        Err(ui_automata::AutomataError::Platform(format!(
            "Invoke: element '{}' supports neither InvokePattern nor SelectionItemPattern",
            self.inner.get_name().unwrap_or_default()
        )))
    }

    fn scroll_into_view(&self) -> Result<(), ui_automata::AutomataError> {
        // Fast path: try ScrollItemPattern (handles virtualised lists correctly).
        if let Ok(sip) = self
            .inner
            .get_pattern::<uiautomation::patterns::UIScrollItemPattern>()
        {
            if sip.scroll_into_view().is_ok() {
                let bounds_ok = self
                    .inner
                    .get_bounding_rectangle()
                    .map(|r| r.get_width() > 1 && r.get_height() > 1)
                    .unwrap_or(false);
                log::info!("scroll_into_view: ScrollItemPattern result — bounds_ok={bounds_ok}");
                if bounds_ok {
                    return Ok(());
                }
            }
        }

        let auto = UIAutomation::new_direct().map_err(map_err)?;
        let walker = auto.get_raw_view_walker().map_err(map_err)?;

        let tr = uia_rect(&self.inner.get_bounding_rectangle().map_err(map_err)?);
        let anchor_rect = find_clipping_ancestor(&self.inner, &tr, &walker).map_err(map_err)?;
        let anchor_rect = match anchor_rect {
            Some(r) => r,
            None => {
                log::info!(
                    "scroll_into_view: reached root without finding a clipping ancestor — element already visible"
                );
                return Ok(());
            }
        };

        scroll_until_visible(&self.inner, &tr, &anchor_rect).map_err(Into::into)
    }

    fn activate_window(&self) -> Result<(), ui_automata::AutomataError> {
        let handle = self
            .inner
            .get_native_window_handle()
            .map_err(map_err)
            .map_err(Into::<ui_automata::AutomataError>::into)?;
        force_foreground(handle.into()).map_err(|e| ui_automata::AutomataError::Platform(e))
    }

    fn minimize_window(&self) -> Result<(), ui_automata::AutomataError> {
        self.inner
            .get_pattern::<UIWindowPattern>()
            .map_err(|_| ui_automata::AutomataError::Internal("No WindowPattern".into()))?
            .set_window_visual_state(WindowVisualState::Minimized)
            .map_err(map_err)
            .map_err(Into::into)
    }

    fn close(&self) -> Result<(), ui_automata::AutomataError> {
        self.inner
            .get_pattern::<UIWindowPattern>()
            .map_err(|_| ui_automata::AutomataError::Internal("No WindowPattern".into()))?
            .close()
            .map_err(map_err)
            .map_err(Into::into)
    }

    fn toggle_state(&self) -> Result<Option<bool>, ui_automata::AutomataError> {
        match self.inner.get_pattern::<UITogglePattern>() {
            Ok(tp) => {
                let state = tp.get_toggle_state()
                    .map_err(|e| ui_automata::AutomataError::Platform(e.to_string()))?;
                Ok(Some(state == ToggleState::On))
            }
            Err(_) => Ok(None),
        }
    }

    fn toggle(&self) -> Result<(), ui_automata::AutomataError> {
        self.inner
            .get_pattern::<UITogglePattern>()
            .map_err(|_| ui_automata::AutomataError::Internal(format!(
                "Toggle: element '{}' does not support TogglePattern",
                self.inner.get_name().unwrap_or_default()
            )))?
            .toggle()
            .map_err(|e| ui_automata::AutomataError::Platform(e.to_string()))
    }

    fn hwnd(&self) -> Option<u64> {
        let handle: isize = self.inner.get_native_window_handle().ok()?.into();
        Some(handle as u64)
    }

    fn automation_id(&self) -> Option<String> {
        self.inner
            .get_automation_id()
            .ok()
            .filter(|s| !s.is_empty())
    }

    fn help_text(&self) -> Option<String> {
        self.inner
            .get_help_text()
            .ok()
            .filter(|s| !s.is_empty())
    }
}

// ── Scroll helpers ────────────────────────────────────────────────────────────

/// Walk up the UIA ancestor chain to find the first ancestor that clips the
/// target element (i.e. at least one corner of the target falls outside the
/// ancestor's bounding rect). Returns `None` if no such ancestor is found
/// (element is already fully visible).
fn find_clipping_ancestor(
    el: &uiautomation::UIElement,
    target_rect: &visioncortex::BoundingRect,
    walker: &uiautomation::core::UITreeWalker,
) -> Result<Option<visioncortex::BoundingRect>, UiError> {
    let corners = [
        target_rect.left_top(),
        target_rect.top_right(),
        target_rect.bottom_left(),
        target_rect.right_bottom(),
    ];

    log::info!(
        "scroll_into_view: target '{}' role={} rect=[{},{} {}x{}]",
        el.get_name().unwrap_or_default(),
        el.get_control_type()
            .map(|t| format!("{t:?}"))
            .unwrap_or_default(),
        target_rect.left,
        target_rect.top,
        target_rect.right - target_rect.left,
        target_rect.bottom - target_rect.top,
    );

    let mut candidate = el.clone();
    loop {
        let parent = match walker.get_parent(&candidate) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        let pr = uia_rect(&parent.get_bounding_rectangle().map_err(map_err)?);
        if !pr.is_empty() && corners.iter().any(|&p| !pr.have_point_inside(p)) {
            log::info!(
                "scroll_into_view: clipping ancestor '{}' role={} rect=[{},{} {}x{}]",
                parent.get_name().unwrap_or_default(),
                parent
                    .get_control_type()
                    .map(|t| format!("{t:?}"))
                    .unwrap_or_default(),
                pr.left,
                pr.top,
                pr.right - pr.left,
                pr.bottom - pr.top,
            );
            return Ok(Some(pr));
        }
        candidate = parent;
    }
}

/// Scroll the container defined by `anchor_rect` until `el` is fully visible
/// within it, or until scrolling stalls (up to 200 ticks per pass).
fn scroll_until_visible(
    el: &uiautomation::UIElement,
    target_rect: &visioncortex::BoundingRect,
    anchor_rect: &visioncortex::BoundingRect,
) -> Result<(), UiError> {
    use super::{ScrollAxis, move_cursor, scroll_wheel};
    use std::thread;
    use std::time::Duration;

    let outside_v = target_rect.top < anchor_rect.top || target_rect.bottom > anchor_rect.bottom;
    let is_degenerate_start = target_rect.left == 0
        && target_rect.top == 0
        && target_rect.right <= 1
        && target_rect.bottom <= 1;

    // Negative = scroll down/right; positive = scroll up/left.
    // When starting degenerate we don't know which way, so begin with -1.
    let initial_delta: i32 = if is_degenerate_start {
        -1
    } else if outside_v {
        if target_rect.top > anchor_rect.bottom {
            -1
        } else {
            1
        }
    } else {
        if target_rect.left > anchor_rect.right {
            1
        } else {
            -1
        }
    };

    log::info!(
        "scroll_into_view: axis={} initial_delta={} degenerate_start={is_degenerate_start}",
        if outside_v { "vertical" } else { "horizontal" },
        initial_delta,
    );

    let anchor_center = anchor_rect.center();
    move_cursor(anchor_center.x, anchor_center.y);
    thread::sleep(Duration::from_millis(30));

    let passes: &[i32] = if is_degenerate_start {
        &[initial_delta, -initial_delta]
    } else {
        &[initial_delta]
    };

    for &delta in passes {
        log::info!("scroll_into_view: pass delta={delta}");
        let mut prev_top = i32::MIN;
        let mut prev_left = i32::MIN;

        for tick in 0..200 {
            if outside_v {
                scroll_wheel(ScrollAxis::Vertical, delta);
            } else {
                scroll_wheel(ScrollAxis::Horizontal, delta);
            }
            thread::sleep(Duration::from_millis(50));

            let nr = uia_rect(&el.get_bounding_rectangle().map_err(map_err)?);
            log::trace!(
                "scroll_into_view: tick={tick} target rect=[{},{} {}x{}]",
                nr.left,
                nr.top,
                nr.right - nr.left,
                nr.bottom - nr.top,
            );
            let new_corners = [
                nr.left_top(),
                nr.top_right(),
                nr.bottom_left(),
                nr.right_bottom(),
            ];
            if new_corners
                .iter()
                .all(|&p| anchor_rect.have_point_inside(p))
            {
                log::info!("scroll_into_view: element now visible after {tick} ticks");
                return Ok(());
            }

            let is_degenerate = nr.left == 0 && nr.top == 0 && nr.right <= 1 && nr.bottom <= 1;
            if !is_degenerate && nr.top == prev_top && nr.left == prev_left {
                log::info!("scroll_into_view: stalled at tick={tick}");
                break;
            }
            prev_top = nr.top;
            prev_left = nr.left;
        }
    }

    Err(UiError::Internal(format!(
        "scroll_into_view: '{}' could not be scrolled into view",
        el.get_name().unwrap_or_default()
    )))
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Read the text value from a UIA element: try ValuePattern first, fall back to name.
fn text_of(el: &uiautomation::UIElement) -> Result<String, UiError> {
    if let Ok(vp) = el.get_pattern::<UIValuePattern>() {
        if let Ok(val) = vp.get_value() {
            return Ok(val);
        }
    }
    Ok(el.get_name().unwrap_or_default())
}
