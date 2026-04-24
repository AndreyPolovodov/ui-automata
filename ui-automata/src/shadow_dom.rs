/// Shadow DOM: a cached registry of named live element handles.
///
/// Anchors are declared with a tier and a selector path relative to a named
/// parent (or the desktop root). The registry holds live handles and re-queries
/// them transparently on staleness using a walk-up strategy.
use std::collections::HashMap;

use crate::{
    AutomataError, Browser, Desktop, Element, SelectorPath, TabHandle,
    node_cache::{NodeCache, is_live},
    registry::{AnchorDef, LaunchContext, LaunchWait, Tier},
    snapshot::{SNAP_DEPTH, SnapNode},
};

// ── AnchorName / AnchorMeta ───────────────────────────────────────────────────

/// Newtype key for anchor names. Prevents accidentally mixing anchor-name strings
/// with other `String` values, and enables typed `HashMap` lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AnchorName(String);

impl AnchorName {
    fn new(s: impl Into<String>) -> Self {
        AnchorName(s.into())
    }
}

impl std::borrow::Borrow<str> for AnchorName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// Lock state captured on first resolution of a root-tier anchor.
/// Kept in a single map so HWND and PID always travel together.
#[derive(Debug, Clone)]
struct AnchorMeta {
    /// Exact window handle. Re-resolution fails if this HWND is gone rather
    /// than drifting to a different window of the same process.
    hwnd: Option<u64>,
    /// PID of the owning process. Kept for `anchor_pid()` callers.
    pid: u32,
}

// ── ShadowDom ─────────────────────────────────────────────────────────────────

/// A cached registry of named live element handles. Does not own the desktop;
/// callers pass `&D` to the methods that need to re-query the UIA tree.
pub struct ShadowDom<D: Desktop> {
    /// Declared topology — all registered anchor definitions.
    defs: HashMap<String, AnchorDef>,
    /// Anchor handles and selector find-result cache.
    nodes: NodeCache<D::Elem>,
    /// Last-known tree snapshot per anchor, used by `sync` to detect changes.
    snapshots: HashMap<String, SnapNode>,
    /// Lock state captured on first resolution of each root anchor.
    /// Combines HWND (for drift prevention) and PID (for `anchor_pid()`)
    /// in a single typed map.
    locks: HashMap<AnchorName, AnchorMeta>,
    /// Launch context set after a successful `launch:` wait. Used to filter
    /// the first resolution of root anchors: `new_pid` filters by PID,
    /// `new_window` excludes pre-existing HWNDs, `match_any` applies no extra filter.
    launch_context: Option<LaunchContext>,
    /// Current subflow depth. At depth 0 all anchors use their raw name.
    /// At depth N, Stable/Ephemeral anchors are stored under `":".repeat(N) + name`.
    depth: usize,
    /// Tab anchor handles: keyed by depth-prefixed anchor name.
    tab_handles: HashMap<String, TabHandle>,
}

impl<D: Desktop> ShadowDom<D> {
    pub fn new() -> Self {
        ShadowDom {
            defs: HashMap::new(),
            nodes: NodeCache::new(),
            snapshots: HashMap::new(),
            locks: HashMap::new(),
            launch_context: None,
            depth: 0,
            tab_handles: HashMap::new(),
        }
    }

    /// Set the current subflow depth. At depth 0, all anchors use their raw name.
    /// At depth N, Stable/Ephemeral anchors are stored under `":".repeat(N) + name`.
    pub fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    /// Get the current subflow depth.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Return the depth-prefixed version of a name.
    fn prefixed(&self, name: &str) -> String {
        ":".repeat(self.depth) + name
    }

    /// Compute the effective storage key for `raw_name` at the current depth.
    ///
    /// At depth 0, returns `raw_name` unchanged.
    /// At depth N, tries the prefixed key first; falls back to `raw_name` if the
    /// prefixed key is not registered (allows child workflows to reference Root/Session
    /// anchors inherited from the parent).
    fn effective_key(&self, raw_name: &str) -> String {
        if self.depth == 0 {
            return raw_name.to_string();
        }
        let prefixed = self.prefixed(raw_name);
        if self.defs.contains_key(&prefixed) || self.nodes.element(&prefixed).is_some() {
            prefixed
        } else {
            raw_name.to_string()
        }
    }

    /// Store the launch context so root-anchor first-resolutions use it for filtering.
    pub fn set_launch_context(&mut self, ctx: LaunchContext) {
        self.launch_context = Some(ctx);
    }

    /// Register anchor definitions and immediately resolve Root-tier anchors.
    pub fn mount(&mut self, anchors: Vec<AnchorDef>, desktop: &D) -> Result<(), AutomataError> {
        let mut new_root_keys: Vec<String> = Vec::new();
        let mut new_browser_keys: Vec<String> = Vec::new();
        let mut new_tab_keys: Vec<String> = Vec::new();

        for mut def in anchors {
            let key = match def.tier {
                Tier::Root | Tier::Session | Tier::Browser => {
                    // Globally shared: if already registered under the raw name, skip (parent wins).
                    if self.defs.contains_key(&def.name) {
                        continue;
                    }
                    def.name.clone()
                }
                Tier::Stable | Tier::Ephemeral | Tier::Tab => {
                    // Depth-scoped.
                    self.prefixed(&def.name)
                }
            };
            def.mount_depth = self.depth;
            match def.tier {
                Tier::Root => new_root_keys.push(key.clone()),
                Tier::Browser => new_browser_keys.push(key.clone()),
                Tier::Tab => new_tab_keys.push(key.clone()),
                _ => {}
            }
            self.defs.insert(key, def);
        }

        // Resolve Browser anchors eagerly: call ensure() then find the browser window via UIA.
        for key in new_browser_keys {
            if self.nodes.element(&key).is_none() {
                if let Err(e) = self.resolve(&key, desktop) {
                    self.defs.remove(&key);
                    return Err(e);
                }
            }
        }

        // Resolve newly registered Root anchors eagerly — they represent the
        // top-level application window and must always be present.
        // Non-root anchors (Stable, Ephemeral) are resolved lazily on first use.
        for key in new_root_keys {
            if self.nodes.element(&key).is_none() {
                if let Err(e) = self.resolve(&key, desktop) {
                    // Rollback: remove the def so a subsequent mount() call can
                    // re-register and retry resolution (e.g. when the window is
                    // still appearing after launch with wait: match_any).
                    self.defs.remove(&key);
                    return Err(e);
                }
            }
        }

        // Mount Tab anchors: open or attach to CDP tabs.
        for key in new_tab_keys {
            if !self.tab_handles.contains_key(&key) {
                let def = match self.defs.get(&key) {
                    Some(d) => d.clone(),
                    None => continue,
                };
                match TabHandle::mount(&def, desktop.browser()) {
                    Ok(handle) => {
                        self.tab_handles.insert(key.to_string(), handle);
                    }
                    Err(e) => {
                        self.defs.remove(&key);
                        return Err(e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Insert an element directly into the cache (used for Ephemeral captures).
    pub fn insert(&mut self, name: impl Into<String>, element: D::Elem) {
        let raw_name = name.into();
        let key = self.prefixed(&raw_name);
        let id = self.nodes.insert(key.clone(), element);
        log::debug!("inserted node {id} for anchor '{key}'");
    }

    /// Retrieve a live handle by name. Re-queries if the cached handle is stale.
    ///
    /// Returns `Err` if the anchor cannot be resolved (root gone, selector
    /// not found after walk-up, or name not registered).
    pub fn get(&mut self, name: &str, desktop: &D) -> Result<&D::Elem, AutomataError> {
        let key = self.effective_key(name);

        // Tab anchors have no UIA element of their own; delegate to the parent Browser anchor.
        let parent_redirect = self
            .defs
            .get(&key)
            .filter(|d| d.tier == Tier::Tab)
            .and_then(|d| d.parent.clone());
        if let Some(parent) = parent_redirect {
            return self.get(&parent, desktop);
        }

        let old_id = self.nodes.node_id(&key);
        let cached_live = self.nodes.anchor_is_live(&key);

        if !cached_live {
            if let Some(stale) = self.nodes.remove(&key) {
                log::debug!("invalidated node {} for anchor '{key}'", stale.id);
            }
            self.resolve(&key, desktop)?;
            let new_id = self.nodes.node_id(&key).unwrap_or(0);
            match old_id {
                Some(oid) => {
                    log::debug!("replaced node {oid} with node {new_id} for anchor '{key}'")
                }
                None => log::debug!("mounted node {new_id} for anchor '{key}'"),
            }
        }

        self.nodes.element(&key).ok_or_else(|| {
            AutomataError::Internal(format!("anchor '{name}' not found after resolve"))
        })
    }

    /// Remove a session anchor and all stable anchors that depend on it.
    pub fn invalidate_session(&mut self, session_name: &str) {
        self.locks.remove(session_name);
        if let Some(n) = self.nodes.remove(session_name) {
            log::debug!(
                "invalidated session anchor '{session_name}' (node {})",
                n.id
            );
        }
        self.snapshots.remove(session_name);
        let dependents: Vec<String> = self
            .defs
            .values()
            .filter(|d| self.depends_on(d, session_name))
            .map(|d| d.name.clone())
            .collect();
        for dep in dependents {
            self.locks.remove(dep.as_str());
            if let Some(n) = self.nodes.remove(&dep) {
                log::debug!(
                    "invalidated dependent anchor '{dep}' (node {}) due to session '{session_name}'",
                    n.id
                );
            }
            self.snapshots.remove(&dep);
        }
    }

    /// Unmount anchors by name, removing their definition, cached handle, and
    /// snapshot. The symmetric inverse of [`mount`](Self::mount): after this
    /// call the names are completely unknown to the registry.
    ///
    /// Root and Session anchors are globally shared and cannot be unmounted
    /// this way; a warning is logged and the name is skipped.
    ///
    /// Any Tab anchor whose tab was opened by the workflow (`created=true`) is
    /// closed via `desktop.browser().close_tab()` before its handle is removed.
    pub fn unmount(&mut self, names: &[&str], desktop: &D) {
        for name in names {
            // Guard: Root/Session/Browser anchors are globally shared — refuse to unmount.
            if let Some(def) = self.defs.get(*name) {
                if matches!(def.tier, Tier::Root | Tier::Session | Tier::Browser) {
                    log::debug!("unmount: ignoring Root/Session/Browser anchor '{name}'");
                    continue;
                }
            }
            let key = self.prefixed(name);
            if let Some(handle) = self.tab_handles.remove(&key) {
                handle.close_if_created(desktop.browser());
                log::debug!("unmount: removed tab handle for '{name}'");
            }
            self.defs.remove(&key);
            self.locks.remove(key.as_str());
            if let Some(n) = self.nodes.remove(&key) {
                log::debug!("unmounted node {} for anchor '{name}'", n.id);
            }
            self.snapshots.remove(&key);
        }
    }

    /// Check whether a handle is currently cached and live (no re-query).
    pub fn is_live(&self, name: &str) -> bool {
        self.nodes.anchor_is_live(name)
    }

    /// Find a descendant element matching `selector` within `scope`, using a
    /// stale-first strategy with partial-tree re-resolution:
    ///
    /// 1. **Cache hit, live** → return immediately (1 COM call, no DFS).
    /// 2. **Cache hit, stale, step-parent live** → re-run the selector's last
    ///    step from the cached step-parent (narrow search — O(subtree) not
    ///    O(whole tree)).  Update the cache on success; fall through on failure.
    /// 3. **Cache hit, stale, step-parent also stale** → clear cache entry,
    ///    fall through to full DFS.
    /// 4. **Cache miss** → full `find_one` traversal from the anchor root;
    ///    cache both the result and its step-parent for next time.
    ///
    /// The cache is cleared whenever the scope anchor is re-resolved or unmounted.
    pub fn find_descendant(
        &mut self,
        scope: &str,
        selector: &SelectorPath,
        desktop: &D,
    ) -> Result<Option<D::Elem>, AutomataError> {
        let key = self.effective_key(scope);

        // Tab scope: delegate to the parent Browser anchor with a document-scoped selector.
        let tab_handle = self
            .defs
            .get(&key)
            .filter(|d| d.tier == Tier::Tab)
            .and_then(|_| self.tab_handles.get(&key).cloned());
        if let Some(handle) = tab_handle {
            let (parent_browser, doc_sel) = handle.scoped_selector(desktop.browser(), selector)?;
            return self.find_descendant(&parent_browser, &doc_sel, desktop);
        }

        // Hard error: scope was never registered. Abort rather than poll forever.
        if !self.defs.contains_key(&key) {
            return Err(AutomataError::Internal(format!(
                "scope '{scope}' is not mounted"
            )));
        }

        let sel_key = selector.to_string();

        if let Some(cached) = self.nodes.found(&key, &sel_key) {
            // Do not trust the cached element handle directly: WPF automation peers can
            // remain "live" (is_visible().is_ok(), has_parent() true) after the underlying
            // visual element has been detached and replaced by a new peer.  Always
            // re-validate by re-running the last selector step from the cached step-parent.
            // This is slightly more expensive than a direct handle reuse but is O(siblings)
            // rather than O(whole subtree), and guarantees correctness for toggled buttons,
            // dynamic lists, and any other WPF replace-on-state-change patterns.
            if let Some(step_parent) = self.nodes.found_parent(&key, &sel_key) {
                if is_live(&step_parent) {
                    if let Some(el) = selector.find_one_from_step_parent(&step_parent) {
                        // Re-found under the same parent — refresh the cache entry.
                        self.nodes
                            .set_found(&key, sel_key, el.clone(), Some(step_parent));
                        return Ok(Some(el));
                    }
                    // Not under the same parent anymore — element moved or gone.
                    // Clear and fall through to full DFS.
                    self.nodes.remove_found(&key, &sel_key);
                    // Fall through to full DFS in case element moved elsewhere.
                } else {
                    // Step-parent is also stale — do not give up; the element may have
                    // re-appeared under a new parent (WPF dynamic item replacement).
                    self.nodes.remove_found(&key, &sel_key);
                    // Fall through to full DFS.
                }
            } else {
                // No step-parent stored (root-step match): fall back to checking the
                // element handle directly, then full DFS if stale.
                if is_live(&cached) {
                    return Ok(Some(cached));
                }
                self.nodes.remove_found(&key, &sel_key);
                // Fall through to full DFS.
            }
        }

        // Slow path: full traversal from the anchor root.
        let root = match self.get(scope, desktop) {
            Ok(el) => el.clone(),
            Err(_) => return Ok(None),
        };
        let found = selector.find_one_with_parent(&root);
        if let Some((ref el, ref parent)) = found {
            self.nodes
                .set_found(&key, sel_key, el.clone(), parent.clone());
        }
        Ok(found.map(|(el, _)| el))
    }

    // ── Internal resolution ───────────────────────────────────────────────────

    /// Resolve anchor by its storage `key` (which may be prefixed for depth-scoped anchors).
    fn resolve(&mut self, key: &str, desktop: &D) -> Result<(), AutomataError> {
        let def = self
            .defs
            .get(key)
            .ok_or_else(|| AutomataError::Internal(format!("anchor '{key}' is not registered")))?
            .clone();

        // For Browser anchors, call ensure() to start Edge with a CDP debug port before
        // resolving the UIA window handle. This makes ensure() transparent to the resolve path.
        if def.tier == Tier::Browser {
            desktop
                .browser()
                .ensure()
                .map_err(|e| AutomataError::Internal(format!("browser.ensure(): {e}")))?;
        }

        let found: D::Elem = match def.parent.as_deref() {
            None => {
                let windows = desktop.application_windows()?;
                // After first resolution, locks[key].hwnd holds the exact window handle.
                // Re-resolution is constrained to that HWND so the anchor cannot drift to
                // a different window of the same process (same PID but different HWND).
                if let Some(meta) = self.locks.get(key) {
                    let locked_hwnd = meta.hwnd;
                    windows
                        .into_iter()
                        .find(|w| w.hwnd() == locked_hwnd)
                        .ok_or_else(|| {
                            let hwnd_str = locked_hwnd
                                .map(|h| format!("0x{h:X}"))
                                .unwrap_or_else(|| "unknown".into());
                            AutomataError::Internal(format!(
                                "anchor '{key}' window (hwnd={hwnd_str}) is no longer present"
                            ))
                        })?
                } else {
                    // First resolution: filter by user-supplied pid/process/selector,
                    // then additionally by launch_context strategy if present.
                    let effective_pid = def.pid;
                    let proc_filter = def.process_name.as_deref().map(|s| s.to_lowercase());
                    let mut candidates: Vec<_> = windows
                        .into_iter()
                        .filter(|w| {
                            effective_pid
                                .map_or(true, |pid| w.process_id().map_or(false, |p| p == pid))
                        })
                        .filter(|w| {
                            proc_filter.as_deref().map_or(true, |pf| {
                                w.process_name()
                                    .map(|n| n.to_lowercase() == pf)
                                    .unwrap_or(false)
                            })
                        })
                        .filter(|w| {
                            // Apply launch_context filter only when the anchor targets the
                            // same process as the launched exe (or has no process filter at
                            // all). Anchors that explicitly target a different process are
                            // left unaffected so multi-root-anchor workflows work correctly.
                            let ctx_applies = match (&self.launch_context, &proc_filter) {
                                (Some(ctx), Some(pf)) => *pf == ctx.process_name,
                                (Some(_), None) => true,
                                (None, _) => false,
                            };
                            if !ctx_applies {
                                return true;
                            }
                            match &self.launch_context {
                                Some(LaunchContext {
                                    wait: LaunchWait::NewPid,
                                    pid,
                                    ..
                                }) => w.process_id().map_or(false, |p| p == *pid),
                                Some(LaunchContext {
                                    wait: LaunchWait::NewWindow,
                                    pre_hwnds,
                                    ..
                                }) => w.hwnd().map_or(false, |h| !pre_hwnds.contains(&h)),
                                _ => true, // MatchAny or no launch
                            }
                        })
                        .filter(|w| def.selector.matches(w))
                        .collect();

                    // Sort candidates by Z-order (topmost window first) so that when
                    // a process has multiple windows the one currently on top is preferred,
                    // even if none of them are in the foreground.
                    if candidates.len() > 1 {
                        let z_order = desktop.hwnd_z_order();
                        if !z_order.is_empty() {
                            let rank = |hwnd: u64| -> usize {
                                z_order
                                    .iter()
                                    .position(|h| *h == hwnd)
                                    .unwrap_or(usize::MAX)
                            };
                            candidates.sort_by_key(|w| w.hwnd().map_or(usize::MAX, rank));
                        }
                    }

                    candidates.into_iter().next().ok_or_else(|| {
                        AutomataError::Internal(format!(
                            "anchor '{key}' not found in application windows \
                                 (selector: {}, pid: {:?}, process: {:?})",
                            def.selector, effective_pid, def.process_name
                        ))
                    })?
                }
            }
            Some(raw_parent_name) => {
                let raw_parent_name = raw_parent_name.to_string();
                // Resolve parent using get(), which applies effective_key internally.
                let parent = self
                    .get(&raw_parent_name, desktop)
                    .map_err(|_| {
                        AutomataError::Internal(format!(
                            "parent anchor '{raw_parent_name}' unavailable while resolving '{key}'"
                        ))
                    })?
                    .clone();

                def.selector.find_one(&parent).ok_or_else(|| {
                    AutomataError::Internal(format!(
                        "anchor '{key}' not found under '{raw_parent_name}' \
                         (selector: {})",
                        def.selector
                    ))
                })?
            }
        };

        // Browser anchors must own the foreground so keyboard input reaches them.
        if def.tier == Tier::Browser {
            if let Err(e) = found.activate_window() {
                log::warn!("Browser anchor '{key}': activate_window failed: {e}");
            }
        }

        // Lock the anchor to the resolved window. HWND prevents same-PID drift;
        // PID is kept for anchor_pid() callers.
        if let Ok(pid) = found.process_id() {
            self.locks.insert(
                AnchorName::new(key),
                AnchorMeta {
                    hwnd: found.hwnd(),
                    pid,
                },
            );
        }

        // Capture the initial snapshot so the first sync() call has a baseline.
        // Render the trace log from the snapshot — no second COM traversal needed.
        let snap = SnapNode::capture(&found, SNAP_DEPTH);
        log::debug!("resolved anchor '{key}':\n{}", snap.format_tree(0));
        self.snapshots.insert(key.to_string(), snap);
        // NodeCache::insert also clears found-cache for this scope.
        let id = self.nodes.insert(key.to_string(), found);
        log::debug!("cached node {id} for anchor '{key}'");

        Ok(())
    }

    /// Snapshot the live subtree of `name`, diff against the previous snapshot,
    /// emit each change to the tracer, and **return** the change lines.
    ///
    /// The returned `Vec` is empty when nothing changed. Each line has the form:
    /// ```text
    /// dom: <scope>: ADDED [role "name"]
    /// dom: <scope> > [role "name"]: REMOVED [child-role "child"]
    /// dom: <scope>: name "old" → "new"
    /// ```
    ///
    /// Primarily used by the executor's poll loop, but also directly testable.
    pub fn sync_changes(&mut self, name: &str, desktop: &D) -> Vec<String> {
        let key = self.effective_key(name);
        let el = match self.get(name, desktop) {
            Ok(e) => e.clone(),
            Err(_) => return vec![],
        };
        let new_snap = SnapNode::capture(&el, SNAP_DEPTH);
        let mut changes = Vec::new();
        if let Some(old_snap) = self.snapshots.get(&key) {
            old_snap.diff_into(&new_snap, name, &mut changes);
        }
        self.snapshots.insert(key, new_snap);
        for line in &changes {
            log::debug!("{line}");
        }
        changes
    }

    /// Like [`sync_changes`] but discards the return value.
    /// Convenience for the executor's poll loop.
    pub fn sync(&mut self, name: &str, desktop: &D) {
        self.sync_changes(name, desktop);
    }

    /// Returns the effective PID for a registered anchor.
    /// Prefers the locked PID (captured on first resolution) over the statically
    /// declared PID, since the locked value is always the more specific one.
    pub fn anchor_pid(&self, name: &str) -> Option<u32> {
        let key = self.effective_key(name);
        self.locks
            .get(key.as_str())
            .map(|m| m.pid)
            .or_else(|| self.defs.get(&key)?.pid)
    }

    /// Returns the locked HWND for a registered anchor, if one was captured on first resolution.
    pub fn anchor_hwnd(&self, name: &str) -> Option<u64> {
        let key = self.effective_key(name);
        self.locks.get(key.as_str()).and_then(|m| m.hwnd)
    }

    /// Remove and return all tab handles mounted at exactly `depth`.
    fn drain_tabs_at_depth(&mut self, depth: usize) -> Vec<TabHandle> {
        let keys: Vec<String> = self
            .tab_handles
            .iter()
            .filter(|(_, h)| h.depth == depth)
            .map(|(k, _)| k.clone())
            .collect();
        keys.into_iter()
            .filter_map(|k| self.tab_handles.remove(&k))
            .collect()
    }

    /// Return a reference to the tab handle for `name`, if present.
    pub fn tab_handle(&self, name: &str) -> Option<&TabHandle> {
        let key = self.effective_key(name);
        self.tab_handles.get(&key)
    }

    /// Remove all anchors stored at exactly `depth` (keys prefixed with exactly
    /// `":".repeat(depth)` followed by a non-colon character).
    /// Acts as a safety net for anything not explicitly unmounted by the subflow.
    /// Any created Tab anchors at this depth are closed via `desktop.browser().close_tab()`.
    pub fn cleanup_depth(&mut self, depth: usize, desktop: &D) {
        // Close and drain any tab handles at this depth.
        let tabs = self.drain_tabs_at_depth(depth);
        for handle in tabs {
            handle.close_if_created(desktop.browser());
            log::debug!(
                "cleanup depth {depth}: removed tab handle for tab_id='{}'",
                handle.tab_id
            );
        }

        let prefix = ":".repeat(depth);
        // Remove defs introduced at this depth — covers both depth-prefixed
        // stable/ephemeral anchors AND unprefixed root anchors that were
        // registered by a subflow (not inherited from a parent).
        let def_keys: Vec<String> = self
            .defs
            .iter()
            .filter(|(_, def)| def.mount_depth == depth)
            .map(|(k, _)| k.clone())
            .collect();
        // Also sweep orphan node-cache entries (Capture ephemerals without defs)
        // that carry the depth prefix but no def entry.
        let node_keys: Vec<String> = self
            .nodes
            .anchor_names()
            .into_iter()
            .filter(|k| {
                k.starts_with(&prefix)
                    && k.len() > prefix.len()
                    && !k[prefix.len()..].starts_with(':')
            })
            .collect();
        for key in def_keys {
            self.defs.remove(&key);
            self.locks.remove(key.as_str());
            if let Some(n) = self.nodes.remove(&key) {
                log::debug!(
                    "cleanup depth {depth}: removed node {} for key '{key}'",
                    n.id
                );
            }
            self.snapshots.remove(&key);
        }
        // Also remove orphan nodes (Capture ephemerals without defs).
        for key in node_keys {
            if self.nodes.remove(&key).is_some() {
                log::debug!("cleanup depth {depth}: removed orphan node for key '{key}'");
            }
        }
    }

    /// Returns true if `def` is a (transitive) child of `session_name`.
    fn depends_on(&self, def: &AnchorDef, session_name: &str) -> bool {
        let mut current = def.parent.as_deref();
        while let Some(p) = current {
            if p == session_name {
                return true;
            }
            current = self.defs.get(p).and_then(|d| d.parent.as_deref());
        }
        false
    }
}

impl<D: Desktop> Default for ShadowDom<D> {
    fn default() -> Self {
        Self::new()
    }
}
