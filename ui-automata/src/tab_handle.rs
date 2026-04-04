use crate::{AutomataError, Browser, SelectorPath, registry::AnchorDef};

/// State for a mounted Tab anchor.
#[derive(Debug, Clone)]
pub struct TabHandle {
    /// CDP target ID for this tab.
    pub tab_id: String,
    /// Name of the parent Browser anchor.
    pub parent_browser: String,
    /// True if this tab was opened by the workflow (closed on unmount).
    /// False if this tab was attached to an existing tab (left open on unmount).
    pub created: bool,
    /// Subflow depth when this tab was mounted (mirrors AnchorDef::mount_depth).
    pub depth: usize,
}

impl TabHandle {
    /// Open or attach to a CDP tab as described by `def`.
    ///
    /// - If `def.url` is set: opens a new tab, navigates to the URL, and waits
    ///   for `document.readyState === 'complete'` (up to 30 s). `created = true`.
    /// - Otherwise: polls `browser.tabs()` until one matches `def.selector`
    ///   (up to 30 s). `created = false` — the tab is left open on unmount.
    pub fn mount(def: &AnchorDef, browser: &impl Browser) -> Result<Self, AutomataError> {
        let parent_browser = def.parent.clone().ok_or_else(|| {
            AutomataError::Internal(format!("Tab anchor '{}' has no parent", def.name))
        })?;

        let (tab_id, created) = if def.selector.is_wildcard() {
            // No selector — open/inherit a blank tab.
            // Navigation is done separately via a BrowserNavigate action.
            let tabs = browser
                .tabs()
                .map_err(|e| AutomataError::Internal(format!("browser.tabs(): {e}")))?;
            let new_tab = tabs.into_iter().find(|(_, info)| {
                info.url == "edge://newtab/" || info.url == "about:blank" || info.url.is_empty()
            });
            if let Some((existing_id, _)) = new_tab {
                // Treat inherited blank/new tabs as created — they exist only
                // for this workflow and should be closed on unmount.
                (existing_id, true)
            } else {
                let tab_id = browser
                    .open_tab(None)
                    .map_err(|e| AutomataError::Internal(format!("open_tab(blank): {e}")))?;
                (tab_id, true)
            }
        } else {
            // Attach mode: poll browser.tabs() until one matches the selector.
            let selector = def.selector.clone();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let tab_id = loop {
                let tabs = browser
                    .tabs()
                    .map_err(|e| AutomataError::Internal(format!("browser.tabs(): {e}")))?;
                if let Some((id, _)) = tabs
                    .into_iter()
                    .find(|(_, info)| selector.matches_tab_info(&info.title, &info.url))
                {
                    break id;
                }
                if std::time::Instant::now() >= deadline {
                    return Err(AutomataError::Internal(format!(
                        "Tab '{}': timed out waiting for tab matching '{selector}'",
                        def.name
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            };
            (tab_id, false)
        };

        // Bring this tab to the foreground so UIA exposes its accessibility tree.
        if let Err(e) = browser.activate_tab(&tab_id) {
            log::warn!("activate_tab('{}') failed: {e}", tab_id);
        }

        log::debug!(
            "mounted tab '{}' (tab_id={tab_id}, created={created})",
            def.name
        );
        Ok(Self {
            tab_id,
            parent_browser,
            created,
            depth: def.mount_depth,
        })
    }

    /// Close this tab via `browser` if the workflow created it; no-op if attached.
    pub fn close_if_created(&self, browser: &impl Browser) {
        if self.created {
            if let Err(e) = browser.close_tab(&self.tab_id) {
                log::warn!("close_tab('{}') failed: {e}", self.tab_id);
            }
        }
    }

    /// Build a document-scoped selector for this tab, verifying it is focused first.
    ///
    /// Checks `document.hasFocus()` via CDP and returns an error if the tab is not
    /// the active foreground tab — prevents silently querying the wrong document.
    ///
    /// Returns `(parent_browser_name, scoped_selector)` so the caller can
    /// delegate `find_descendant` to the parent Browser anchor.
    pub fn scoped_selector(
        &self,
        browser: &impl Browser,
        selector: &SelectorPath,
    ) -> Result<(String, SelectorPath), AutomataError> {
        // Guard: verify this tab is the active (visible) one before building a
        // UIA selector.
        let visibility = browser
            .eval(&self.tab_id, "document.visibilityState")
            .unwrap_or_default();
        if visibility != "visible" {
            return Err(AutomataError::Internal(format!(
                "tab '{}' is not the active tab (visibilityState={:?})",
                self.tab_id, visibility
            )));
        }
        let doc_sel_str = format!(">> [id=RootWebArea] >> {selector}");
        let doc_sel = SelectorPath::parse(&doc_sel_str)
            .map_err(|e| AutomataError::Internal(format!("tab selector build: {e}")))?;
        Ok((self.parent_browser.clone(), doc_sel))
    }
}
