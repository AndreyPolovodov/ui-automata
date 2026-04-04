use crate::ElementInfo;

// ── Non-Windows stub ──────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
use crate::Result;

#[cfg(not(target_os = "windows"))]
pub struct UiaProbe;

#[cfg(not(target_os = "windows"))]
impl UiaProbe {
    pub fn new() -> Result<Self> {
        anyhow::bail!("Windows only")
    }
    pub fn at(&self, _x: i32, _y: i32) -> Option<ElementInfo> {
        None
    }
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub use win::UiaProbe;

#[cfg(target_os = "windows")]
mod win {
    use uiautomation::UIAutomation;
    use uiautomation::types::Point;

    use super::ElementInfo;
    use crate::Result;

    /// Queries UIA for the element under a given screen point.
    /// Create once per thread; COM must be initialised before calling `new`.
    pub struct UiaProbe {
        automation: UIAutomation,
    }

    impl UiaProbe {
        pub fn new() -> Result<Self> {
            Ok(Self {
                automation: UIAutomation::new_direct()?,
            })
        }

        /// Returns element properties for the element at `(x, y)`, or `None` on failure.
        pub fn at(&self, x: i32, y: i32) -> Option<ElementInfo> {
            let (_, info) = self.at_with_ancestors(x, y)?;
            Some(info)
        }

        /// Returns the ancestor chain (root first) and the element at `(x, y)`.
        pub fn at_with_ancestors(&self, x: i32, y: i32) -> Option<(Vec<ElementInfo>, ElementInfo)> {
            let element = self.automation.element_from_point(Point::new(x, y)).ok()?;
            let walker = self.automation.get_control_view_walker().ok()?;

            // Walk up the tree collecting ancestors (closest parent first).
            let mut ancestors_rev: Vec<ElementInfo> = Vec::new();
            let mut current = element.clone();
            loop {
                match walker.get_parent(&current) {
                    Ok(parent) => {
                        ancestors_rev.push(element_info(&parent));
                        current = parent;
                    }
                    Err(_) => break,
                }
            }
            ancestors_rev.reverse(); // now root first

            Some((ancestors_rev, element_info(&element)))
        }
    }

    fn element_info(element: &uiautomation::UIElement) -> ElementInfo {
        let name = element.get_name().unwrap_or_default();
        let role = element.get_localized_control_type().unwrap_or_else(|_| {
            element
                .get_control_type()
                .map(|ct| format!("{ct:?}"))
                .unwrap_or_default()
        });
        let class = element.get_classname().unwrap_or_default();
        let automation_id = element.get_automation_id().unwrap_or_default();
        let enabled = element.is_enabled().unwrap_or(false);
        let focusable = element.is_keyboard_focusable().unwrap_or(false);
        let value = element
            .get_pattern::<uiautomation::patterns::UIValuePattern>()
            .ok()
            .and_then(|p| p.get_value().ok())
            .unwrap_or_default();
        let (x, y, w, h) = element
            .get_bounding_rectangle()
            .map(|r| (r.get_left(), r.get_top(), r.get_width(), r.get_height()))
            .unwrap_or((0, 0, 0, 0));

        ElementInfo {
            name,
            role,
            class,
            automation_id,
            value,
            enabled,
            focusable,
            x,
            y,
            w,
            h,
        }
    }
}
