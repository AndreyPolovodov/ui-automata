// ── Non-Windows stubs ─────────────────────────────────────────────────────────

/// No-op `Browser` stub for non-Windows platforms.
pub struct NoBrowser;

impl ui_automata::Browser for NoBrowser {
    fn ensure(&self) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn open_tab(&self, _url: Option<&str>) -> Result<String, ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn close_tab(&self, _tab_id: &str) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn activate_tab(&self, _tab_id: &str) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn navigate(&self, _tab_id: &str, _url: &str) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn eval(&self, _tab_id: &str, _expr: &str) -> Result<String, ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn tab_info(&self, _tab_id: &str) -> Result<ui_automata::TabInfo, ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn tabs(&self) -> Result<Vec<(String, ui_automata::TabInfo)>, ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
}

#[derive(Clone)]
pub struct UIElement;

impl ui_automata::Element for UIElement {
    fn name(&self) -> Option<String> {
        None
    }
    fn role(&self) -> String {
        String::new()
    }
    fn text(&self) -> Result<String, ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn inner_text(&self) -> Result<String, ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn is_enabled(&self) -> Result<bool, ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn is_visible(&self) -> Result<bool, ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn process_id(&self) -> Result<u32, ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn bounds(&self) -> Result<(i32, i32, i32, i32), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn children(&self) -> Result<Vec<Self>, ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn parent(&self) -> Option<Self> {
        None
    }
    fn click(&self) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn double_click(&self) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn hover(&self) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn click_at(
        &self,
        _x: f64,
        _y: f64,
        _k: ui_automata::ClickType,
    ) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn type_text(&self, _text: &str) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn press_key(&self, _key: &str) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn set_value(&self, _value: &str) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn focus(&self) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn scroll_into_view(&self) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn activate_window(&self) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn minimize_window(&self) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
    fn close(&self) -> Result<(), ui_automata::AutomataError> {
        Err(ui_automata::AutomataError::Platform("Windows only".into()))
    }
}

use crate::{ClickType, UiError};

impl Default for UIElement {
    fn default() -> Self {
        Self
    }
}

impl UIElement {
    pub fn name(&self) -> Option<String> {
        None
    }
    pub fn name_or_empty(&self) -> String {
        String::new()
    }
    pub fn role(&self) -> String {
        String::new()
    }
    pub fn id(&self) -> Option<String> {
        None
    }
    pub fn id_or_empty(&self) -> String {
        String::new()
    }
    pub fn process_id(&self) -> Result<u32, UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn bounds(&self) -> Result<(i32, i32, i32, i32), UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn is_enabled(&self) -> Result<bool, UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn is_visible(&self) -> Result<bool, UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn focus(&self) -> Result<(), UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn control_type(&self) -> Result<String, UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn children(&self) -> Result<Vec<UIElement>, UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn parent(&self) -> Result<Option<UIElement>, UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn text(&self) -> Result<String, UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn inner_text(&self) -> Result<String, UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn click(&self) -> Result<(), UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn double_click(&self) -> Result<(), UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn click_at_position(
        &self,
        _x_pct: u8,
        _y_pct: u8,
        _kind: ClickType,
    ) -> Result<(), UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn type_text(&self, _text: &str, _use_clipboard: bool) -> Result<(), UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn press_key(&self, _key: &str) -> Result<(), UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn activate_window(&self) -> Result<(), UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn minimize_window(&self) -> Result<(), UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
    pub fn close(&self) -> Result<(), UiError> {
        Err(UiError::Platform("Windows only".into()))
    }
}
