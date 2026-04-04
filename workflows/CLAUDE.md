# Workflow Authoring Guide

> **Engine source code:** https://github.com/visioncortex/ui-automata/
> Read the source when you need to understand exact action semantics, condition evaluation,
> or recovery handler behaviour beyond what this guide covers.

## Basic Concepts

A workflow is a YAML file with these top-level keys:

- **`launch`** — optional; spawns an exe before the phases run (`wait: new_pid` or `new_window`)
- **`params`** — named inputs with optional defaults; referenced as `{param.name}`
- **`outputs`** — named values the workflow publishes; written via `Eval … output: name`
- **`anchors`** — named UI element handles (see below)
- **`phases`** — ordered list of work units; each has `steps` (or a `subflow` or `flow_control`)
- **`defaults`** — fallback `timeout` and `action_snapshot` for all steps

**Anchors** are handles to UI elements:
- `Root` — a top-level window; mounted once, lives for the whole workflow
- `Stable` — resolved once per phase mount, cached until unmounted
- `Ephemeral` — re-evaluated on every step (for elements that may be replaced)

**Steps** each have:
- `intent` — human-readable description (logged, helps debugging)
- `action` — what to do (`Click`, `TypeText`, `SetValue`, `Extract`, `Eval`, `Exec`, `PressKey`, `Invoke`, `ActivateWindow`, `MoveFile`, `WriteOutput`, …)
- `expect` — condition that must hold after the action (`ElementFound`, `ElementHasText`, `DialogPresent`, `DialogAbsent`, `WindowClosed`, `ExecSucceeded`, …)
- `timeout` — overrides the phase/workflow default

**Variable substitution** works in most string fields:
- `{param.name}` — input parameter
- `{output.name}` — value written by a previous `Eval`/`Extract`
- `{env.VAR}` — environment variable
- `{workflow.dir}` — directory containing the workflow file (useful for sibling file paths)

**Flow control** phases use `flow_control: { condition: …, go_to: phase_name }` instead of steps — used for loops and conditional branches. The `condition` expression supports `==`, `!=`, `<`, `>`, `<=`, `>=`, `&&`, `||`, `!`, string literals, and variable substitution (e.g. `{output.count} != "0"`).

**Subflows** inline another workflow as a phase: `subflow: other.yml` with optional `params:`.

## Quick Start

A minimal workflow that opens Notepad and types text:

```yaml
name: hello_notepad
defaults:
  timeout: 5s
launch:
  exe: notepad.exe
  wait: new_pid
anchors:
  notepad:
    type: Root
    selector: "[name~=Notepad]"
  editor:
    type: Stable
    parent: notepad
    selector: ">> [role=edit][name='Text Editor']"
phases:
  - name: type_text
    mount: [notepad, editor]
    steps:
      - intent: type hello
        action:
          type: TypeText
          scope: editor
          selector: "[role=edit][name='Text Editor']"
          text: "Hello World"
        expect:
          type: ElementHasText
          scope: editor
          selector: "[role=edit][name='Text Editor']"
          pattern:
            contains: "Hello World"
```

**To run:** use the `workflow` MCP tool with `file: "win10/notepad/notepad_demo.yml"`, or pass YAML inline.

**To discover element IDs:** call `desktop element_tree` or `desktop find_elements` on the live window before writing selectors. Never guess automation IDs. You can also use `vision window_layout` for a quick OCR-based overview.

**To browse the library:** call `resources list` to see all available files, then `resources read` with the full path (e.g. `win10/notepad/notepad_demo.yml`) to read one.

**Reference examples** — use `resources read` to fetch these:
- `win10/notepad/notepad_demo.yml` — launch, type, menus, dialogs, save, close
- `win10/notepad/notepad_extract_value.yml` — `Extract` + `{output.x}` substitution
- `win10/notepad/notepad_loop_counter.yml` — `Eval`, `flow_control`, loops, outputs
- `explorer/explorer_navigate.yml` — `ActivateWindow`, address bar navigation
- `mastercam/process_file.yml` — subflows, `Exec`, `MoveFile`, multi-phase pipeline

## Best Practices

## 1. Never rely on `Sleep` — always use `expect:` with a timeout

`Sleep` is a fixed wait that makes workflows fragile and slow. Every step already
has a configurable timeout. Use `expect:` to describe the condition the action
should produce, and let the engine poll until it's satisfied or the timeout fires.

```yaml
# BAD
- intent: wait for results
  action: { type: Sleep, duration: 4s }
  expect: { type: Always }

# GOOD
- intent: wait for results to appear
  action: { type: NoOp }
  expect:
    type: ElementFound
    scope: store
    selector: "> [role=card]"
```

Set phase or step-level `timeout:` to a reasonable upper bound (e.g. `10s`).
Add a `recovery:` handler if the wait may need a corrective action on failure.

## 2. Never use `expect: Always` — state what is expected, even when trivial

`expect: Always` means "I don't care what happens." That turns failures into
silent passes and makes bugs invisible. Every action produces a detectable
postcondition — name it.

| Action | Minimal meaningful expect |
|--------|--------------------------|
| `Click` a button | `ElementFound` on the thing that appears next |
| `TypeText` | `ElementHasText` with the typed value |
| `SetValue ""` | `ElementHasText` with `exact: ""` |
| `PressKey Return` | `ElementFound` on the resulting page/element |
| `ActivateWindow` | `ElementFound` on a landmark in that window |
| `CloseWindow` | `ProcessRunning: false` or `ElementFound` elsewhere |

## 3. Prefer immediate-child selector `>` over descendant `>>`

`>>` walks the entire subtree — it is O(n) in tree size and slow on complex UIs.
Use `>` (immediate children) when the target is a direct child of the scope root,
and reserve `>>` only when the depth is genuinely unknown.

```yaml
# SLOW — searches the whole subtree
selector: ">> [id=TextBox]"

# FAST — checks direct children only
selector: ">> [id=SearchBox] > [id=TextBox]"
```

When the element is several levels deep but the path is known, chain selectors:

```yaml
selector: "> [role=pane] > [id=TextBox]"
```

## 4. Use additional anchors to narrow the search scope

A root anchor targets a whole window. If the element you need is inside a known
sub-container, declare a second anchor scoped to that container. All selectors on
that anchor then search a much smaller subtree, making `>` viable even when the
exact depth from the window root is unknown.

```yaml
# BAD — searches the entire window tree for every step
anchors:
  app:
    type: Root
    process: notepad
    selector: "*"

steps:
  - action: { type: Click, scope: app, selector: ">> [id=SearchBox]" }
  - action: { type: TypeText, scope: app, selector: ">> [id=TextBox]", text: "hello" }

# GOOD — lock a stable anchor to the search container, then use > inside it
anchors:
  app:
    type: Root
    process: notepad
    selector: "*"
  search_box:
    type: Stable
    parent: app
    selector: ">> [id=SearchBox]"

steps:
  - action: { type: Click, scope: search_box, selector: "> [id=TextBox]" }
  - action: { type: TypeText, scope: search_box, selector: "> [id=TextBox]", text: "hello" }
```

Anchor types:
- `Root` — resolves to a top-level window; registered once and held for the entire workflow (cannot be unmounted).
- `Stable` — resolves once and caches the element for the lifetime of the phase.
- `Ephemeral` — re-evaluated on every step (use for elements that may be replaced).

## 5. Inspect the element tree before writing steps

Before authoring steps for an unfamiliar page, call `desktop element_tree` or
`desktop find_elements` to discover automation IDs and actual bounds. Do not
guess selector IDs — one inspection call avoids many failed round-trips.

Prefer `[id=AutomationId]` over `[name=...]` predicates: automation IDs are
assigned by developers and stay stable across language locales and minor UI
updates, whereas visible names can change. Combine with `[role=...]` for
extra specificity (e.g. `[role=button][id=OkButton]`).

**Selector operators:**

| Op | Meaning | Case |
|----|---------|------|
| `=`  | exact match | sensitive |
| `~=` | contains | **insensitive** |
| `^=` | starts with | **insensitive** |
| `$=` | ends with | **insensitive** |

Use `~=` for window titles that may vary slightly (`[name~=Notepad]`). Use `^=`/`$=` together to match names with special characters you can't easily quote (e.g. `[name^=Don][name$=ave]` matches "Don't Save").

## 6. Root anchors persist for the entire workflow — declare one per destination

`Root` anchors are **globally shared**: the first `mount:` that lists a Root anchor
registers and resolves it; every subsequent `mount:` that lists the same name is a
no-op. Root anchors also **cannot be unmounted** — they stay live for the whole
workflow and are only re-queried if the cached HWND goes stale (window closed/destroyed).

Because a Root anchor is resolved once (to a specific HWND) and stays locked, it
keeps working even if the window title changes during navigation. However, if you
need to target a window that opens *as a result* of navigation — a separate HWND —
you need a second Root anchor for it:

```yaml
anchors:
  panel:
    type: Root
    process: explorer
    selector: "[name=Control Panel]"          # the original explorer window

  firewall:
    type: Root
    process: explorer
    selector: "[name=Windows Defender Firewall]"  # same HWND, or a new one

phases:
  - name: search
    mount: [panel]
    steps: [...]          # navigates within panel; panel stays valid

  - name: read_status
    mount: [firewall]     # resolves fresh if not already registered
    steps: [...]
```

## 7. Hosted child windows (`role=window`) are invisible to `FindAll`

UIA's `FindAll(TreeScope::Descendants)` traverses into child HWNDs (embedded
windows) and **can** find their leaf elements — but the child-window element
itself (`role=window`) is **not** returned as a match. This means:

- `>> [id=Hub]` returns nothing even though `>> [id=hubProfileText]` (a leaf
  inside Hub) works fine.
- A `Stable` anchor with `selector: ">> [id=Hub]"` will always fail to resolve.
- Do **not** try to scope an anchor to an embedded window element. Scope to the
  Root anchor and use `>>` to reach the leaves directly.

Spot the pattern: if `find_elements` with `include_ancestors: true` shows an
ancestor with `role=window` and a non-null `automation_id`, that element is a
hosted child window and cannot be used as a selector target.

## 8. Off-screen elements have degenerate bounds — use `Invoke` not `Click`

Elements that exist in the UIA tree but are scrolled out of view report a
bounding box of `(0, 0, 1, 1)`. A `Click` on such an element fails because the
click guard rejects coordinate `(0, 0)` as it falls outside the target window.

**Do not** use `ScrollIntoView` for items in WinUI / UWP scrollable lists — it
uses mouse-wheel events which trigger momentum/elastic scroll and the list
snaps back.

**Use `Invoke` instead.** `Invoke` calls UIA's `IInvokePattern::Invoke()`
directly — no bounding rect required, no mouse involved, no scroll side-effects.

```yaml
# BAD — fails if About is off-screen (bounds 0,0,1,1)
- intent: click About
  action: { type: Click, scope: settings, selector: ">> [id=SettingsPageAbout_New]" }

# BAD — elastic scroll snaps back in WinUI lists
- intent: scroll About into view
  action: { type: ScrollIntoView, scope: settings, selector: ">> [id=SettingsPageAbout_New]" }

# GOOD — activates the item via UIA InvokePattern regardless of scroll position
- intent: navigate to About
  action: { type: Invoke, scope: settings, selector: ">> [id=SettingsPageAbout_New]" }
  expect:
    type: ElementFound
    scope: settings
    selector: ">> [name=Device Specifications]"
```
