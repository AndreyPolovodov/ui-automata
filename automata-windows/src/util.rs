/// Shared UIA helpers used across multiple modules.

#[cfg(target_os = "windows")]
pub use windows_impl::*;

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::thread;
    use std::time::{Duration, Instant};

    use uiautomation::controls::ControlType;
    use uiautomation::inputs::Mouse;
    use uiautomation::patterns::UIInvokePattern;
    use uiautomation::types::{Point, TreeScope, UIProperty};
    use uiautomation::variants::Variant;
    use uiautomation::{UIAutomation, UIElement};

    /// Build a UIA condition that matches Window or Pane control types.
    ///
    /// Used to enumerate top-level windows via `find_all(TreeScope::Children, &cond)`.
    pub fn window_pane_condition(
        auto: &UIAutomation,
    ) -> anyhow::Result<uiautomation::core::UICondition> {
        let cond_window = auto.create_property_condition(
            UIProperty::ControlType,
            Variant::from(ControlType::Window as i32),
            None,
        )?;
        let cond_pane = auto.create_property_condition(
            UIProperty::ControlType,
            Variant::from(ControlType::Pane as i32),
            None,
        )?;
        Ok(auto.create_or_condition(cond_window, cond_pane)?)
    }

    /// Try `UIInvokePattern` first; if that fails, click the element's centre.
    ///
    /// Returns an error only if both invoke and click fail.
    pub fn invoke_or_click(el: &UIElement) -> Result<(), String> {
        if let Ok(ip) = el.get_pattern::<UIInvokePattern>() {
            return ip.invoke().map_err(|e| format!("invoke failed: {e}"));
        }
        let rect = el
            .get_bounding_rectangle()
            .map_err(|e| format!("could not get element bounds: {e}"))?;
        let cx = rect.get_left() + rect.get_width() / 2;
        let cy = rect.get_top() + rect.get_height() / 2;
        Mouse::new()
            .click(Point::new(cx, cy))
            .map_err(|e| format!("mouse click failed: {e}"))
    }

    /// Find the first descendant whose `Name` property equals `name`.
    pub fn find_named(auto: &UIAutomation, root: &UIElement, name: &str) -> Option<UIElement> {
        auto.create_property_condition(UIProperty::Name, Variant::from(name), None)
            .ok()
            .and_then(|c| root.find_first(TreeScope::Descendants, &c).ok())
    }

    /// Poll until a descendant named `name` appears, or `timeout` elapses.
    ///
    /// Returns `None` if the element did not appear before the deadline.
    pub fn find_named_timeout(
        auto: &UIAutomation,
        root: &UIElement,
        name: &str,
        timeout: Duration,
    ) -> Option<UIElement> {
        let deadline = Instant::now() + timeout;
        let mut delay_ms = 50u64;
        loop {
            if let Some(el) = find_named(auto, root, name) {
                return Some(el);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(delay_ms));
            delay_ms = (delay_ms * 2).min(1000);
        }
    }
}
