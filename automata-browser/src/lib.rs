use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

pub const DEFAULT_PORT: u16 = 9222;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// A tab returned by `GET /json`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BrowserTab {
    pub id: String,
    pub title: String,
    pub url: String,
    #[serde(rename = "webSocketDebuggerUrl", default)]
    pub ws_debugger_url: String,
    #[serde(rename = "type", default)]
    pub tab_type: String,
}

/// CDP client bound to a specific debug port.
///
/// All operations are async and enforced by an internal timeout (default 30 s).
/// CDP WebSocket connections are opened fresh per call — no persistent connection state.
#[derive(Clone)]
pub struct Browser {
    port: u16,
    timeout: Duration,
}

impl Browser {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    fn ws_url(&self, tab_id: &str) -> String {
        format!("ws://127.0.0.1:{}/devtools/page/{tab_id}", self.port)
    }

    /// Ensure Edge is running with a CDP debug port open.
    ///
    /// Starting from `self.port`, scans up to 10 consecutive ports:
    /// - If a port already speaks CDP → reuse it (returns `already_running: true, port`).
    /// - If a port is free → launch Edge on it (returns `already_running: false, port`).
    /// - If a port is occupied by something else → skip and try the next one.
    ///
    /// Returns `(already_running, actual_port)`.
    pub async fn ensure_edge(&self) -> Result<(bool, u16)> {
        self.timed(ensure_edge_inner(self.port)).await
    }

    /// List open tabs.
    pub async fn list_tabs(&self) -> Result<Vec<BrowserTab>> {
        self.timed(list_tabs_inner(self.port)).await
    }

    /// Navigate a tab to a URL (CDP Page.navigate).
    pub async fn navigate(&self, tab_id: &str, url: &str) -> Result<()> {
        self.timed(navigate_inner(&self.ws_url(tab_id), url)).await
    }

    /// Evaluate a JavaScript expression and return the result value.
    pub async fn eval(&self, tab_id: &str, expression: &str) -> Result<serde_json::Value> {
        self.timed(eval_inner(&self.ws_url(tab_id), expression))
            .await
    }

    /// Walk the live DOM and return a pruned JSON tree.
    ///
    /// Only text-bearing nodes (`p`, `a`, `span`, `button`, headings, etc.) and
    /// their structural ancestors survive. Each node carries: `tag`, `id`, `class`
    /// (array), `text` (leaf), `href` (anchors), `children`.
    pub async fn dom_tree(&self, tab_id: &str) -> Result<serde_json::Value> {
        self.timed(dom_tree_inner(&self.ws_url(tab_id))).await
    }

    /// Capture a screenshot of a tab as a base64-encoded WebP image.
    pub async fn screenshot(&self, tab_id: &str) -> Result<String> {
        self.timed(screenshot_inner(&self.ws_url(tab_id))).await
    }

    /// Close a tab by its CDP target ID.
    pub async fn close_tab(&self, tab_id: &str) -> Result<()> {
        self.timed(close_tab_inner(self.port, tab_id)).await
    }

    /// Activate (bring to foreground) a tab by its CDP target ID.
    pub async fn activate_tab(&self, tab_id: &str) -> Result<()> {
        self.timed(activate_tab_inner(self.port, tab_id)).await
    }

    /// Open a new tab, navigate it to `url`, and return the tab ID.
    pub async fn open_tab(&self, url: &str) -> Result<String> {
        self.timed(open_tab_inner(self.port, url)).await
    }

    async fn timed<F, T>(&self, fut: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        tokio::time::timeout(self.timeout, fut)
            .await
            .map_err(|_| anyhow::anyhow!("CDP operation timed out after {:?}", self.timeout))?
    }
}

// ── Internal async implementations ───────────────────────────────────────────

/// Scan up to 100 ports starting from `start`. Returns `(already_running, actual_port)`.
async fn ensure_edge_inner(start: u16) -> Result<(bool, u16)> {
    for port in start..start.saturating_add(100) {
        if probe_browser(port).await {
            return Ok((true, port)); // already running on this port
        }
        if is_port_free(port).await {
            launch_browser_process(port)?;
            let deadline = tokio::time::Instant::now() + DEFAULT_TIMEOUT;
            loop {
                if probe_browser(port).await {
                    return Ok((false, port)); // newly launched
                }
                if tokio::time::Instant::now() >= deadline {
                    bail!("timed out waiting for browser debug port {port}");
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        // port occupied by something else — try next
    }
    bail!(
        "no usable port found in range {start}–{}: all occupied by non-browser processes",
        start.saturating_add(9)
    );
}

/// Returns true only if the port speaks the Chrome DevTools Protocol.
async fn probe_browser(port: u16) -> bool {
    let Ok(resp) = reqwest::get(format!("http://127.0.0.1:{port}/json/version")).await else {
        return false;
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return false;
    };
    json.get("Browser").is_some()
}

/// Returns true if nothing is listening on the port (TCP connect fails).
async fn is_port_free(port: u16) -> bool {
    tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err()
}

async fn list_tabs_inner(port: u16) -> Result<Vec<BrowserTab>> {
    let url = format!("http://127.0.0.1:{port}/json");
    let resp = reqwest::get(&url).await.with_context(|| {
        format!("GET {url} failed — is Edge running with --remote-debugging-port={port}?")
    })?;
    let tabs: Vec<BrowserTab> = resp.json().await.context("failed to parse tab list JSON")?;
    Ok(tabs)
}

async fn navigate_inner(ws_url: &str, url: &str) -> Result<()> {
    let result = cdp_call(ws_url, "Page.navigate", json!({ "url": url })).await?;
    if let Some(err) = result.get("errorText").and_then(|v| v.as_str()) {
        bail!("Page.navigate failed: {err}");
    }
    Ok(())
}

async fn eval_inner(ws_url: &str, expression: &str) -> Result<serde_json::Value> {
    let result = cdp_call(
        ws_url,
        "Runtime.evaluate",
        json!({ "expression": expression, "returnByValue": true }),
    )
    .await?;

    if let Some(exc) = result.get("exceptionDetails") {
        let desc = exc
            .pointer("/exception/description")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown exception");
        bail!("JS exception: {desc}");
    }

    Ok(result
        .pointer("/result/value")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

async fn dom_tree_inner(ws_url: &str) -> Result<serde_json::Value> {
    const JS: &str = r#"
(function() {
    const SKIP = new Set([
        'script','style','svg','noscript','template',
        'head','meta','link','br','hr','img','input',
        'select','textarea','iframe','canvas','video','audio',
    ]);

    const TEXT_TAGS = new Set([
        'p','a','span','button',
        'h1','h2','h3','h4','h5','h6',
        'li','td','th','label',
        'strong','em','b','i','s','u',
        'code','pre','small','sub','sup',
        'blockquote','dt','dd','caption','figcaption','summary',
    ]);

    function walk(el) {
        if (el.nodeType !== 1) return null;
        const tag = el.tagName.toLowerCase();
        if (SKIP.has(tag)) return null;

        try {
            const st = window.getComputedStyle(el);
            if (st.display === 'none' || st.visibility === 'hidden' || st.opacity === '0') return null;
        } catch(_) {}

        const children = [];
        for (const child of el.children) {
            const r = walk(child);
            if (r) children.push(r);
        }

        const node = { tag };
        if (el.id) node.id = el.id;
        const classes = (el.className || '').trim().split(/\s+/).filter(Boolean);
        if (classes.length) node.class = classes;
        if (tag === 'a' && el.href) node.href = el.href;

        if (TEXT_TAGS.has(tag)) {
            const ownText = Array.from(el.childNodes)
                .filter(n => n.nodeType === 3)
                .map(n => n.textContent.replace(/\s+/g, ' ').trim())
                .filter(Boolean)
                .join(' ');

            if (ownText) {
                node.text = ownText;
                if (children.length) node.children = children;
                return node;
            }

            if (children.length) {
                node.children = children;
                return node;
            }

            const full = (el.innerText || '').replace(/\s+/g, ' ').trim();
            if (full) { node.text = full; return node; }
            return null;
        }

        if (children.length) { node.children = children; return node; }
        return null;
    }

    return walk(document.body);
})()
"#;

    let result = cdp_call(
        ws_url,
        "Runtime.evaluate",
        json!({ "expression": JS, "returnByValue": true }),
    )
    .await?;

    if let Some(exc) = result.get("exceptionDetails") {
        let desc = exc
            .pointer("/exception/description")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown exception");
        bail!("JS exception: {desc}");
    }

    Ok(result
        .pointer("/result/value")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

async fn screenshot_inner(ws_url: &str) -> Result<String> {
    let result = cdp_call(
        ws_url,
        "Page.captureScreenshot",
        json!({ "format": "webp", "quality": 95 }),
    )
    .await?;
    result
        .get("data")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned())
        .ok_or_else(|| anyhow::anyhow!("Page.captureScreenshot returned no data field"))
}

async fn open_tab_inner(port: u16, url: &str) -> Result<String> {
    let endpoint = format!("http://127.0.0.1:{port}/json/new?{url}");
    let client = reqwest::Client::new();
    let resp = client
        .put(&endpoint)
        .send()
        .await
        .with_context(|| format!("PUT {endpoint} failed"))?;
    if !resp.status().is_success() {
        bail!("open tab: HTTP {}", resp.status());
    }
    let tab: BrowserTab = resp
        .json()
        .await
        .context("failed to parse new tab response")?;
    Ok(tab.id)
}

async fn close_tab_inner(port: u16, tab_id: &str) -> Result<()> {
    let url = format!("http://127.0.0.1:{port}/json/close/{tab_id}");
    let resp = reqwest::get(&url)
        .await
        .with_context(|| format!("GET {url} failed"))?;
    if !resp.status().is_success() {
        bail!("close tab {tab_id}: HTTP {}", resp.status());
    }
    Ok(())
}

async fn activate_tab_inner(port: u16, tab_id: &str) -> Result<()> {
    let url = format!("http://127.0.0.1:{port}/json/activate/{tab_id}");
    let resp = reqwest::get(&url)
        .await
        .with_context(|| format!("GET {url} failed"))?;
    if !resp.status().is_success() {
        bail!("activate tab {tab_id}: HTTP {}", resp.status());
    }
    Ok(())
}

/// Open a fresh WebSocket, send one CDP command, return the `result` field.
async fn cdp_call(
    ws_url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    let (mut ws, _) = connect_async(ws_url)
        .await
        .with_context(|| format!("WebSocket connect to {ws_url} failed"))?;

    let msg = json!({ "id": 1, "method": method, "params": params });
    ws.send(Message::Text(msg.to_string().into()))
        .await
        .context("failed to send CDP message")?;

    while let Some(frame) = ws.next().await {
        let frame = frame.context("WebSocket read error")?;
        let text = match frame {
            Message::Text(t) => t,
            Message::Close(_) => bail!("WebSocket closed before receiving CDP response"),
            _ => continue,
        };
        let val: serde_json::Value = serde_json::from_str(&text).context("invalid CDP JSON")?;
        if val.get("id").and_then(|v| v.as_u64()) == Some(1) {
            if let Some(err) = val.get("error") {
                bail!("CDP error: {err}");
            }
            return Ok(val
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null));
        }
    }

    bail!("WebSocket closed without a CDP response for id=1");
}

// ── SyncBrowser ───────────────────────────────────────────────────────────────

/// Synchronous wrapper around [`Browser`].
///
/// Owns a single-threaded Tokio runtime so callers need no async context and no
/// direct `tokio` dependency. Each method simply blocks until the async operation
/// completes.
pub struct SyncBrowser {
    inner: Browser,
    rt: tokio::runtime::Runtime,
}

impl SyncBrowser {
    pub fn new(port: u16) -> anyhow::Result<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        Ok(Self {
            inner: Browser::new(port),
            rt,
        })
    }

    pub fn port(&self) -> u16 {
        self.inner.port()
    }

    pub fn set_port(&mut self, port: u16) {
        self.inner = Browser::new(port);
    }

    pub fn ensure_edge(&self) -> anyhow::Result<(bool, u16)> {
        self.rt.block_on(self.inner.ensure_edge())
    }

    pub fn list_tabs(&self) -> anyhow::Result<Vec<BrowserTab>> {
        self.rt.block_on(self.inner.list_tabs())
    }

    pub fn navigate(&self, tab_id: &str, url: &str) -> anyhow::Result<()> {
        self.rt.block_on(self.inner.navigate(tab_id, url))
    }

    pub fn eval(&self, tab_id: &str, expression: &str) -> anyhow::Result<serde_json::Value> {
        self.rt.block_on(self.inner.eval(tab_id, expression))
    }

    pub fn dom_tree(&self, tab_id: &str) -> anyhow::Result<serde_json::Value> {
        self.rt.block_on(self.inner.dom_tree(tab_id))
    }

    pub fn screenshot(&self, tab_id: &str) -> anyhow::Result<String> {
        self.rt.block_on(self.inner.screenshot(tab_id))
    }

    pub fn open_tab(&self, url: &str) -> anyhow::Result<String> {
        self.rt.block_on(self.inner.open_tab(url))
    }

    pub fn close_tab(&self, tab_id: &str) -> anyhow::Result<()> {
        self.rt.block_on(self.inner.close_tab(tab_id))
    }

    pub fn activate_tab(&self, tab_id: &str) -> anyhow::Result<()> {
        self.rt.block_on(self.inner.activate_tab(tab_id))
    }
}

// ── Browser launcher ──────────────────────────────────────────────────────────

/// Spawn a browser process with `--remote-debugging-port=<port>`.
///
/// * **Windows** — tries Edge (`msedge.exe`) at its standard install paths.
/// * **Linux**   — probes `google-chrome`, `google-chrome-stable`, `chromium`,
///                 `chromium-browser` in order.
/// * **macOS**   — tries Google Chrome at its standard install path.
///
/// A dedicated `--user-data-dir` in the system temp directory forces a fully
/// independent process rather than handing off to an already-running instance
/// (which would silently ignore `--remote-debugging-port`).
fn launch_browser_process(port: u16) -> Result<()> {
    use std::process::Command;

    let user_data_dir = std::env::temp_dir().join("cdp-debug-profile");

    #[cfg(target_os = "windows")]
    let candidates: &[&str] = &[
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        "msedge",
    ];

    #[cfg(target_os = "linux")]
    let candidates: &[&str] = &[
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
    ];

    #[cfg(target_os = "macos")]
    let candidates: &[&str] = &[
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "google-chrome",
    ];

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    let candidates: &[&str] = &[];

    let mut disable_features: Vec<&str> = vec!["AutoImportAtFirstRun"];

    #[cfg(target_os = "windows")]
    disable_features.push("msEdgeEnableAutoProfileSwitching");

    let flags: Vec<String> = vec![
        format!("--remote-debugging-port={port}"),
        format!("--user-data-dir={}", user_data_dir.display()),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-sync".into(),
        format!("--disable-features={}", disable_features.join(",")),
    ];

    let mut last_err: Option<(&str, std::io::Error)> = None;
    for &candidate in candidates {
        let result = Command::new(candidate).args(&flags).spawn();
        match result {
            Ok(_) => return Ok(()),
            Err(e) => last_err = Some((candidate, e)),
        }
    }

    match last_err {
        Some((path, err)) => bail!("failed to launch browser ({path}): {err}"),
        None => bail!("no supported browser found for this platform"),
    }
}
