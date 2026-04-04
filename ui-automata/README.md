# UI Automata

Declarative workflow engine for Windows UI automation, built for reliable unattended execution and AI agent use.

<video src="https://github.com/user-attachments/assets/81f0cbb4-e3bf-416f-a373-74ee2cb3bedb" controls width="100%"></video>

## The problem

Windows work that is too interactive for a script and too tedious to do by hand — checking a service status in Event Viewer, extracting a value from a legacy desktop app, configuring a settings dialog with no API — is exactly what AI agents should handle. But every step in a Windows UI workflow can fail in ways a script cannot see:

- **Timing**: a button click completes before the app finishes processing. The UI looks ready; it is not.
- **Transient disabled state**: the element exists and is visible, but temporarily disabled. The click fires, nothing happens, no error is returned.
- **Popup interruptions**: a modal dialog captures focus mid-step. The script waits for an outcome that will never come.
- **Stale handles**: the app rebuilds its UI after a navigation. Cached references point to the wrong place; clicks land silently on the wrong target.
- **Focus loss**: a keypress meant for one field lands on another.

These are not edge cases — they are routine in any real Windows application.

## Why existing tools fall short

| | UI Automata | AutoHotkey | UIPath | Selenium | Vision agents |
|---|---|---|---|---|---|
| Native Windows apps | ✓ | ✓ | ✓ | ✗ | ✓ |
| Structured recovery | ✓ | ✗ | partial | ✗ | ✗ |
| Agent-native | ✓ | ✗ | ✗ | ✗ | ✓ |
| Audit trail | ✓ | ✗ | ✓ | partial | ✗ |
| Execution speed | fast | fast | fast | fast | slow |
| Cost per run | low | low | high | low | high |
| Resolution-stable | ✓ | ✗ | partial | — | ✗ |

AutoHotkey clicks pixel coordinates and `Sleep`s — no observable outcomes, no recovery. UIPath requires every edge case scripted in advance by a specialist. Selenium covers browsers only. pywinauto wraps UIA in Python but is fully imperative. Vision-based agents are slow, expensive per run, and fragile to layout changes.

## The approach

**Every action is an intent, not a command.**

Each step declares an **action**, an **expect** condition, and an optional **recovery** handler. The engine runs the same lifecycle for every step:

1. Execute the action
2. Poll the `expect` condition every 100ms
3. Condition passes → advance; timeout → check recovery handlers, then retry, skip, or fail

```yaml
- intent: click Open button
  action:
    type: Click
    scope: main_window
    selector: ">> [role=button][name=Open]"
  expect:
    type: DialogPresent
    scope: main_window
  timeout: 10s
```

No sleeps. No guessing. No silent failures. Recovery handlers are declared once and fire wherever their trigger condition is met; known failure modes are handled in one place, not scattered through step logic.

- Elements are identified by role and name, not pixel coordinates — selectors survive resolution changes and most app updates.
- Wrong-window interactions are caught at the execution layer before any action fires.
- Every action, condition check, and recovery handler is logged; failures include the full execution trace and a UI tree dump.

## The shadow DOM

Windows UI Automation is a cross-process RPC protocol — every element query is a round-trip to the target process. Walking a nested element path issues one call per level; a 20-step workflow that re-queries handles on every step pays that cost repeatedly.

ui-automata maintains a **shadow DOM**: a cached mirror of the live element tree. Handles are resolved once and reused. When a handle goes stale (the app rebuilt its UI), the engine walks up to the nearest live ancestor, re-queries downward, and continues without failing the step. This is the inverse of React's virtual DOM — instead of pushing changes into a UI, we efficiently read from one we do not control.

**HWND locking**: when a Root anchor is first resolved, the engine records the exact OS window handle. All subsequent lookups go directly to that HWND. Focus theft, title changes, new windows with similar names — none cause the anchor to drift. If the original window is destroyed, the workflow fails explicitly rather than silently attaching to something else.

| Tier | Lifetime | On stale |
|------|----------|----------|
| **Root** | Process lifetime | Fatal — window is gone |
| **Session** | One open/close cycle | Re-resolved on next use |
| **Stable** | While parent window is open | Re-queried from nearest live ancestor |
| **Ephemeral** | Single phase | Released on phase exit |

## Selectors

CSS-like paths over Windows UI Automation properties:

| Attribute | UIA property |
|---|---|
| `role` | Control type / accessibility role |
| `name` | Accessible name |
| `id` | UIA AutomationId (survives localization) |

```python
>> [role=edit][name='File name:']            # descendant edit field
>  [role=button][name^=Don][name$=Save]      # direct child: "Don't Save"
> [role=list item]:nth(0)                    # first list item
>> [role=list item][name~=Wing]:parent       # parent of matching item
>> [role=tab item]:nth(2) > [role=button]    # button inside third tab
>> [id=SettingsPageAbout_New]                # by AutomationId
>> [role=button|menu item]                   # OR: matches either role
```

String operators: `=` exact, `~=` contains, `^=` starts with, `$=` ends with.
Combinators: `>` immediate child, `>>` any descendant.
Modifiers: `:nth(n)`, `:parent`, `:ancestor(n)`.
`Button[name=Open]` is shorthand for `[role=button][name=Open]`.

Works across Win32, WPF, WinForms, WinUI, and UWP.

## Capabilities

**Conditions** — usable as `expect`, `precondition`, recovery trigger, or flow-control predicate:

- *Element*: `ElementFound`, `ElementEnabled`, `ElementVisible`, `ElementHasText` (exact / contains / starts_with / regex / non_empty), `ElementHasChildren`
- *Window*: `WindowWithAttribute` (title, PID, automation ID), `WindowWithState` (active / visible), `WindowClosed`, `DialogPresent`, `DialogAbsent`, `ForegroundIsDialog`
- *Browser*: `TabWithAttribute` (title / URL match on a CDP tab anchor), `TabWithState` (JS expression evaluated in a tab — truthy = true; use to wait for page readiness)
- *System*: `ProcessRunning`, `FileExists`, `ExecSucceeded` (exit code 0), `EvalCondition` (boolean expression against outputs/locals), `Always`
- *Logic*: `AllOf`, `AnyOf`, `Not`

**Actions**:

- *Interaction*: `Click`, `DoubleClick`, `Hover`, `ClickAt` (fractional coordinates), `Invoke` (IInvokePattern — works on off-screen / virtualised elements), `TypeText`, `SetValue` (ValuePattern, no keystroke simulation), `PressKey`, `Focus`, `ScrollIntoView`, `ActivateWindow`, `MinimizeWindow`, `CloseWindow`, `DismissDialog`
- *Data*: `Extract` (UIA attribute → output variable), `Eval` (expression → output variable), `WriteOutput` (output variable → file)
- *System*: `Exec` (external process, capture stdout), `MoveFile`, `Sleep`
- *Browser*: `BrowserNavigate` (navigate a tab anchor to a URL), `BrowserEval` (evaluate JS in a tab, store result)
- *Control*: `NoOp` (wait for a condition without acting)

**Control flow**: phases can jump to any named phase — loops, branches, early exits. `finally` phases run unconditionally.

**Composition**: workflows declare input `params` and named `outputs` and call other workflows as subroutines.

**Tooling**: JSON Schema for autocomplete and inline validation; built-in linter catches unknown types, invalid selectors, missing fields, and undeclared references before the workflow runs.

## Install (Windows)

```ps
PowerShell -ExecutionPolicy Bypass -Command "iwr https://raw.githubusercontent.com/visioncortex/ui-automata/refs/heads/main/install/install-windows.ps1 | iex"
```

### Example (Windows 11)

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/visioncortex/ui-automata/main/workflow-schema.json
name: notepad_hello
description: Open Notepad, type a message, and save the file.

defaults:
  timeout: 5s

launch:
  exe: notepad.exe
  wait: new_window

anchors:
  notepad:
    type: Root
    process: Notepad
    selector: "[name~=Notepad]"

  editor:
    type: Stable
    parent: notepad
    selector: ">> [role=document][name='Text editor']"

  saveas_dialog:
    type: Ephemeral
    parent: notepad
    selector: "> [role=dialog][name^=Save]"

phases:

  - name: type_text
    mount: [notepad, editor]  # mounted before steps run
    steps:
      - intent: type text into editor
        action:
          type: TypeText
          scope: editor
          selector: "*"
          text: "Hello Automata"
        expect:
          type: ElementHasText
          scope: editor
          selector: "*"
          pattern:
            contains: "Hello Automata"

  - name: save_file
    mount: [saveas_dialog]
    unmount: [saveas_dialog]
    steps:
      - intent: activate keyboard shortcut for Save As
        action:
          type: PressKey
          scope: notepad
          selector: "*"
          key: "ctrl+shift+s"
        expect:
          type: DialogPresent
          scope: notepad

      - intent: type filename in Save As dialog
        action:
          type: SetValue
          scope: saveas_dialog
          selector: ">> [role=edit][name='File name:']"
          value: "hello-world"
        expect:
          type: ElementHasText
          scope: saveas_dialog
          selector: ">> [role=edit][name='File name:']"
          pattern:
            contains: "hello-world"

      - intent: click Save button
        action:
          type: Invoke
          scope: saveas_dialog
          selector: ">> [role=button][name=Save]"
        expect:
          type: DialogAbsent
          scope: notepad
```

### Example (Windows 10)

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/visioncortex/ui-automata/main/workflow-schema.json
name: notepad_hello
description: Open Notepad, type a message, and save the file.

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

  saveas_dialog:
    type: Ephemeral
    parent: notepad
    selector: ">> [role=dialog][name='Save As']"

phases:

  - name: type_text
    mount: [notepad, editor]
    steps:
      - intent: type text into editor
        action:
          type: TypeText
          scope: editor
          selector: "*"
          text: "Hello Automata"
        expect:
          type: ElementHasText
          scope: editor
          selector: "*"
          pattern:
            contains: "Hello Automata"

  - name: save_file
    mount: [saveas_dialog]
    unmount: [saveas_dialog]
    steps:
      - intent: activate keyboard shortcut for Save As
        action:
          type: PressKey
          scope: notepad
          selector: "*"
          key: "ctrl+shift+s"
        expect:
          type: DialogPresent
          scope: notepad

      - intent: type filename in Save As dialog
        action:
          type: SetValue
          scope: saveas_dialog
          selector: ">> [role=combo box][name='File name:'] > [role=edit]"
          value: "hello-world"
        expect:
          type: ElementHasText
          scope: saveas_dialog
          selector: ">> [role=combo box][name='File name:'] > [role=edit]"
          pattern:
            contains: "hello-world"

      - intent: click Save button
        action:
          type: Invoke
          scope: saveas_dialog
          selector: ">> [role=button][name=Save]"
        expect:
          type: DialogAbsent
          scope: notepad
```

## Built for the AI agent era

The project includes an MCP server (`automata-agent`) that exposes the full automation engine to AI agents. This is a separate component, not part of the open-source library.

### Agent-driven authoring

The MCP server gives an agent access to the live desktop: it queries the element tree, tests selectors, runs individual actions, and observes results — the same discovery loop a human would do with an inspector tool. From that exploration it writes the workflow. A human provides intent and reviews the result.

### What the agent can do

- **desktop**: list windows, walk the UIA element tree, test selectors live
- **vision**: OCR and visual layout capture for apps that do not fully expose UIA
- **app**: launch apps, list installed apps, manage windows via the taskbar
- **window**: minimize, maximize, restore, reposition, or screenshot a window by HWND
- **run_actions**: run ad-hoc UI automation steps without a workflow file
- **start_workflow**: run a named workflow and stream per-phase progress until completion
- **workflow**: list workflows, check status, cancel runs, browse run history, lint YAML
- **input**: raw mouse and keyboard input — works on any window regardless of UIA support
- **clipboard**: read or write the Windows clipboard
- **browser**: control Microsoft Edge via CDP — navigate, evaluate JavaScript, read the DOM
- **file**: read, write, copy, move, delete, glob, stat
- **system**: shell execution, process management, system diagnostics
- **resources**: browse the embedded workflow library
