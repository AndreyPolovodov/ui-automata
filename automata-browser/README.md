# automata-browser

CDP-based browser control for [ui-automata](https://github.com/visioncortex/ui-automata/).

## What this is for

`automata-browser` gives ui-automata workflows a way to drive a browser as part of a
larger desktop automation sequence — the same way you might click a Win32 dialog or
invoke a ribbon button. The primary target is **Microsoft Edge on Windows**, which is
pre-installed, always present, and a stable automation target via CDP.

Typical use cases:

- Open a URL as part of a workflow step, extract data, and hand off to the next step
- Fill a web form mid-workflow after interacting with a native desktop application
- Capture a screenshot of a web page at a known workflow checkpoint
- Evaluate JavaScript to read page state before deciding what to do next

The browser is treated as just another controllable surface — tabs are identified by
their CDP target ID and all operations (navigate, eval, dom, screenshot) are addressed
by that ID, with no persistent connection state required by the caller.

## What this is not for

- **General cross-browser testing** — there is no Safari or Firefox support, and none
  is planned. If you need cross-browser coverage, use Playwright.
- **macOS or Linux browser automation** — the launcher probes for Chrome/Chromium on
  those platforms as a best-effort fallback, but the primary workflow target is Edge on
  Windows.
- **High-frequency scraping or headless pipelines** — the stateless-per-call CDP model
  opens a fresh WebSocket per operation. It is correct and simple, but not optimised for
  throughput. See the roadmap below.

## Design

### Edge as the stable target

Edge ships with every modern Windows installation and can be launched with
`--remote-debugging-port` without user interaction. This makes it a uniquely reliable
automation target in a Windows-first tool: no download step, no version pinning,
no external dependency.

Chrome/Chromium are probed on Linux and macOS as a convenience, but they are not a
supported production target.

### Tab IDs, not WebSocket URLs

The public API uses the CDP **target ID** (a hex string) to identify tabs — the same
identifier returned by `GET /json`. The WebSocket debugger URL is an implementation
detail constructed internally from the known port and target ID:

```
ws://127.0.0.1:{port}/devtools/page/{tab_id}
```

Callers never construct or store WebSocket URLs. This keeps the API stable regardless
of CDP internal routing and makes tab handles safe to pass through the ui-automata
protocol.

### Stateless-per-call (current)

Every operation opens a fresh WebSocket, sends one CDP command, and closes. This
means:

- No session state to manage or recover
- Safe to call from multiple threads without coordination
- Correct for the workflow use case where operations are seconds apart

The cost is connection overhead on every call, which is negligible at workflow cadence.

### Persistent sessions (roadmap)

For operations that require continuity — monitoring a download, streaming DOM mutation
events, or driving a long multi-step web form — a persistent session model is planned.
A session would hold an open WebSocket and expose a typed event stream, with the
stateless API remaining available for one-shot operations.

## Roadmap

- **Persistent CDP sessions** — long-lived WebSocket connections for event-driven
  workflows; opt-in alongside the existing stateless API
- **Download manager** — session-based tracking of `Browser.downloadWillBegin` /
  `Page.downloadProgress` events; expose download state as a workflow condition so steps
  can wait on or react to file downloads without polling the filesystem

## CLI

```
automata-browser launch      [--port 9222]
automata-browser tabs        [--port 9222]
automata-browser open        [<url>]          [--port 9222]
automata-browser navigate    <tab_id> <url>   [--port 9222]
automata-browser eval        <tab_id> <expr>  [--port 9222]
automata-browser dom         <tab_id>         [--port 9222]
automata-browser screenshot  <tab_id>         [--port 9222]
automata-browser activate    <tab_id>         [--port 9222]
automata-browser close       <tab_id>         [--port 9222]
```

`open` without a URL opens a blank tab (`about:blank`).
All commands default to port `9222` if `--port` is not supplied.

## License

MIT
