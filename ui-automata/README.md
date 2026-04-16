<div align="center">

  <img src="https://github.com/visioncortex/vtracer/raw/master/docs/images/visioncortex-banner.png">
  <h1>UI Automata</h1>

  <p>
    <strong>The AI Toolkit for Windows Desktop Automation</strong>
  </p>

  <h3>
    <a href="https://automata.visioncortex.org">Website</a>
    <span> | </span>
    <a href="https://github.com/visioncortex/ui-automata/releases/latest">Download</a>
    <span> | </span>
    <a href="https://automata.visioncortex.org/docs/introduction/what-is-ui-automata/">Docs</a>
  </h3>
</div>

UI Automata lets an AI agent write, run, and debug automation workflows across any Windows application (Win32, WPF, WinForms, WinUI, UWP) and Edge-based browsers, in the same workflow file.

We at Vision Cortex built it to help clients in industrial design automate CAD/CAM tasks: multi-step workflows in desktop applications that have no API, require precise timing, and need to handle popups and error dialogs without failing silently. We're open-sourcing it so other teams can do the same.

## The problem

Windows UI work is exactly what AI agents should handle. But every step can fail in ways a script cannot see:

- **Timing**: a button click completes before the app finishes processing. The UI looks ready; it is not.
- **Transient disabled state**: the element exists and is visible, but temporarily disabled. The click fires, nothing happens, no error is returned.
- **Popup interruptions**: a modal dialog captures focus mid-step. The script waits for an outcome that will never come.
- **Stale handles**: the app rebuilds its UI after a navigation. Cached references point to the wrong place; clicks land silently on the wrong target.
- **Focus loss**: a keypress meant for one field lands on another.

These are not edge cases: they are routine in any real Windows application.

## Quick Demo

<video src="https://github.com/user-attachments/assets/81f0cbb4-e3bf-416f-a373-74ee2cb3bedb" controls width="100%"></video>

> Install Python on this machine from the Microsoft Store, not python.org. Pick the latest 3.x version.

The agent opens the Store via its app URI, searches for Python, reads the result cards to identify 3.13, clicks Get, and polls until installation completes.

> Grab the latest installer from the official site, run it silently, then open Git Bash and confirm it works.

For Git, the agent navigates to gitforwindows.org in Edge, triggers the download using UIA (CDP synthetic clicks are blocked for file downloads), waits for the installer to finish via the Downloads panel, runs it silently with UAC confirmation, then falls back to vision OCR to read the Git Bash terminal output where UIA has no coverage.

## Installation

```powershell
PowerShell -ExecutionPolicy Bypass -Command "iwr https://raw.githubusercontent.com/visioncortex/ui-automata/refs/heads/main/install/install-windows.ps1 | iex"
```

## Use cases

UI Automata is designed to be driven by an AI agent through its MCP server. Here are some example prompts:

### Automate a desktop application

<video src="https://github.com/user-attachments/assets/59429bd6-fa34-4530-998f-b6628eacac54" controls width="100%"></video>

> Use the ui-automata skill. Open Mastercam, load the file at C:\Projects\part.mcam, open the simulator window, and export the result csv to C:\output\.

The agent walks the element tree, tests selectors live, writes the workflow YAML, and runs it (all in one session). You provide the intent and review the result.

<!--
### Build a workflow from your actions

> I need to automate our weekly report export from our ERP. It has no API. Watch me navigate through the dialogs and turn my actions into a reusable workflow that takes `start_date` and `end_date` as parameters.

The agent observes the live element tree as you navigate, then generates a parameterised workflow you can schedule and run unattended.
-->

### Fix a broken workflow

> Our workflow that enters invoices into AccountMate is failing with a stale handle error after the confirmation dialog closes. Here's the error trace. Fix it.

The agent replays the workflow, pauses at the failure, inspects the live element tree, and adjusts the anchor or selector to handle the UI rebuild.

### Automate across desktop and browser

> Download this month's supplier invoices from our vendor portal in Edge, then open our accounting desktop app and enter each one. The portal URL is...

Workflows can mix browser steps (CDP-powered, structured DOM access) with desktop app steps in a single file. No need to stitch together separate tools.

## How it works

Every step declares an **action**, an **expect** condition, and an optional **recovery** handler:

```yaml
- intent: click the Open button
  action:
    type: Click
    scope: main_window
    selector: ">> [role=button][name=Open]"
  expect:
    type: DialogPresent
    scope: main_window
  timeout: 10s
```

The engine runs the same lifecycle for every step:

1. Execute the action
2. Poll `expect` every 100ms
3. Condition passes → advance. Timeout → run recovery handler, then retry, skip, or fail.

No sleeps. No hardcoded waits. Recovery handlers are declared once and fire wherever their trigger condition is met: a dialog appearing mid-workflow, a confirmation prompt, a progress bar that needs to clear.

<video src="https://github.com/user-attachments/assets/a68c115f-34fc-461e-b682-bc21840c6685" controls width="100%"></video>

See the [notepad demo workflow](https://github.com/visioncortex/ui-automata/blob/main/workflows/win11/notepad/notepad_demo.yml) for a complete example with phases, anchors, recovery handlers, and flow control.

## Selectors

CSS-like paths over Windows UI Automation properties:

```
>> [role=edit][name='File name:']           # descendant edit field
>  [role=button][name^=Don][name$=Save]     # direct child: "Don't Save"
>> [role=list item]:nth(0)                  # first list item
>> [role=list item][name~=Wing]:parent      # parent of matching item
>> [id=SettingsPageAbout_New]               # by AutomationId (locale-stable)
>> [role=button|menu item]                  # OR: matches either role
```

Works across Win32, WPF, WinForms, WinUI, and UWP. → [Full reference](https://automata.visioncortex.org/docs/core-concepts/selectors/)

## The shadow DOM

Windows UI Automation is a cross-process RPC protocol: every element query is a round-trip to the target process. Walk a path of nested elements and each level is a separate cross-process call. Traditional automation tools pay this cost on every step; a 20-step workflow makes 20+ round-trips, re-discovering structure it already found the step before.

UI Automata's answer is the **shadow DOM**: a cached mirror of the live element tree. Handles are resolved once and reused for every subsequent step (a cached lookup is effectively free compared to a live UIA query). Think of it as the inverse of React's virtual DOM: React maintains a virtual tree to efficiently write to a UI it controls; the shadow DOM maintains one to efficiently read from a UI it does not.

→ [How the shadow DOM works](https://automata.visioncortex.org/docs/core-concepts/shadow-dom/)

## Agent tools

The included MCP server (`automata-agent`) gives an AI agent direct access to the Windows desktop:

- **desktop** — list windows, walk the UIA element tree, test selectors live
- **vision** — OCR and visual layout capture for apps with incomplete UIA support
- **app** — launch apps, list installed apps, manage windows via the taskbar
- **window** — minimize, maximize, restore, reposition, or screenshot a window by HWND
- **run_actions** — run ad-hoc UI automation steps without a workflow file
- **start_workflow** — run a named workflow and stream per-phase progress
- **workflow** — list workflows, check status, cancel runs, browse run history, lint YAML
- **input** — raw mouse and keyboard input, works on any window regardless of UIA support
- **clipboard** — read or write the Windows clipboard
- **browser** — control Edge via CDP: navigate, evaluate JavaScript, read the DOM
- **file** — read, write, copy, move, delete, glob, stat
- **system** — shell execution, process management, system diagnostics
- **resources** — browse the embedded workflow library

## Compared to vision-based agents

Vision agents work by taking a screenshot and asking an inference model what to click next. UI Automata uses Windows UI Automation directly, with vision as a fallback (not the primary path):

| | UI Automata | Vision agent |
|---|---|---|
| **Approach** | UIA elements + DOM query + vision | Screenshot only |
| **Reliability** | Deterministic — same selector works across runs | May vary across runs |
| **Speed** | Sub-second per step | Round-trip to inference API per step |
| **Cost** | Low — runs locally, no per-step inference | High — every step consumes tokens |
| **Vision** | On-device, used as fallback | Cloud inference, primary approach |
| **Platform** | Windows (all frameworks) | macOS-first, limited Windows |
| **Model dependency** | Any agent, any model | Locked to specific providers |
| **Browser automation** | CDP (structured page access) | Screenshot of browser |
| **Trace** | Structured log with full action detail | Sequence of screenshots |

The two approaches are complementary: use UI Automata for deterministic, repeatable workflows; use vision when you need to handle unfamiliar apps or pages on the fly.

## Community

Have a question or want to share what you've built? Join the conversation on [GitHub Discussions](https://github.com/visioncortex/ui-automata/discussions).

Found a bug? Want a feature? [Open an issue](https://github.com/visioncortex/ui-automata/issues/new).
