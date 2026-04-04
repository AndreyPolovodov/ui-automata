# Browser Use Guide

Two complementary mechanisms are available: **CDP** (`browser` tool) and **UIA** (`run_actions` tool).
Use the right one for each job — they have different strengths and blind spots.

## CDP vs UIA — when to use which

| Situation | Use |
|-----------|-----|
| Read page content, extract text/URLs | CDP `eval` / `dom` |
| Navigate to a URL | CDP `navigate` |
| Click a same-tab link (no `target=_blank`) | CDP `eval` `.click()` |
| Click a link that opens a new tab/window (`target=_blank`) | UIA `Invoke` or `Click` |
| Click the browser's own UI (address bar, toolbar buttons, download bar) | UIA |
| Download a file via a link | UIA `Invoke` (CDP `.click()` is blocked — see below) |
| Type into a web form field | CDP `eval` (`element.value = …; element.dispatchEvent(…)`) or UIA `SetValue` |
| Open/dismiss Edge dialogs (pop-up blocked, permissions) | UIA |

## Click on links using CDP

```html
<a href="https://iana.org/domains/example">Learn more</a>
```

Works for same-tab links (no `target=_blank`). Target by `href`:

```json
browser {"action":"eval","tab_id":"<id>","expression":"document.querySelector('a[href=\"https://www.iana.org/domains/reserved\"]').click()"}
```

## CDP `.click()` is blocked for `target=_blank` links

```html
<a id="download" href="https://github.com/git-for-windows/git/releases/latest" target="_blank">Download</a>
```

CDP `.click()` dispatches a **synthetic event** — it does not count as a user gesture.
Edge treats pop-ups opened from synthetic events as blocked.

Symptoms:
- A "Pop-up blocked" button appears in the address bar.
- The download or new tab never opens.

**Fix:** use UIA `Invoke` or `Click` instead — both send real `SendInput` events
which Edge counts as a user gesture.

```json
run_actions {"window":{"process":"msedge"},"steps":[
  {"intent":"click download link","action":{"type":"Invoke","scope":"window","selector":">> [role=document] >> [role=link][name=Download]"},"expect":{"type":"Always"}}
]}
```

## UIA element names don't always match visible text

The UIA name of an element comes from the accessibility tree, which may or may not match
the visible rendered text — it depends on how the page sets accessible names (`aria-label`,
`title`, inner text, etc.).

**Always discover the real UIA name before writing selectors:**

```json
desktop {"action":"find_elements","process":"msedge","selector":">> [role=document] >> [role=link]"}
```

Never guess the name from the page source — use what the tree reports.

## UIA `Invoke` vs `Click` on browser links

- **`Invoke`** — calls `IInvokePattern::Invoke()` directly; no bounding rect needed.
  Reliable for off-screen elements, toolbar buttons, and download links.
  Works for `target=_blank` links that open new tabs/windows.

- **`Click`** — sends real `SendInput` mouse events to the element's bounding rect.
  Works for on-screen same-tab links.
  Requires the element to have valid (non-zero) bounds.

For **download links** or **`target=_blank` links**: prefer `Invoke`.
For **on-screen in-page buttons**: prefer `Click`.

## Ensure CDP is connected before use

Always call `browser ensure` first. If Edge is restarted or crashes, the CDP port
changes and existing tab IDs become invalid.

```json
browser {"action":"ensure"}
browser {"action":"tabs"}
```

After `ensure`, always re-fetch tabs — previous tab IDs are not guaranteed to survive.

## Opening the Downloads panel

Edge's Downloads panel is a UIA dialog, not a web page — use UIA, not CDP:

```json
run_actions {"window":{"process":"msedge"},"steps":[
  {"intent":"open downloads panel","action":{"type":"PressKey","scope":"window","selector":"*","key":"ctrl+j"},"expect":{"type":"ElementFound","scope":"window","selector":">> [role=dialog][name=Downloads]"}}
]}
```

Inspect download items via the element tree:

```json
desktop {"action":"element_tree","process":"msedge","selector":">> [role=dialog][name=Downloads]"}
```

## Handling a "Pop-up blocked" prompt

If a CDP `.click()` was used on a `target=_blank` link, dismiss and allow via UIA:

```json
run_actions {"window":{"process":"msedge"},"steps":[
  {"intent":"open pop-up blocked menu","action":{"type":"Click","scope":"window","selector":">> [role=button][name^=Pop-up]"},"expect":{"type":"ElementFound","scope":"window","selector":">> [role=radio button][name^=Always allow]"}},
  {"intent":"select Always allow","action":{"type":"Click","scope":"window","selector":">> [role=radio button][name^=Always allow]"},"expect":{"type":"Always"}},
  {"intent":"click Done","action":{"type":"Click","scope":"window","selector":">> [role=button][name=Done]"},"expect":{"type":"Always"}}
]}
```

## Multiple Edge windows

When more than one Edge window is open, `run_actions {"window":{"process":"msedge"}}` targets
the **topmost** (z-order front) window. To target a specific window, filter by `hwnd` (from `desktop list_windows`).

## Closing the browser

Use `CloseWindow` via `run_actions` — do not kill the process.

`CloseWindow` sends `WM_CLOSE` which triggers Edge's normal shutdown path (session save, etc.).
