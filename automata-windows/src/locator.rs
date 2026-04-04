use super::{Selector, UIElement, UiError};

/// Scoped element finder: searches within a root element for a selector match.
pub struct Locator {
    root: UIElement,
    selector: Selector,
}

impl Locator {
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn new(root: UIElement, selector: Selector) -> Self {
        Self { root, selector }
    }

    /// Search the subtree of `root` for the first element matching the selector.
    /// Returns an error if no match is found.
    pub fn find(&self) -> Result<UIElement, UiError> {
        find_in(&self.root, &self.selector)
    }
}

fn matches(el: &UIElement, selector: &Selector) -> bool {
    match selector {
        Selector::Role { role, name } => {
            if el.role() != *role {
                return false;
            }
            match name {
                None => true,
                Some(n) => el.name_or_empty() == *n,
            }
        }
    }
}

fn find_in(el: &UIElement, selector: &Selector) -> Result<UIElement, UiError> {
    let children = el
        .children()
        .map_err(|e| UiError::Platform(e.to_string()))?;
    for child in children {
        if matches(&child, selector) {
            return Ok(child);
        }
        if let Ok(found) = find_in(&child, selector) {
            return Ok(found);
        }
    }
    Err(UiError::Internal(format!(
        "Element not found: {selector:?}"
    )))
}
