/// CSS-like selector path for navigating UIA element trees.
///
/// Syntax:
/// ```text
/// [Combinator] Step [Combinator Step]*
///
/// Step        ::= Predicate+ [":nth(" n ")"] [":parent" | ":ancestor(" n ")"]
/// Predicate   ::= "[" attr Op value "]"
/// attr        ::= "role" | "name" | "title"
/// Op          ::= "=" | "~=" | "^=" | "$="
/// Combinator  ::= ">" (immediate child) | ">>" (any descendant)
/// ```
///
/// When a selector starts without a combinator, the first step is matched
/// against the scope root element itself. When it starts with `>>` or `>`,
/// the search begins inside the scope root — useful when the scope anchor IS
/// the container you want to search within.
///
/// Examples:
/// ```text
/// # Root match: the scope element itself is tested against this step.
/// # Typically used when the scope is a desktop window list.
/// Window[title~=Mastercam]
///
/// # Immediate child: find the ToolBar that is a direct child of the scope
/// # root. Does NOT match grandchildren.
/// Window[title~=Mastercam] > ToolBar[name=Mastercam]
///
/// # Any descendant: find the "Open" button anywhere inside the window.
/// Window[title~=Mastercam] >> Button[name=Open]
///
/// # Leading >>: search within the scope root without re-matching it.
/// # Useful when the scope anchor IS the container (e.g. scope = dialog).
/// >> [role=combo box][name='File name:'] >> [role=edit]
///
/// # :nth — select by position (0-indexed) among matched siblings.
/// ToolBar[name=Mastercam] > Group:nth(1)
///
/// # Bare role shorthand: equivalent to [role=Button].
/// Button[name=Open]
///
/// # Predicate-only (no bare role): matches any element named "File name:"
/// # regardless of role — useful when role strings differ across OS versions.
/// [name="File name:"]
///
/// # contains (~=): case-insensitive; window whose title contains "mill".
/// Window[title~=mill]
///
/// # starts-with (^=): case-insensitive; window whose title begins with "processing".
/// Window[title^=Processing]
///
/// # ends-with ($=): case-insensitive; combine with starts-with to match without
/// # quoting special characters — e.g. button named "Don't Save" (apostrophe may be U+2019).
/// >> [role=button][name^=Don][name$=Save]
///
/// # OR values — pipe separates alternatives within one predicate.
/// # Matches a window whose title contains "Mill" OR "Design".
/// [name~=Editor|Designer]
///
///# Full multi-step path: scope=desktop, find window, then toolbar child,
/// # then the third Group inside it (0-indexed).
/// Window[title~=Mastercam] >> ToolBar[name=Mastercam] > Group:nth(2)
///
/// # :parent — navigate to the matched element's parent.
/// # Useful to anchor on a container you can only identify by a child.
/// >> [role=button][name=Performance]:parent
///
/// # :ancestor(n) — navigate n levels up. :parent is :ancestor(1).
/// >> [role=button][name=Performance]:ancestor(2)
///
/// # Mid-selector: find Performance's parent, then select its 9th child.
/// >> [role=button][name=Performance]:parent > *:nth(9)
/// ```
use crate::{AutomataError, Element};

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorPath {
    steps: Vec<PathStep>,
}

impl<'de> serde::Deserialize<'de> for SelectorPath {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        SelectorPath::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathStep {
    combinator: Combinator,
    predicates: Vec<Predicate>,
    nth: Option<usize>, // :nth(n) — 0-indexed among matched siblings
    ascend: usize,      // :parent = 1, :ancestor(n) = n, 0 = no ascension
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Combinator {
    /// The root step — no combinator, matching starts from the given element.
    Root,
    /// `>` — immediate children only.
    Child,
    /// `>>` — any descendant (depth-first).
    Descendant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Predicate {
    attr: Attr,
    op: Op,
    /// One or more alternatives — the predicate passes if **any** value matches.
    /// Written as `[name=Editor|Designer]` in selector syntax.
    values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Attr {
    Role,
    Name,
    Title, // alias for name on Window elements
    AutomationId,
    Url, // Tab anchor matching only — ignored on UIA elements
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Exact,      // =
    Contains,   // ~=
    StartsWith, // ^=
    EndsWith,   // $=
}

// ── Parse ─────────────────────────────────────────────────────────────────────

impl SelectorPath {
    /// Returns true if this is a bare wildcard (`*`) with no predicates.
    pub fn is_wildcard(&self) -> bool {
        matches!(self.steps.as_slice(), [step] if step.predicates.is_empty())
    }

    pub fn parse(input: &str) -> Result<Self, AutomataError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(AutomataError::Internal("empty selector".into()));
        }

        // Split on combinators `>>` then `>` while preserving which combinator
        // preceded each segment.  We walk char-by-char to avoid splitting inside
        // `[...]` brackets.
        let segments = split_segments(input)?;
        if segments.is_empty() {
            return Err(AutomataError::Internal("empty selector".into()));
        }

        let mut steps = Vec::with_capacity(segments.len());
        for (combinator, seg) in segments {
            steps.push(parse_step(combinator, seg)?);
        }

        Ok(SelectorPath { steps })
    }

    // ── Matching ─────────────────────────────────────────────────────────────

    /// Find the first element matching this path, starting from `root`.
    ///
    /// Uses an early-exit strategy: descendant searches stop as soon as the
    /// first match is found, avoiding a full DFS of the entire subtree.
    pub fn find_one<E: Element>(&self, root: &E) -> Option<E> {
        if self.steps.is_empty() {
            return None;
        }
        match_steps(root, &self.steps, Some(1)).into_iter().next()
    }

    /// Find all elements matching this path, starting from `root`.
    pub fn find_all<E: Element>(&self, root: &E) -> Vec<E> {
        if self.steps.is_empty() {
            return vec![];
        }
        match_steps(root, &self.steps, None)
    }

    /// Test whether a single element matches the first step of this path
    /// (useful for single-step selectors used as element filters).
    pub fn matches<E: Element>(&self, element: &E) -> bool {
        match self.steps.first() {
            Some(step) => step_matches(step, element),
            None => false,
        }
    }

    /// Like [`find_one`], but also returns the "step parent" — the element from
    /// which the final selector step was resolved.
    ///
    /// The step parent is stored in the found-cache alongside the element so
    /// that when the element goes stale, a narrow re-search can be attempted
    /// from the step parent before falling back to a full DFS from the anchor
    /// root.  The step parent is `None` for root-step selectors that match the
    /// root element itself.
    pub fn find_one_with_parent<E: Element>(&self, root: &E) -> Option<(E, Option<E>)> {
        if self.steps.is_empty() {
            return None;
        }
        find_first_with_step_parent(root, &self.steps)
    }

    /// Test whether a `TabInfo` matches the predicates in the first step of this selector.
    ///
    /// Used for Tab anchor attach mode — polls `Browser::tabs()` until a matching tab
    /// appears. Recognized attributes: `title` / `name` → tab title, `url` → tab URL.
    /// Other attributes (role, id) always pass, treating them as "don't care".
    pub fn matches_tab_info(&self, title: &str, url: &str) -> bool {
        let Some(step) = self.steps.first() else {
            return false;
        };
        step.predicates.iter().all(|p| {
            let actual = match p.attr {
                Attr::Name | Attr::Title => title,
                Attr::Url => url,
                // Ignore role / automation_id for tab matching.
                Attr::Role | Attr::AutomationId => return true,
            };
            p.values.iter().any(|v| match p.op {
                Op::Exact => actual == v.as_str(),
                Op::Contains => actual
                    .to_ascii_lowercase()
                    .contains(v.to_ascii_lowercase().as_str()),
                Op::StartsWith => actual
                    .to_ascii_lowercase()
                    .starts_with(v.to_ascii_lowercase().as_str()),
                Op::EndsWith => actual
                    .to_ascii_lowercase()
                    .ends_with(v.to_ascii_lowercase().as_str()),
            })
        })
    }

    /// Re-find using only the **last step** of this selector, starting from a
    /// cached step-parent.
    ///
    /// This is the fast-path for stale-element re-resolution: instead of a
    /// full DFS from the anchor root, we search only within the step-parent's
    /// immediate children or subtree (depending on the final combinator).
    ///
    /// Returns `None` if the element is no longer present under `step_parent`.
    pub fn find_one_from_step_parent<E: Element>(&self, step_parent: &E) -> Option<E> {
        let step = self.steps.last()?;
        match &step.combinator {
            Combinator::Root | Combinator::Child => {
                let children = step_parent.children().ok()?;
                apply_nth(
                    children
                        .into_iter()
                        .filter(|c| step_matches(step, c))
                        .collect(),
                    step.nth,
                )
                .into_iter()
                .next()
                .and_then(|el| {
                    if step.ascend > 0 {
                        ascend_n(el, step.ascend)
                    } else {
                        Some(el)
                    }
                })
            }
            Combinator::Descendant => {
                let mut acc = vec![];
                let limit = if step.nth.is_none() { Some(1) } else { None };
                collect_descendants(step_parent, step, &mut acc, limit);
                apply_nth(acc, step.nth).into_iter().next().and_then(|el| {
                    if step.ascend > 0 {
                        ascend_n(el, step.ascend)
                    } else {
                        Some(el)
                    }
                })
            }
        }
    }
}

// ── Internal matching logic ───────────────────────────────────────────────────

/// Walk up n levels from `el`, returning the ancestor or `None` if the root
/// is reached before `n` steps.
fn ascend_n<E: Element>(el: E, n: usize) -> Option<E> {
    let mut cur = el;
    for _ in 0..n {
        cur = cur.parent()?;
    }
    Some(cur)
}

/// Recursively resolve steps against a candidate pool.
/// `steps[0]` describes how to find candidates relative to `origin`;
/// remaining steps are applied to each candidate's subtree.
///
/// `limit` caps how many total results are collected. `find_one` passes
/// `Some(1)`; `find_all` passes `None` (unlimited).
fn match_steps<E: Element>(origin: &E, steps: &[PathStep], limit: Option<usize>) -> Vec<E> {
    let step = &steps[0];
    let rest = &steps[1..];

    let candidates: Vec<E> = match &step.combinator {
        Combinator::Root => {
            // The root step matches the origin itself.
            if step_matches(step, origin) {
                vec![origin.clone()]
            } else {
                vec![]
            }
        }
        Combinator::Child => {
            // Immediate children of the last matched element.
            match origin.children() {
                Ok(children) => children
                    .into_iter()
                    .filter(|c| step_matches(step, c))
                    .collect(),
                Err(e) => {
                    log::debug!(
                        "selector: children() failed on '{}' ({}): {e}",
                        origin.name().unwrap_or_default(),
                        origin.role()
                    );
                    vec![]
                }
            }
        }
        Combinator::Descendant => {
            // All descendants (depth-first).
            let mut acc = vec![];
            // Apply early-exit only when this is the last step and no :nth
            // filter is present. :nth needs the full candidate list to pick
            // the correct sibling; multi-step limits are handled in the
            // flat_map below.
            let step_limit = if rest.is_empty() && step.nth.is_none() {
                limit
            } else {
                None
            };
            collect_descendants(origin, step, &mut acc, step_limit);
            acc
        }
    };

    // Apply :nth filter across candidates that share the same parent context.
    let candidates = apply_nth(candidates, step.nth);

    // Apply :parent / :ancestor(n) ascension.
    let candidates: Vec<E> = if step.ascend > 0 {
        candidates
            .into_iter()
            .filter_map(|c| ascend_n(c, step.ascend))
            .collect()
    } else {
        candidates
    };

    if rest.is_empty() {
        candidates
    } else {
        // For multi-step paths, stop collecting once the limit is reached.
        let mut results = Vec::new();
        for c in candidates {
            let remaining = limit.map(|l| l.saturating_sub(results.len()));
            results.extend(match_steps(&c, rest, remaining));
            if limit.is_some_and(|l| results.len() >= l) {
                break;
            }
        }
        results
    }
}

fn collect_descendants<E: Element>(
    parent: &E,
    step: &PathStep,
    acc: &mut Vec<E>,
    limit: Option<usize>,
) {
    if limit.is_some_and(|l| acc.len() >= l) {
        return;
    }
    let children = match parent.children() {
        Ok(c) => c,
        Err(e) => {
            log::debug!(
                "selector: children() failed on '{}' ({}): {e}",
                parent.name().unwrap_or_default(),
                parent.role()
            );
            return;
        }
    };
    for child in children {
        if step_matches(step, &child) {
            acc.push(child.clone());
            if limit.is_some_and(|l| acc.len() >= l) {
                return;
            }
        }
        collect_descendants(&child, step, acc, limit);
        if limit.is_some_and(|l| acc.len() >= l) {
            return;
        }
    }
}

/// Walk the subtree rooted at `parent` DFS-style to find the **first**
/// element matching `step`.  Returns both the match and its immediate DFS
/// parent (the element whose `children()` yielded the match) so callers can
/// store the parent for narrow stale-re-resolution.
fn collect_first_with_parent<E: Element>(parent: &E, step: &PathStep) -> Option<(E, E)> {
    let children = match parent.children() {
        Ok(c) => c,
        Err(_) => return None,
    };
    for child in children {
        if step_matches(step, &child) {
            return Some((child, parent.clone()));
        }
        if let Some(found) = collect_first_with_parent(&child, step) {
            return Some(found);
        }
    }
    None
}

/// Recursive implementation for [`SelectorPath::find_one_with_parent`].
fn find_first_with_step_parent<E: Element>(
    origin: &E,
    steps: &[PathStep],
) -> Option<(E, Option<E>)> {
    let step = &steps[0];
    let rest = &steps[1..];

    if rest.is_empty() {
        // Final step — track the step parent alongside the result.
        return match &step.combinator {
            Combinator::Root => {
                if step_matches(step, origin) {
                    let el = origin.clone();
                    if step.ascend > 0 {
                        ascend_n(el, step.ascend).map(|a| (a, None))
                    } else {
                        Some((el, None))
                    }
                } else {
                    None
                }
            }
            Combinator::Child => {
                let children = origin.children().ok()?;
                let matched = apply_nth(
                    children
                        .into_iter()
                        .filter(|c| step_matches(step, c))
                        .collect(),
                    step.nth,
                )
                .into_iter()
                .next();
                match matched {
                    None => None,
                    Some(el) if step.ascend > 0 => ascend_n(el, step.ascend).map(|a| (a, None)),
                    Some(el) => Some((el, Some(origin.clone()))),
                }
            }
            Combinator::Descendant => {
                if step.nth.is_some() {
                    // :nth requires the full match list; fall back to using the
                    // origin as an approximate step parent (still far better
                    // than the anchor root for narrow re-resolution).
                    let mut acc = vec![];
                    collect_descendants(origin, step, &mut acc, None);
                    apply_nth(acc, step.nth).into_iter().next().and_then(|el| {
                        if step.ascend > 0 {
                            ascend_n(el, step.ascend).map(|a| (a, None))
                        } else {
                            Some((el, Some(origin.clone())))
                        }
                    })
                } else if step.ascend > 0 {
                    collect_first_with_parent(origin, step)
                        .and_then(|(el, _)| ascend_n(el, step.ascend).map(|a| (a, None)))
                } else {
                    collect_first_with_parent(origin, step).map(|(el, parent)| (el, Some(parent)))
                }
            }
        };
    }

    // Not the final step: find candidates for this step (no early-exit needed
    // since we need all of them to recurse into), then descend.
    let candidates: Vec<E> = match &step.combinator {
        Combinator::Root => {
            if step_matches(step, origin) {
                vec![origin.clone()]
            } else {
                vec![]
            }
        }
        Combinator::Child => match origin.children() {
            Ok(c) => c.into_iter().filter(|c| step_matches(step, c)).collect(),
            Err(_) => vec![],
        },
        Combinator::Descendant => {
            let mut acc = vec![];
            collect_descendants(origin, step, &mut acc, None);
            acc
        }
    };
    let candidates = apply_nth(candidates, step.nth);
    let candidates: Vec<E> = if step.ascend > 0 {
        candidates
            .into_iter()
            .filter_map(|c| ascend_n(c, step.ascend))
            .collect()
    } else {
        candidates
    };

    for candidate in candidates {
        if let Some(result) = find_first_with_step_parent(&candidate, rest) {
            return Some(result);
        }
    }
    None
}

fn apply_nth<E>(candidates: Vec<E>, nth: Option<usize>) -> Vec<E> {
    match nth {
        None => candidates,
        Some(n) => candidates.into_iter().nth(n).into_iter().collect(),
    }
}

fn step_matches<E: Element>(step: &PathStep, element: &E) -> bool {
    step.predicates
        .iter()
        .all(|p| predicate_matches(p, element))
}

fn predicate_matches<E: Element>(pred: &Predicate, element: &E) -> bool {
    let actual = match pred.attr {
        Attr::Role => element.role(),
        Attr::Name | Attr::Title => element.name().unwrap_or_default(),
        Attr::AutomationId => element.automation_id().unwrap_or_default(),
        // Url is only meaningful for Tab anchor matching; always returns empty on UIA elements.
        Attr::Url => String::new(),
    };
    pred.values.iter().any(|v| match pred.op {
        Op::Exact => actual == v.as_str(),
        Op::Contains => actual
            .to_ascii_lowercase()
            .contains(v.to_ascii_lowercase().as_str()),
        Op::StartsWith => actual
            .to_ascii_lowercase()
            .starts_with(v.to_ascii_lowercase().as_str()),
        Op::EndsWith => actual
            .to_ascii_lowercase()
            .ends_with(v.to_ascii_lowercase().as_str()),
    })
}

// ── Parser helpers ────────────────────────────────────────────────────────────

/// Split the selector string into `(Combinator, segment_str)` pairs.
/// Respects `[...]` brackets so that `>>` inside an attribute value is not
/// treated as a combinator.
fn split_segments(input: &str) -> Result<Vec<(Combinator, &str)>, AutomataError> {
    let bytes = input.as_bytes();
    let mut segments: Vec<(Combinator, &str)> = vec![];
    let mut depth = 0usize;
    let mut seg_start = 0;
    let mut i = 0;
    // Combinator that the *next* segment should get (set when we consume a `>` or `>>`).
    let mut pending: Option<Combinator> = None;

    while i < bytes.len() {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => depth = depth.saturating_sub(1),
            b'>' if depth == 0 => {
                let seg = input[seg_start..i].trim();
                if !seg.is_empty() {
                    let combinator = pending.take().unwrap_or(if segments.is_empty() {
                        Combinator::Root
                    } else {
                        Combinator::Child
                    });
                    segments.push((combinator, seg));
                }
                // Determine the combinator for the upcoming segment.
                let is_descendant = bytes.get(i + 1) == Some(&b'>');
                pending = Some(if is_descendant {
                    i += 1; // skip second >
                    Combinator::Descendant
                } else {
                    Combinator::Child
                });
                seg_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }

    // Last segment
    let tail = input[seg_start..].trim();
    if !tail.is_empty() {
        let combinator = pending.take().unwrap_or(if segments.is_empty() {
            Combinator::Root
        } else {
            Combinator::Child
        });
        segments.push((combinator, tail));
    }

    if depth != 0 {
        return Err(AutomataError::Internal("unclosed '[' in selector".into()));
    }

    if segments.is_empty() {
        return Err(AutomataError::Internal(
            "selector produced no segments".into(),
        ));
    }

    Ok(segments)
}

fn parse_step(combinator: Combinator, seg: &str) -> Result<PathStep, AutomataError> {
    let seg = seg.trim();

    // Extract pseudo-class suffixes (:nth, :parent, :ancestor) before anything else
    let (seg, nth, ascend) = extract_pseudos(seg)?;

    // Wildcard: `*` or `*:nth(n)` matches any element.
    if seg == "*" {
        return Ok(PathStep {
            combinator,
            predicates: vec![],
            nth,
            ascend,
        });
    }

    // Remaining text is zero or more [attr op value] predicates optionally
    // preceded by a bare role shorthand (e.g. `Button[name=Open]`).
    let (bare_role, rest) = split_bare_role(seg);

    let mut predicates: Vec<Predicate> = vec![];

    if !bare_role.is_empty() {
        predicates.push(Predicate {
            attr: Attr::Role,
            op: Op::Exact,
            values: vec![bare_role.to_string()],
        });
    }

    // Parse bracket predicates
    let mut s = rest.trim();
    while s.starts_with('[') {
        let close = s.find(']').ok_or_else(|| {
            AutomataError::Internal(format!("unclosed '[' in selector segment: {seg}"))
        })?;
        let inner = &s[1..close];
        predicates.push(parse_predicate(inner)?);
        s = s[close + 1..].trim();
    }

    if predicates.is_empty() {
        return Err(AutomataError::Internal(format!(
            "selector step has no predicates: '{seg}'"
        )));
    }

    Ok(PathStep {
        combinator,
        predicates,
        nth,
        ascend,
    })
}

/// Extract trailing pseudo-class suffixes `:nth(n)`, `:parent`, `:ancestor(n)`
/// from a step segment. Returns `(remainder, nth, ascend)`.
/// Pseudos are stripped right-to-left so any order is accepted.
fn extract_pseudos(seg: &str) -> Result<(&str, Option<usize>, usize), AutomataError> {
    let mut s = seg;
    let mut nth: Option<usize> = None;
    let mut ascend: usize = 0;

    loop {
        let t = s.trim_end();
        if t.ends_with(":parent") {
            s = &t[..t.len() - ":parent".len()];
            ascend = 1;
            continue;
        }
        if t.ends_with(')') {
            if let Some(open) = t.rfind('(') {
                let before = &t[..open];
                let inner = &t[open + 1..t.len() - 1];
                if before.ends_with(":ancestor") {
                    let n = inner.trim().parse::<usize>().map_err(|_| {
                        AutomataError::Internal(format!("invalid :ancestor index in '{seg}'"))
                    })?;
                    s = &before[..before.len() - ":ancestor".len()];
                    ascend = n;
                    continue;
                }
                if before.ends_with(":nth") {
                    let n = inner.trim().parse::<usize>().map_err(|_| {
                        AutomataError::Internal(format!("invalid :nth index in '{seg}'"))
                    })?;
                    s = &before[..before.len() - ":nth".len()];
                    nth = Some(n);
                    continue;
                }
            }
        }
        break;
    }

    Ok((s, nth, ascend))
}

/// Split a segment like `Button[name=Open]` into `("Button", "[name=Open]")`.
/// If the segment starts with `[`, the bare role is empty.
fn split_bare_role(seg: &str) -> (&str, &str) {
    if let Some(pos) = seg.find('[') {
        (&seg[..pos], &seg[pos..])
    } else {
        (seg, "")
    }
}

/// Parse a single `attr op value` string (contents of `[...]`).
fn parse_predicate(inner: &str) -> Result<Predicate, AutomataError> {
    // Handle comma-separated multi-predicate shorthand inside one bracket:
    // e.g. `role=Button, name=Open` — split on `,` and parse first only here;
    // the caller handles the multi-predicate case by calling us per-bracket.
    // For simplicity we only parse ONE predicate per bracket call.
    let inner = inner.trim();

    let (attr_str, op, value) = if let Some(pos) = inner.find("~=") {
        (&inner[..pos], Op::Contains, inner[pos + 2..].trim())
    } else if let Some(pos) = inner.find("^=") {
        (&inner[..pos], Op::StartsWith, inner[pos + 2..].trim())
    } else if let Some(pos) = inner.find("$=") {
        (&inner[..pos], Op::EndsWith, inner[pos + 2..].trim())
    } else if let Some(pos) = inner.find('=') {
        (&inner[..pos], Op::Exact, inner[pos + 1..].trim())
    } else {
        return Err(AutomataError::Internal(format!(
            "no operator found in predicate: '{inner}'"
        )));
    };

    let attr = match attr_str.trim() {
        "role" => Attr::Role,
        "name" => Attr::Name,
        "title" => Attr::Title,
        "id" | "automation_id" => Attr::AutomationId,
        "url" => Attr::Url,
        other => {
            return Err(AutomataError::Internal(format!(
                "unknown attribute '{other}' in selector"
            )));
        }
    };

    // Split on `|` for OR semantics: `[name=Editor|Designer]` matches either.
    // Quotes are stripped from each alternative individually.
    let values: Vec<String> = value
        .split('|')
        .map(|v| v.trim().trim_matches(|c| c == '\'' || c == '"').to_string())
        .collect();

    Ok(Predicate { attr, op, values })
}

// ── Display ───────────────────────────────────────────────────────────────────

impl std::fmt::Display for SelectorPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, step) in self.steps.iter().enumerate() {
            if i > 0 {
                match step.combinator {
                    Combinator::Child => write!(f, " > ")?,
                    Combinator::Descendant => write!(f, " >> ")?,
                    Combinator::Root => {}
                }
            }
            if step.predicates.is_empty() {
                write!(f, "*")?;
            }
            for pred in &step.predicates {
                let op = match pred.op {
                    Op::Exact => "=",
                    Op::Contains => "~=",
                    Op::StartsWith => "^=",
                    Op::EndsWith => "$=",
                };
                let attr = match pred.attr {
                    Attr::Role => "role",
                    Attr::Name | Attr::Title => "name",
                    Attr::AutomationId => "id",
                    Attr::Url => "url",
                };
                write!(f, "[{attr}{op}{}]", pred.values.join("|"))?;
            }
            if let Some(n) = step.nth {
                write!(f, ":nth({n})")?;
            }
            match step.ascend {
                0 => {}
                1 => write!(f, ":parent")?,
                n => write!(f, ":ancestor({n})")?,
            }
        }
        Ok(())
    }
}
