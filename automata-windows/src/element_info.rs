/// Properties of a UI element probed at a point.
#[derive(Debug, Clone)]
pub struct ElementInfo {
    pub name: String,
    pub role: String,
    pub class: String,
    pub automation_id: String,
    pub value: String,
    pub enabled: bool,
    pub focusable: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
