/// Find a top-level window named `dialog_title`, wait up to `timeout` for it to appear,
/// then find a descendant named `button_name` and invoke/click it.
///
/// Returns `true` if the button was successfully actioned.
/// Returns `false` silently if the dialog never appears (e.g. already approved).
#[cfg(target_os = "windows")]
pub fn dismiss_dialog_button(
    dialog_title: &str,
    button_name: &str,
    timeout: std::time::Duration,
) -> bool {
    use uiautomation::UIAutomation;

    use crate::util::{find_named, find_named_timeout, invoke_or_click};

    crate::init_com();
    let auto = match UIAutomation::new_direct() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let root = match auto.get_root_element() {
        Ok(r) => r,
        Err(_) => return false,
    };
    let dialog = match find_named_timeout(&auto, &root, dialog_title, timeout) {
        Some(d) => d,
        None => return false,
    };
    let button = match find_named(&auto, &dialog, button_name) {
        Some(b) => b,
        None => return false,
    };
    invoke_or_click(&button).is_ok()
}

#[cfg(not(target_os = "windows"))]
pub fn dismiss_dialog_button(_: &str, _: &str, _: std::time::Duration) -> bool {
    false
}
