/// Combined cache for anchor element handles and selector find results.
///
/// Owns three maps that share a common invalidation boundary:
///
/// - **`anchors`** — the named element handles resolved by `ShadowDom`
///   (one per registered anchor).
/// - **`children`** — results of previous `find_descendant` calls, keyed by
///   `(scope_name, selector_string)`.
/// - **`parents`** — the "step parent" of each cached child: the element from
///   which the final selector step was resolved.  Used for narrow
///   stale-re-resolution: instead of a full DFS from the anchor root we
///   re-run just the last step from the cached step parent, which is usually
///   an order of magnitude cheaper.
///
/// The three caches are coupled: whenever an anchor handle is inserted or
/// removed the entire children/parents cache for that scope is also cleared,
/// because a new anchor pointer means all previously discovered descendants
/// are potentially stale.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Element;

// ── Node ID counter ───────────────────────────────────────────────────────────

static NEXT_NODE_ID: AtomicU64 = AtomicU64::new(1);

fn next_node_id() -> u64 {
    NEXT_NODE_ID.fetch_add(1, Ordering::Relaxed)
}

// ── Liveness check ────────────────────────────────────────────────────────────

/// An element is live if it is still attached to the UIA tree.
/// Two checks combined — both must pass:
///   1. `is_visible()` (`is_offscreen()`) returns `Err` for destroyed UIA elements.
///   2. `has_parent()` returns `false` for detached elements (dismissed dialogs);
///      root windows always have the desktop as parent so they pass this check.
/// For mocks, `kill()` makes `is_visible()` return `Err` and `has_parent()` false.
pub(crate) fn is_live<E: Element>(el: &E) -> bool {
    el.is_visible().is_ok() && el.has_parent()
}

// ── CachedNode ────────────────────────────────────────────────────────────────

pub(crate) struct CachedNode<E> {
    pub element: E,
    /// Unique ID assigned at insertion. Used only for trace log messages.
    pub id: u64,
}

// ── NodeCache ─────────────────────────────────────────────────────────────────

pub(crate) struct NodeCache<E: Element> {
    /// Named anchor handles, one per registered anchor.
    anchors: HashMap<String, CachedNode<E>>,
    /// Cached results of `find_descendant`, keyed by scope → selector_string.
    /// Cleared automatically on any anchor insert or remove for that scope.
    children: HashMap<String, HashMap<String, E>>,
    /// Step-parent of each cached child, keyed identically to `children`.
    /// `None` when the selector matched the scope root itself (rare).
    parents: HashMap<String, HashMap<String, E>>,
}

impl<E: Element> NodeCache<E> {
    pub fn new() -> Self {
        NodeCache {
            anchors: HashMap::new(),
            children: HashMap::new(),
            parents: HashMap::new(),
        }
    }

    // ── Anchor ops ────────────────────────────────────────────────────────────

    pub fn element(&self, name: &str) -> Option<&E> {
        self.anchors.get(name).map(|n| &n.element)
    }

    pub fn node_id(&self, name: &str) -> Option<u64> {
        self.anchors.get(name).map(|n| n.id)
    }

    pub fn anchor_is_live(&self, name: &str) -> bool {
        self.anchors.get(name).is_some_and(|n| is_live(&n.element))
    }

    /// Insert an anchor element and return the assigned node ID.
    ///
    /// Automatically clears any stale children/parents cache entries for this
    /// scope, since a new anchor pointer means previously discovered
    /// descendants may no longer be valid.
    pub fn insert(&mut self, name: String, element: E) -> u64 {
        let id = next_node_id();
        self.anchors
            .insert(name.clone(), CachedNode { element, id });
        self.children.remove(&name);
        self.parents.remove(&name);
        id
    }

    /// Returns all anchor names currently stored in the cache.
    pub fn anchor_names(&self) -> Vec<String> {
        self.anchors.keys().cloned().collect()
    }

    /// Remove an anchor and return the evicted node (used for log messages).
    ///
    /// Also clears children/parents cache entries for this scope.
    pub fn remove(&mut self, name: &str) -> Option<CachedNode<E>> {
        self.children.remove(name);
        self.parents.remove(name);
        self.anchors.remove(name)
    }

    // ── Found-cache ops ───────────────────────────────────────────────────────

    /// Return a clone of the cached found element for `(scope, sel_key)`, if any.
    pub fn found(&self, scope: &str, sel_key: &str) -> Option<E> {
        self.children.get(scope)?.get(sel_key).cloned()
    }

    /// Return a clone of the cached step-parent for `(scope, sel_key)`, if any.
    pub fn found_parent(&self, scope: &str, sel_key: &str) -> Option<E> {
        self.parents.get(scope)?.get(sel_key).cloned()
    }

    /// Store a found element and its optional step-parent.
    pub fn set_found(&mut self, scope: &str, sel_key: String, el: E, parent: Option<E>) {
        self.children
            .entry(scope.to_string())
            .or_default()
            .insert(sel_key.clone(), el);
        if let Some(p) = parent {
            self.parents
                .entry(scope.to_string())
                .or_default()
                .insert(sel_key, p);
        } else {
            // Remove any stale parent entry so `found_parent` returns None.
            if let Some(m) = self.parents.get_mut(scope) {
                m.remove(&sel_key);
            }
        }
    }

    pub fn remove_found(&mut self, scope: &str, sel_key: &str) {
        if let Some(m) = self.children.get_mut(scope) {
            m.remove(sel_key);
        }
        if let Some(m) = self.parents.get_mut(scope) {
            m.remove(sel_key);
        }
    }
}
