use std::collections::HashMap;
use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AutomataError, Browser, ClickType, Desktop, Element, SelectorPath, ShadowDom, debug::dump_tree,
    output::Output,
};

// ── ExtractAttribute ──────────────────────────────────────────────────────────

/// Which text property to read from each matched element during an `Extract` action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExtractAttribute {
    /// The UIA Name property — the element's accessible label or caption.
    Name,
    /// The ValuePattern text, falling back to the Name property.
    /// Use for edit fields and other value-bearing controls.
    #[default]
    Text,
    /// Direct children's names joined by newlines, excluding the element's own name.
    /// Useful for composite controls like list items or tooltips where the
    /// meaningful text lives in child elements.
    InnerText,
}

// ── Action ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum Action {
    // ── Mouse ─────────────────────────────────────────────────────────────────
    /// Left-click the centre of the element found by `selector` under `scope`.
    Click {
        scope: String,
        selector: SelectorPath,
    },

    /// Double-click the centre of the element found by `selector` under `scope`.
    DoubleClick {
        scope: String,
        selector: SelectorPath,
    },

    /// Move the mouse cursor to the centre of the element without clicking.
    /// Useful for triggering hover menus or tooltips.
    Hover {
        scope: String,
        selector: SelectorPath,
    },

    /// Scroll ancestor containers until the element is within their visible
    /// viewport. Hovers each scrollable ancestor and sends wheel events,
    /// stopping as soon as the element's bounding box is fully visible or
    /// it stops moving (meaning the container doesn't scroll further).
    ScrollIntoView {
        scope: String,
        selector: SelectorPath,
    },

    /// Click at a fractional position within the element's bounding box.
    ClickAt {
        scope: String,
        selector: SelectorPath,
        x_pct: f64,
        y_pct: f64,
        kind: ClickType,
    },

    // ── Keyboard ──────────────────────────────────────────────────────────────
    /// Type `text` into the element found by `selector` under `scope`.
    TypeText {
        scope: String,
        selector: SelectorPath,
        text: String,
    },

    /// Send a key expression (e.g. `"{ENTER}"`, `"{TAB}"`) to the element.
    PressKey {
        scope: String,
        selector: SelectorPath,
        key: String,
    },

    // ── Focus / window ────────────────────────────────────────────────────────
    /// Give keyboard focus to the element found by `selector` under `scope`.
    Focus {
        scope: String,
        selector: SelectorPath,
    },

    /// Activate an element via UIA `IInvokePattern::Invoke()`.
    ///
    /// Unlike `Click`, `Invoke` does not require a valid bounding rect, so it
    /// works on elements that are scrolled out of view (bounds `(0,0,1,1)`).
    /// Also tries `SelectionItemPattern` when `InvokePattern` is unavailable.
    /// Returns an error if neither pattern is supported — does not fall back to
    /// `Click`. Prefer this over `Click` + `ScrollIntoView` for items in
    /// WinUI/UWP scrollable lists where mouse-wheel scrolling causes snap-back.
    Invoke {
        scope: String,
        selector: SelectorPath,
    },
    /// Bring the window for `scope` to the foreground and restore it if minimized.
    ActivateWindow { scope: String },
    /// Minimize the window for `scope`.
    MinimizeWindow { scope: String },
    /// Close the window for `scope` via its close button (sends WM_CLOSE).
    CloseWindow { scope: String },

    // ── Value ─────────────────────────────────────────────────────────────────
    /// Set the value of an edit / combo-box directly via IValuePattern.
    SetValue {
        scope: String,
        selector: SelectorPath,
        value: String,
    },

    /// Set a checkbox or toggle button to a specific state via `ITogglePattern`.
    /// Reads the current toggle state first and calls `Toggle()` only if needed.
    /// Idempotent: safe to call even if the element is already in the desired state.
    SetToggle {
        scope: String,
        selector: SelectorPath,
        /// `true` = checked/on, `false` = unchecked/off.
        state: bool,
    },

    // ── Dialog helpers ────────────────────────────────────────────────────────
    /// Find the first dialog child of `scope` and close it.
    DismissDialog { scope: String },

    /// Click a button named `name` inside the current foreground window.
    ClickForegroundButton { name: String },

    /// Click any element named `name` in the foreground window tree.
    ClickForeground { name: String },

    /// Do nothing. Used as a placeholder action when a step only waits.
    NoOp,

    /// Pause for the given duration before the expect condition is evaluated.
    /// Useful after `Hover` to wait for a tooltip to appear before the next step.
    Sleep {
        #[serde(with = "crate::duration::serde")]
        #[schemars(schema_with = "crate::schema::duration_schema")]
        duration: Duration,
    },

    /// Write all values stored under `key` in the workflow output buffer to a
    /// file. Each value is written as one CSV-quoted line. The file is created
    /// or truncated. `path` supports `{output.*}` substitution.
    WriteOutput { key: String, path: String },

    /// Read text from one or more elements and store the values in the workflow
    /// output buffer under `key`. The value is accessible via `{output.<key>}`
    /// substitution in subsequent steps.
    ///
    /// Extracts a text value from a matched element and stores it under `key`.
    /// Whether the value is propagated to the parent workflow is controlled by
    /// the workflow's `outputs` declaration (set at load time, not in YAML).
    Extract {
        /// Output key. Accessible as `{output.<key>}` in later steps.
        key: String,
        /// Anchor name that provides the search root.
        scope: String,
        /// Selector path. Matches the first element unless `multiple` is true.
        selector: SelectorPath,
        /// Which text property to read from each matched element.
        #[serde(default)]
        attribute: ExtractAttribute,
        /// If true, extract all matching elements. If false (default), extract only the first match.
        #[serde(default)]
        multiple: bool,
        /// If true, store in local scope only — not propagated to parent workflow.
        /// Set automatically from the workflow's `outputs` list; not read from YAML.
        #[serde(skip_deserializing, default)]
        local: bool,
    },

    /// Evaluate a simple expression and store the result under `key`.
    ///
    /// The expression language supports arithmetic (`+`, `-`, `*`, `/`, `%`),
    /// comparison operators (`==`, `<`, `<=`, `>`, `>=`), logical operators
    /// (`&&`, `||`, both requiring `Bool` operands), parenthesised grouping,
    /// single-quoted string literals, numeric literals, variable references
    /// (`local.key` / bare identifier for locals, `param.key` for immutable
    /// workflow params, `output.key` for the output buffer), and built-in
    /// functions (`split_lines`, `round`, `floor`, `ceil`, `min`, `max`,
    /// `trim`, `len`).
    ///
    /// The result is always stored as a **local variable** (overwrite semantics).
    /// Bare identifiers resolve from locals first, falling back to the output buffer.
    Eval {
        /// Key under which the result is stored in local variables.
        key: String,
        /// Expression to evaluate.
        expr: String,
        /// If set, also appends the result to the output buffer under this key.
        #[serde(default)]
        output: Option<String>,
    },

    /// Spawn an external process and wait for it to exit.
    /// Stores the exit code as a string in locals under `__exec_exit_code__`.
    /// Fails if the exit code is non-zero — use `on_failure: continue` to suppress.
    /// Use `ExecSucceeded` as the `expect` condition to detect success without failing the step.
    /// `{output.*}` and `{param.*}` tokens in `command` and `args` are substituted before execution.
    Exec {
        /// Executable path or command name (resolved via PATH).
        command: String,
        /// Arguments to pass to the process.
        #[serde(default)]
        args: Vec<String>,
        /// If set, stdout is captured and each line stored in the output buffer under this key.
        #[serde(default)]
        key: Option<String>,
    },

    /// Move a file from `source` to `destination`.
    /// Fails if the destination already exists.
    /// Creates the destination directory if it does not exist.
    /// `{output.*}` tokens in both fields are substituted before execution.
    MoveFile { source: String, destination: String },

    /// Navigate the browser tab anchored to `scope` to `url`.
    /// Polls `document.readyState` until `"complete"` with a hardcoded 30s deadline
    /// (independent of the step's `timeout:`). `scope` must name a `Tab` anchor.
    BrowserNavigate { scope: String, url: String },

    /// Evaluate a JavaScript expression in the browser tab anchored to `scope`.
    /// Stores the string result in the output buffer under `key`.
    /// `scope` must name a `Tab` anchor.
    BrowserEval {
        scope: String,
        expr: String,
        /// Output key to store the result. If omitted the result is discarded.
        key: Option<String>,
    },
}

impl Action {
    /// Short human-readable description for trace output.
    pub fn describe(&self) -> String {
        match self {
            Action::Click { scope, selector } => format!("Click({scope}:{selector})"),
            Action::DoubleClick { scope, selector } => format!("DoubleClick({scope}:{selector})"),
            Action::Hover { scope, selector } => format!("Hover({scope}:{selector})"),
            Action::ScrollIntoView { scope, selector } => {
                format!("ScrollIntoView({scope}:{selector})")
            }
            Action::ClickAt {
                scope,
                selector,
                x_pct,
                y_pct,
                ..
            } => {
                format!("ClickAt({scope}:{selector} @{x_pct:.2},{y_pct:.2})")
            }
            Action::TypeText {
                scope,
                selector,
                text,
            } => {
                let preview: String = text.chars().take(20).collect();
                format!("TypeText({scope}:{selector} {preview:?})")
            }
            Action::PressKey {
                scope,
                selector,
                key,
            } => {
                format!("PressKey({scope}:{selector} {key:?})")
            }
            Action::Focus { scope, selector } => format!("Focus({scope}:{selector})"),
            Action::Invoke { scope, selector } => format!("Invoke({scope}:{selector})"),
            Action::ActivateWindow { scope } => format!("ActivateWindow({scope})"),
            Action::MinimizeWindow { scope } => format!("MinimizeWindow({scope})"),
            Action::CloseWindow { scope } => format!("CloseWindow({scope})"),
            Action::SetValue {
                scope,
                selector,
                value,
            } => {
                format!("SetValue({scope}:{selector} {value:?})")
            }
            Action::SetToggle { scope, selector, state } => {
                format!("SetToggle({scope}:{selector} → {})", if *state { "on" } else { "off" })
            }
            Action::DismissDialog { scope } => format!("DismissDialog({scope})"),
            Action::ClickForegroundButton { name } => format!("ClickForegroundButton({name:?})"),
            Action::ClickForeground { name } => format!("ClickForeground({name:?})"),
            Action::NoOp => "NoOp".into(),
            Action::Sleep { duration } => format!("Sleep({}ms)", duration.as_millis()),
            Action::WriteOutput { key, path } => format!("WriteOutput({key} → {path})"),
            Action::Eval { key, expr, .. } => format!("Eval({key} = {expr:?})"),
            Action::Extract {
                key,
                scope,
                selector,
                attribute,
                multiple,
                local,
            } => format!(
                "Extract({key}={scope}:{selector} attr={attribute:?} multi={multiple} local={local})"
            ),
            Action::Exec { command, args, key } => {
                let k = key
                    .as_deref()
                    .map(|k| format!(" → {k}"))
                    .unwrap_or_default();
                format!("Exec({command} {}){k}", args.join(" "))
            }
            Action::MoveFile {
                source,
                destination,
            } => {
                format!("MoveFile({source} → {destination})")
            }
            Action::BrowserNavigate { scope, url } => {
                format!("BrowserNavigate({scope} → {url:?})")
            }
            Action::BrowserEval { scope, expr, key } => match key {
                Some(k) => format!("BrowserEval({scope}:{k}={expr:?})"),
                None => format!("BrowserEval({scope}:{expr:?})"),
            },
        }
    }

    pub fn execute<D: Desktop>(
        &self,
        dom: &mut ShadowDom<D>,
        desktop: &D,
        output: &mut Output,
        locals: &mut HashMap<String, String>,
        params: &HashMap<String, String>,
    ) -> Result<(), AutomataError> {
        match self {
            Action::Click { scope, selector } => {
                find_required(dom, desktop, scope, selector)?.click()
            }

            Action::DoubleClick { scope, selector } => {
                find_required(dom, desktop, scope, selector)?.double_click()
            }

            Action::Hover { scope, selector } => {
                find_required(dom, desktop, scope, selector)?.hover()
            }

            Action::ScrollIntoView { scope, selector } => {
                find_required(dom, desktop, scope, selector)?.scroll_into_view()
            }

            Action::ClickAt {
                scope,
                selector,
                x_pct,
                y_pct,
                kind,
            } => find_required(dom, desktop, scope, selector)?.click_at(*x_pct, *y_pct, *kind),

            Action::TypeText {
                scope,
                selector,
                text,
            } => find_required(dom, desktop, scope, selector)?.type_text(text),

            Action::PressKey {
                scope,
                selector,
                key,
            } => find_required(dom, desktop, scope, selector)?.press_key(key),

            Action::Focus { scope, selector } => {
                find_required(dom, desktop, scope, selector)?.focus()
            }

            Action::Invoke { scope, selector } => {
                find_required(dom, desktop, scope, selector)?.invoke()
            }

            Action::ActivateWindow { scope } => dom.get(scope, desktop)?.clone().activate_window(),

            Action::MinimizeWindow { scope } => dom.get(scope, desktop)?.clone().minimize_window(),

            Action::CloseWindow { scope } => dom.get(scope, desktop)?.clone().close(),

            Action::SetValue {
                scope,
                selector,
                value,
            } => find_required(dom, desktop, scope, selector)?.set_value(value),

            Action::SetToggle { scope, selector, state } => {
                let el = find_required(dom, desktop, scope, selector)?;
                match el.toggle_state()? {
                    None => Err(AutomataError::Internal(format!(
                        "SetToggle: element '{}' does not support TogglePattern",
                        selector
                    ))),
                    Some(current) if current == *state => Ok(()), // already in desired state
                    _ => el.toggle(),
                }
            }

            Action::DismissDialog { scope } => {
                let root = dom.get(scope, desktop)?.clone();
                let dialog = root
                    .children()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|c| c.role() == "dialog")
                    .ok_or_else(|| {
                        AutomataError::Internal(format!(
                            "DismissDialog: no dialog child found under '{scope}'"
                        ))
                    })?;
                if dialog.close().is_ok() {
                    return Ok(());
                }
                let button = dialog
                    .children()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|c| c.role() == "button")
                    .ok_or_else(|| {
                        AutomataError::Internal(format!(
                            "DismissDialog: no button found in dialog under '{scope}'"
                        ))
                    })?;
                button.click()
            }

            Action::ClickForegroundButton { name } => click_in_foreground(desktop, name, "button"),

            Action::ClickForeground { name } => click_in_foreground(desktop, name, ""),

            Action::NoOp => Ok(()),

            Action::Sleep { duration } => {
                std::thread::sleep(*duration);
                Ok(())
            }

            Action::WriteOutput { key, path } => {
                use std::io::Write;
                let rows = output.get(key);
                let mut f = std::fs::File::create(path)
                    .map_err(|e| AutomataError::Internal(format!("WriteOutput: {e}")))?;
                for row in rows {
                    let escaped = row.replace('"', "\"\"");
                    writeln!(f, "\"{escaped}\"")
                        .map_err(|e| AutomataError::Internal(format!("WriteOutput: {e}")))?;
                }
                Ok(())
            }

            Action::Extract {
                key,
                scope,
                selector,
                attribute,
                multiple,
                local,
            } => {
                let elements = if *multiple {
                    let root = dom.get(scope, desktop)?.clone();
                    selector.find_all(&root)
                } else {
                    dom.find_descendant(scope, selector, desktop)?
                        .into_iter()
                        .collect()
                };
                if elements.is_empty() {
                    log::warn!("extract[{key}]: no elements matched selector");
                }
                for el in elements {
                    let value = match attribute {
                        ExtractAttribute::Name => el.name().unwrap_or_default(),
                        ExtractAttribute::Text => el.text().unwrap_or_default(),
                        ExtractAttribute::InnerText => el.inner_text().unwrap_or_default(),
                    };
                    log::info!("extract[{key}]: {value:?}");
                    if *local {
                        // Local extracts overwrite; only the last value is kept.
                        locals.insert(key.clone(), value);
                    } else {
                        output.push(key, value);
                    }
                }
                Ok(())
            }

            Action::Exec { command, args, key } => {
                use std::process::{Command, Stdio};
                let mut cmd = Command::new(command);
                cmd.args(args);
                if key.is_some() {
                    cmd.stdout(Stdio::piped());
                }
                let child = cmd.spawn().map_err(|e| {
                    AutomataError::Internal(format!("Exec: failed to spawn '{command}': {e}"))
                })?;
                let result = child
                    .wait_with_output()
                    .map_err(|e| AutomataError::Internal(format!("Exec: wait failed: {e}")))?;
                let exit_code = result.status.code().unwrap_or(-1);
                locals.insert(
                    crate::condition::EXEC_EXIT_CODE_KEY.to_owned(),
                    exit_code.to_string(),
                );
                if !result.status.success() {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    return Err(AutomataError::Internal(format!(
                        "Exec: '{command}' exited with {}: {stderr}",
                        result.status
                    )));
                }
                if let Some(k) = key {
                    for line in String::from_utf8_lossy(&result.stdout).lines() {
                        output.push(k, line.to_string());
                    }
                }
                Ok(())
            }

            Action::MoveFile {
                source,
                destination,
            } => {
                let dest = std::path::Path::new(destination.as_str());
                if dest.exists() {
                    return Err(AutomataError::Internal(format!(
                        "MoveFile: destination already exists: {destination}"
                    )));
                }
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        AutomataError::Internal(format!(
                            "MoveFile: failed to create destination directory: {e}"
                        ))
                    })?;
                }
                std::fs::rename(source, destination)
                    .map_err(|e| AutomataError::Internal(format!("MoveFile: {e}")))?;
                log::info!("move_file: {source} → {destination}");
                Ok(())
            }

            Action::Eval {
                key,
                expr,
                output: out_key,
            } => {
                let value = crate::expression::eval_expr(expr, locals, params, output)
                    .map_err(|e| AutomataError::Internal(format!("Eval({key}): {e}")))?
                    .into_string();
                locals.insert(key.clone(), value.clone());
                if let Some(ok) = out_key {
                    log::info!("eval[{key}] = {:?} (output[{ok}])", value);
                    output.push(ok, value);
                } else {
                    log::info!("eval[{key}] = {:?}", value);
                }
                Ok(())
            }

            Action::BrowserNavigate { scope, url } => {
                let tab = dom
                    .tab_handle(scope)
                    .ok_or_else(|| {
                        AutomataError::Internal(format!("'{scope}' is not a mounted Tab anchor"))
                    })?
                    .clone();
                let tab_id = tab.tab_id.clone();
                let browser = desktop.browser();
                browser
                    .navigate(&tab_id, url)
                    .map_err(|e| AutomataError::Internal(format!("navigate: {e}")))?;
                // Poll until document.readyState === 'complete'.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                loop {
                    let ready = browser
                        .eval(&tab_id, "document.readyState")
                        .unwrap_or_default();
                    if ready == "complete" {
                        break;
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(AutomataError::Internal(format!(
                            "BrowserNavigate({scope}): timed out waiting for readyState=complete"
                        )));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                // Press Escape on the browser window to dismiss the address bar and
                // return UIA focus to the page content. Edge steals focus into the
                // address bar during navigation; without this, RootWebArea children
                // are hidden from UIA.
                if let Ok(elem) = dom.get(&tab.parent_browser, desktop) {
                    if let Err(e) = elem.press_key("{ESCAPE}") {
                        log::warn!("BrowserNavigate({scope}): press Escape failed: {e}");
                    }
                }
                Ok(())
            }

            Action::BrowserEval { scope, expr, key } => {
                let tab_id = dom
                    .tab_handle(scope)
                    .ok_or_else(|| {
                        AutomataError::Internal(format!("'{scope}' is not a mounted Tab anchor"))
                    })?
                    .tab_id
                    .clone();
                let result = desktop
                    .browser()
                    .eval(&tab_id, expr)
                    .map_err(|e| AutomataError::Internal(format!("browser eval: {e}")))?;
                if let Some(k) = key {
                    log::info!("browser_eval[{k}] = {result:?}");
                    output.push(k, result);
                }
                Ok(())
            }
        }
    }

    /// Return a clone with all `{output.<key>}` tokens substituted in string fields.
    /// Checks the output buffer first (first value per key), then local vars.
    pub fn apply_output(&self, locals: &HashMap<String, String>, output: &Output) -> Self {
        let sub = |s: &str| sub_output(s, locals, output);
        match self {
            Action::TypeText {
                scope,
                selector,
                text,
            } => Action::TypeText {
                scope: scope.clone(),
                selector: selector.clone(),
                text: sub(text),
            },
            Action::SetValue {
                scope,
                selector,
                value,
            } => Action::SetValue {
                scope: scope.clone(),
                selector: selector.clone(),
                value: sub(value),
            },
            Action::PressKey {
                scope,
                selector,
                key,
            } => Action::PressKey {
                scope: scope.clone(),
                selector: selector.clone(),
                key: sub(key),
            },
            Action::WriteOutput { key, path } => Action::WriteOutput {
                key: key.clone(),
                path: sub(path),
            },
            Action::Exec { command, args, key } => Action::Exec {
                command: sub(command),
                args: args.iter().map(|a| sub(a)).collect(),
                key: key.clone(),
            },
            Action::MoveFile {
                source,
                destination,
            } => Action::MoveFile {
                source: sub(source),
                destination: sub(destination),
            },
            Action::BrowserNavigate { scope, url } => Action::BrowserNavigate {
                scope: scope.clone(),
                url: sub(url),
            },
            Action::BrowserEval { scope, expr, key } => Action::BrowserEval {
                scope: scope.clone(),
                expr: sub(expr),
                key: key.as_deref().map(sub),
            },
            _ => self.clone(),
        }
    }

    /// Set `local` on `Extract` actions based on the workflow's `outputs` declaration.
    /// Keys listed in `outputs` are returned to the parent workflow; all others are local.
    pub(crate) fn apply_outputs(&mut self, outputs: &std::collections::HashSet<String>) {
        let (key, local) = match self {
            Action::Extract { key, local, .. } => (key as &str, local),
            _ => return,
        };
        *local = !outputs.contains(key);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Replace all `{output.<key>}` tokens in `s`.
/// Checks the output buffer first (first value per key), then local vars.
/// Unknown keys expand to an empty string.
pub fn sub_output(s: &str, locals: &HashMap<String, String>, output: &Output) -> String {
    let mut out = s.to_owned();
    for (k, values) in output.as_map() {
        if let Some(v) = values.first() {
            out = out.replace(&format!("{{output.{k}}}"), v);
        }
    }
    for (k, v) in locals {
        out = out.replace(&format!("{{output.{k}}}"), v);
    }
    out
}

fn find_required<D: Desktop>(
    dom: &mut ShadowDom<D>,
    desktop: &D,
    scope: &str,
    selector: &SelectorPath,
) -> Result<D::Elem, AutomataError> {
    match dom.find_descendant(scope, selector, desktop)? {
        Some(el) => Ok(el),
        None => {
            let tree = dom
                .get(scope, desktop)
                .ok()
                .map(|root| dump_tree(root, 3))
                .unwrap_or_default();
            Err(AutomataError::Internal(format!(
                "element not found: selector '{selector}' under scope '{scope}'\n{tree}"
            )))
        }
    }
}

fn click_in_foreground<D: Desktop>(
    desktop: &D,
    name: &str,
    role: &str,
) -> Result<(), AutomataError> {
    let fg = desktop
        .foreground_window()
        .ok_or_else(|| AutomataError::Internal("no foreground window".into()))?;

    let matches = |el: &D::Elem| -> bool {
        let name_ok = el.name().as_deref() == Some(name);
        let role_ok = role.is_empty() || el.role() == role;
        name_ok && role_ok
    };

    let children = fg.children().unwrap_or_default();
    if let Some(el) = children.iter().find(|c| matches(c)) {
        return el.click();
    }

    for child in &children {
        if let Ok(grandchildren) = child.children() {
            if let Some(el) = grandchildren.iter().find(|c| matches(c)) {
                return el.click();
            }
        }
    }

    let all_windows = desktop.application_windows().unwrap_or_default();
    let all_trees: String = all_windows
        .iter()
        .map(|w| {
            let title = w.name().unwrap_or_else(|| "<unnamed>".to_string());
            format!("=== {title} ===\n{}", dump_tree(w, 3))
        })
        .collect::<Vec<_>>()
        .join("\n");
    Err(AutomataError::Internal(format!(
        "element '{name}' not found in foreground window\nAll windows:\n{all_trees}"
    )))
}
