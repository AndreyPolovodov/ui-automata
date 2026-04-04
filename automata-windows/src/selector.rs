/// How to locate a UI element within a parent.
#[derive(Debug, Clone)]
pub enum Selector {
    /// Match by localized role and optional exact name.
    Role { role: String, name: Option<String> },
}
