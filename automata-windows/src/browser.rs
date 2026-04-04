/// Windows `Browser` implementation wrapping the `automata-browser` CDP client.
///
/// Uses `automata_browser::SyncBrowser` which owns its own single-threaded Tokio
/// runtime, so no external async context is required.
use std::sync::Mutex;

use ui_automata::{AutomataError, Browser, TabInfo};

pub struct WindowsBrowser {
    inner: Mutex<automata_browser::SyncBrowser>,
}

impl WindowsBrowser {
    pub fn new(port: u16) -> Self {
        let browser = automata_browser::SyncBrowser::new(port)
            .expect("failed to create Tokio runtime for browser");
        Self {
            inner: Mutex::new(browser),
        }
    }

    fn err(e: impl std::fmt::Display) -> AutomataError {
        AutomataError::Platform(e.to_string())
    }
}

impl Browser for WindowsBrowser {
    fn ensure(&self) -> Result<(), AutomataError> {
        let mut inner = self.inner.lock().unwrap();
        let (_, port) = inner.ensure_edge().map_err(Self::err)?;
        inner.set_port(port);
        Ok(())
    }

    fn open_tab(&self, url: Option<&str>) -> Result<String, AutomataError> {
        self.inner
            .lock()
            .unwrap()
            .open_tab(url.unwrap_or("about:blank"))
            .map_err(Self::err)
    }

    fn close_tab(&self, tab_id: &str) -> Result<(), AutomataError> {
        self.inner
            .lock()
            .unwrap()
            .close_tab(tab_id)
            .map_err(Self::err)
    }

    fn activate_tab(&self, tab_id: &str) -> Result<(), AutomataError> {
        self.inner
            .lock()
            .unwrap()
            .activate_tab(tab_id)
            .map_err(Self::err)
    }

    fn navigate(&self, tab_id: &str, url: &str) -> Result<(), AutomataError> {
        self.inner
            .lock()
            .unwrap()
            .navigate(tab_id, url)
            .map_err(Self::err)
    }

    fn eval(&self, tab_id: &str, expr: &str) -> Result<String, AutomataError> {
        let val = self
            .inner
            .lock()
            .unwrap()
            .eval(tab_id, expr)
            .map_err(Self::err)?;
        Ok(match val {
            serde_json::Value::String(s) => s,
            v => v.to_string(),
        })
    }

    fn tab_info(&self, tab_id: &str) -> Result<TabInfo, AutomataError> {
        self.inner
            .lock()
            .unwrap()
            .list_tabs()
            .map_err(Self::err)?
            .into_iter()
            .find(|t| t.id == tab_id)
            .map(|t| TabInfo {
                title: t.title,
                url: t.url,
            })
            .ok_or_else(|| AutomataError::Platform(format!("tab '{tab_id}' not found")))
    }

    fn tabs(&self) -> Result<Vec<(String, TabInfo)>, AutomataError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .list_tabs()
            .map_err(Self::err)?
            .into_iter()
            .filter(|t| t.tab_type == "page")
            .map(|t| {
                (
                    t.id,
                    TabInfo {
                        title: t.title,
                        url: t.url,
                    },
                )
            })
            .collect())
    }
}
