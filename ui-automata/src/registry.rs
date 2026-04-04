/// Anchor declarations: tiers, definitions, and launch-wait strategies.
use std::collections::HashSet;

use crate::SelectorPath;

// ── LaunchWait / LaunchContext ────────────────────────────────────────────────

/// How to identify the launched application's window after calling `launch:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, schemars::JsonSchema, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchWait {
    /// Wait until the anchor's selector resolves against any window of the process.
    /// Use for apps that reuse an existing process (browsers opening a new tab).
    #[default]
    MatchAny,
    /// Wait for a window owned by the exact PID returned by the OS launcher.
    /// Use for normal multi-instance apps (Notepad, Word).
    NewPid,
    /// Snapshot existing windows before launch; wait for a new HWND to appear in
    /// the process. Use for single-instance apps (Explorer, VS Code) where the
    /// launched process hands off to an existing one and exits.
    NewWindow,
}

/// Context stored after a successful `launch:` wait, used by `ShadowDom::resolve`
/// to filter the first resolution of root anchors.
#[derive(Debug, Clone)]
pub struct LaunchContext {
    pub wait: LaunchWait,
    /// PID returned by `open_application`. Used for `NewPid` filtering.
    pub pid: u32,
    /// HWNDs that existed before `open_application` was called. Used for `NewWindow` filtering.
    pub pre_hwnds: HashSet<u64>,
    /// Lowercase process name derived from the launched exe (without `.exe`).
    /// The launch filter is skipped for root anchors that explicitly target a
    /// different process, so multiple-root-anchor workflows work correctly.
    pub process_name: String,
}

// ── Tier / AnchorDef ─────────────────────────────────────────────────────────

/// Lifetime tier of a registered anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Present for the entire process lifetime. Staleness = fatal error.
    Root,
    /// Can be opened and closed during a workflow. Dependents are invalidated
    /// wholesale when the session window goes away.
    Session,
    /// Stable while its root/session parent is open. Re-queried on stale.
    Stable,
    /// Plan-scoped captures. Released explicitly at plan exit.
    Ephemeral,
    /// CDP browser session. Calls `ensure()` on mount; stored as a root UIA anchor.
    Browser,
    /// CDP tab within a Browser anchor. Stored in the ShadowDom tab-handle map.
    Tab,
}

/// Declaration of a named anchor.
#[derive(Debug, Clone)]
pub struct AnchorDef {
    /// Unique name (used as a key in plans, conditions, actions).
    pub name: String,
    /// Parent anchor to resolve the selector relative to.
    /// `None` means the selector is applied to desktop application windows.
    pub parent: Option<String>,
    /// CSS-like path from the parent to this element.
    pub selector: SelectorPath,
    pub tier: Tier,
    /// Optional PID to pin this anchor to a specific process.
    /// When set, resolution filters application windows by PID before applying
    /// the selector, preventing accidental attachment to a different process.
    pub pid: Option<u32>,
    /// Optional process name filter (case-insensitive, without .exe).
    /// When set, resolution only considers windows whose owning process name
    /// matches this string. Can be used instead of or alongside `pid`.
    pub process_name: Option<String>,
    /// Subflow depth at which this anchor was mounted. Set by `ShadowDom::mount()`
    /// so `cleanup_depth` can remove anchors introduced by a subflow regardless
    /// of their tier (including Root anchors that are not depth-prefixed).
    pub mount_depth: usize,
}

impl AnchorDef {
    pub fn root(name: impl Into<String>, selector: SelectorPath) -> Self {
        AnchorDef {
            name: name.into(),
            parent: None,
            selector,
            tier: Tier::Root,
            pid: None,
            process_name: None,
            mount_depth: 0,
        }
    }

    pub fn session(name: impl Into<String>, selector: SelectorPath) -> Self {
        AnchorDef {
            name: name.into(),
            parent: None,
            selector,
            tier: Tier::Session,
            pid: None,
            process_name: None,
            mount_depth: 0,
        }
    }

    /// Pin this anchor to a specific process. Resolution will only match windows
    /// belonging to that PID, preventing accidental attachment to unrelated
    /// windows with the same title. Can be chained onto any constructor:
    /// `AnchorDef::session("notepad", sel).with_pid(notepad_pid)`
    pub fn with_pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }

    pub fn stable(
        name: impl Into<String>,
        parent: impl Into<String>,
        selector: SelectorPath,
    ) -> Self {
        AnchorDef {
            name: name.into(),
            parent: Some(parent.into()),
            selector,
            tier: Tier::Stable,
            pid: None,
            process_name: None,
            mount_depth: 0,
        }
    }

    pub fn ephemeral(
        name: impl Into<String>,
        parent: impl Into<String>,
        selector: SelectorPath,
    ) -> Self {
        AnchorDef {
            name: name.into(),
            parent: Some(parent.into()),
            selector,
            tier: Tier::Ephemeral,
            pid: None,
            process_name: None,
            mount_depth: 0,
        }
    }
}
