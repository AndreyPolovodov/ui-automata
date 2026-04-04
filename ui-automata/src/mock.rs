/// Minimal mock implementations of `Element` and `Desktop` for unit tests.
///
/// Enabled via `#[cfg(any(test, feature = "mock"))]`.
use std::sync::{Arc, Mutex};

use crate::{AutomataError, Browser, ClickType, Desktop, Element, TabInfo};

// ── MockElement ───────────────────────────────────────────────────────────────

/// A scriptable element node. Use the builder helpers to construct trees.
#[derive(Clone)]
pub struct MockElement {
    inner: Arc<MockElementInner>,
}

struct MockElementInner {
    pub role: String,
    pub name: Option<String>,
    /// Overrides `name` for the `text()` call. If `None`, `text()` falls back to `name`.
    pub text: Option<String>,
    pub automation_id: Option<String>,
    pub alive: Mutex<bool>,
    pub children: Mutex<Vec<MockElement>>,
    pub parent: Mutex<Option<std::sync::Weak<MockElementInner>>>,
}

impl MockElement {
    /// Create a leaf element.
    pub fn leaf(role: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(MockElementInner {
                role: role.into(),
                name: Some(name.into()),
                text: None,
                automation_id: None,
                alive: Mutex::new(true),
                children: Mutex::new(vec![]),
                parent: Mutex::new(None),
            }),
        }
    }

    /// Create a leaf where `name()` and `text()` return different values.
    /// Use this to test `attribute: text` vs `attribute: name` distinctly.
    pub fn leaf_text(
        role: impl Into<String>,
        name: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            inner: Arc::new(MockElementInner {
                role: role.into(),
                name: Some(name.into()),
                text: Some(text.into()),
                automation_id: None,
                alive: Mutex::new(true),
                children: Mutex::new(vec![]),
                parent: Mutex::new(None),
            }),
        }
    }

    /// Create a parent element with children.
    pub fn parent(
        role: impl Into<String>,
        name: impl Into<String>,
        children: Vec<MockElement>,
    ) -> Self {
        let inner = Arc::new(MockElementInner {
            role: role.into(),
            name: Some(name.into()),
            text: None,
            automation_id: None,
            alive: Mutex::new(true),
            children: Mutex::new(vec![]),
            parent: Mutex::new(None),
        });
        let this = Self { inner };
        this.set_children(children);
        this
    }

    /// Set the automation_id on this element (builder-style).
    pub fn with_automation_id(self, id: impl Into<String>) -> Self {
        // Arc::make_mut would require Clone on inner; instead wrap via a new inner.
        let inner = &*self.inner;
        Self {
            inner: Arc::new(MockElementInner {
                role: inner.role.clone(),
                name: inner.name.clone(),
                text: inner.text.clone(),
                automation_id: Some(id.into()),
                alive: Mutex::new(*inner.alive.lock().unwrap()),
                children: Mutex::new(inner.children.lock().unwrap().clone()),
                parent: Mutex::new(None),
            }),
        }
    }

    /// Mark the element as stale. `name()` will return `None`; interaction
    /// methods and `children()` will return `Err(Platform(...))`.
    pub fn kill(&self) {
        *self.inner.alive.lock().unwrap() = false;
    }

    /// Restore a previously killed element.
    pub fn revive(&self) {
        *self.inner.alive.lock().unwrap() = true;
    }

    pub fn is_alive(&self) -> bool {
        *self.inner.alive.lock().unwrap()
    }

    /// Replace the element's children (simulates UI rebuild).
    pub fn set_children(&self, children: Vec<MockElement>) {
        for child in &children {
            *child.inner.parent.lock().unwrap() = Some(Arc::downgrade(&self.inner));
        }
        *self.inner.children.lock().unwrap() = children;
    }

    fn check_alive(&self) -> Result<(), AutomataError> {
        if self.is_alive() {
            Ok(())
        } else {
            Err(AutomataError::Platform("stale element handle".into()))
        }
    }
}

impl Element for MockElement {
    fn name(&self) -> Option<String> {
        if !self.is_alive() {
            return None;
        }
        self.inner.name.clone()
    }

    fn role(&self) -> String {
        self.inner.role.clone()
    }

    fn automation_id(&self) -> Option<String> {
        self.inner.automation_id.clone()
    }

    fn text(&self) -> Result<String, AutomataError> {
        self.check_alive()?;
        Ok(self
            .inner
            .text
            .clone()
            .or_else(|| self.inner.name.clone())
            .unwrap_or_default())
    }

    fn inner_text(&self) -> Result<String, AutomataError> {
        self.check_alive()?;
        let mut parts: Vec<String> = self.inner.name.clone().into_iter().collect();
        for child in self.inner.children.lock().unwrap().iter() {
            if let Some(n) = child.name() {
                parts.push(n);
            }
        }
        Ok(parts.join("\n"))
    }

    fn is_enabled(&self) -> Result<bool, AutomataError> {
        self.check_alive().map(|_| true)
    }

    fn is_visible(&self) -> Result<bool, AutomataError> {
        self.check_alive().map(|_| true)
    }

    fn process_id(&self) -> Result<u32, AutomataError> {
        self.check_alive().map(|_| 1234)
    }

    fn bounds(&self) -> Result<(i32, i32, i32, i32), AutomataError> {
        self.check_alive().map(|_| (0, 0, 100, 30))
    }

    fn children(&self) -> Result<Vec<Self>, AutomataError> {
        self.check_alive()?;
        Ok(self.inner.children.lock().unwrap().clone())
    }

    fn has_parent(&self) -> bool {
        self.is_alive()
    }

    fn parent(&self) -> Option<Self> {
        let weak = self.inner.parent.lock().unwrap().clone()?;
        Some(Self {
            inner: weak.upgrade()?,
        })
    }

    fn click(&self) -> Result<(), AutomataError> {
        self.check_alive()
    }

    fn double_click(&self) -> Result<(), AutomataError> {
        self.check_alive()
    }

    fn click_at(&self, _x: f64, _y: f64, _kind: ClickType) -> Result<(), AutomataError> {
        self.check_alive()
    }

    fn type_text(&self, _text: &str) -> Result<(), AutomataError> {
        self.check_alive()
    }

    fn press_key(&self, _key: &str) -> Result<(), AutomataError> {
        self.check_alive()
    }

    fn set_value(&self, _value: &str) -> Result<(), AutomataError> {
        self.check_alive()
    }

    fn focus(&self) -> Result<(), AutomataError> {
        self.check_alive()
    }

    fn hover(&self) -> Result<(), AutomataError> {
        self.check_alive()
    }

    fn scroll_into_view(&self) -> Result<(), AutomataError> {
        self.check_alive()
    }

    fn activate_window(&self) -> Result<(), AutomataError> {
        self.check_alive()
    }

    fn minimize_window(&self) -> Result<(), AutomataError> {
        self.check_alive()
    }

    fn close(&self) -> Result<(), AutomataError> {
        self.check_alive()
    }
}

// ── Element-tree YAML parser ──────────────────────────────────────────────────

/// Parse a `MockDesktop` from an element-tree YAML string in the same format
/// produced by the `element-tree` binary and stored under `notes/`.
///
/// The input may be a **single** root node or a **sequence** of root nodes.
/// Fields `x`, `y`, `width`, `height` are accepted but ignored.
/// The optional `text` field maps to the separate text-override value so that
/// `attribute: text` and `attribute: name` can return different strings.
///
/// Panics with a descriptive message on parse errors (test-only code).
///
/// # Example
/// ```rust,no_run
/// let desktop = ui_automata::mock::mock_desktop_from_yaml(r#"
/// role: window
/// name: App
/// children:
///   - role: tool tip
///     name: tip
///     children:
///       - role: text
///         name: "Size: 42 bytes"
///       - role: text
///         name: "Modified: today"
/// "#);
/// ```
pub fn mock_desktop_from_yaml(yaml: &str) -> MockDesktop {
    // Intermediate struct — accepts (and discards) the layout fields.
    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct YamlNode {
        role: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        children: Vec<YamlNode>,
        // layout fields — accepted but ignored
        #[serde(default)]
        x: Option<serde_yaml::Value>,
        #[serde(default)]
        y: Option<serde_yaml::Value>,
        #[serde(default)]
        width: Option<serde_yaml::Value>,
        #[serde(default)]
        height: Option<serde_yaml::Value>,
    }

    fn node_to_element(n: YamlNode) -> MockElement {
        let children: Vec<MockElement> = n.children.into_iter().map(node_to_element).collect();
        let inner = Arc::new(MockElementInner {
            role: n.role,
            name: n.name,
            text: n.text,
            automation_id: None,
            alive: Mutex::new(true),
            children: Mutex::new(vec![]),
            parent: Mutex::new(None),
        });
        let elem = MockElement { inner };
        elem.set_children(children);
        elem
    }

    // Try sequence first, then fall back to single-node.
    let roots: Vec<YamlNode> = serde_yaml::from_str::<Vec<YamlNode>>(yaml).unwrap_or_else(|_| {
        let node: YamlNode =
            serde_yaml::from_str(yaml).expect("mock_desktop_from_yaml: invalid YAML");
        vec![node]
    });

    MockDesktop::new(roots.into_iter().map(node_to_element).collect())
}

// ── MockBrowser ───────────────────────────────────────────────────────────────

/// No-op browser for unit tests.
pub struct MockBrowser;

impl Browser for MockBrowser {
    fn ensure(&self) -> Result<(), AutomataError> {
        Ok(())
    }

    fn open_tab(&self, _url: Option<&str>) -> Result<String, AutomataError> {
        Ok("mock-tab-1".into())
    }

    fn close_tab(&self, _tab_id: &str) -> Result<(), AutomataError> {
        Ok(())
    }

    fn activate_tab(&self, _tab_id: &str) -> Result<(), AutomataError> {
        Ok(())
    }

    fn navigate(&self, _tab_id: &str, _url: &str) -> Result<(), AutomataError> {
        Ok(())
    }

    fn eval(&self, _tab_id: &str, _expr: &str) -> Result<String, AutomataError> {
        Ok("complete".into())
    }

    fn tab_info(&self, _tab_id: &str) -> Result<TabInfo, AutomataError> {
        Ok(TabInfo {
            title: "Mock Tab".into(),
            url: "about:blank".into(),
        })
    }

    fn tabs(&self) -> Result<Vec<(String, TabInfo)>, AutomataError> {
        Ok(vec![])
    }
}

// ── MockDesktop ───────────────────────────────────────────────────────────────

/// A scriptable desktop. Set `windows` and `foreground` before calling methods.
pub struct MockDesktop {
    pub windows: Vec<MockElement>,
    pub foreground: Option<MockElement>,
    pub browser: MockBrowser,
}

impl MockDesktop {
    pub fn new(windows: Vec<MockElement>) -> Self {
        Self {
            windows,
            foreground: None,
            browser: MockBrowser,
        }
    }
}

impl Desktop for MockDesktop {
    type Elem = MockElement;
    type Browser = MockBrowser;

    fn browser(&self) -> &MockBrowser {
        &self.browser
    }

    fn application_windows(&self) -> Result<Vec<MockElement>, AutomataError> {
        Ok(self.windows.clone())
    }

    fn open_application(&self, _exe: &str) -> Result<u32, AutomataError> {
        Ok(0)
    }

    fn foreground_window(&self) -> Option<MockElement> {
        self.foreground.clone()
    }

    fn foreground_hwnd(&self) -> Option<u64> {
        None
    }
}
