# Install Git for Windows (winget)

Installs Git using the Windows Package Manager (`winget`). No browser, no GUI installer pages.
The agent should offer the user a choice of scope before proceeding.

Same binary as a manual Git for Windows install — includes `git.exe`, Git Bash (mintty + MSYS2),
and Git Credential Manager.

> **Git Bash (mintty) note:** UIA is unavailable in mintty terminals. Use
> `vision window_layout hwnd=<hwnd>` to read output and `run_actions TypeText` / `PressKey`
> to send input. See [browser/install_git_for_windows.md](../browser/install_git_for_windows.md)
> for the full mintty interaction reference.

## Decision: present the choice to the user

Before running anything, ask the user:

> **How would you like to install Git?**
>
> 1. **User scope** *(recommended)* — installs to `%LOCALAPPDATA%\Programs\Git`.
>    No UAC prompt. Git is available only for the current user.
>
> 2. **Machine scope** — installs to `C:\Program Files\Git`.
>    No UAC prompt if the current user is an administrator. A UAC prompt will appear for
>    standard users — see Path 2 for how to handle it.
>    Git is available for all users on the machine.

Wait for the user's answer, then follow the corresponding path below.

## Path 1 — User scope (no UAC)

```json
system {"action":"exec","args":["winget.exe","install","--id","Git.Git","-e","--source","winget","--scope","user","--silent","--accept-package-agreements","--accept-source-agreements"]}
```

Expected output contains `Successfully installed` or `No applicable upgrade found` (already installed).
On success, verify:

```json
system {"action":"exec","args":["git.exe","--version"]}
```

> **Key constraints:**
> - `--scope user` installs to `%LOCALAPPDATA%\Programs\Git` — not on PATH immediately in the
>   current shell session. Open a new terminal or use the full path to verify.
> - If winget is not found: see **Winget not available** below.

## Path 2 — Machine scope (requires UAC)

```json
system {"action":"exec","args":["winget.exe","install","--id","Git.Git","-e","--source","winget","--scope","machine","--silent","--accept-package-agreements","--accept-source-agreements"],"timeout_secs":120}
```

If the current user is an administrator, winget installs silently with no UAC prompt.
If the user is a **standard user**, a UAC dialog will appear. **UIA is unavailable** on the UAC
dialog (Secure Desktop) — use `vision` + `input mouse_click`:

1. Take a `vision screen_layout` to locate the UAC dialog and the **Yes** button.
2. Click **Yes** with `input mouse_click` using the coordinates from vision output.
3. Wait for the winget command to complete (up to 2 minutes).

Verify:

```json
system {"action":"exec","args":["git.exe","--version"]}
```

> **Key constraints:**
> - Do not retry winget while the UAC prompt is pending — the command is already waiting.
> - If the UAC dialog does not appear within 30 s, take a `vision screen_layout` to check state.

## Verify installation

**Step 1 — check git.exe is on PATH:**

```json
system {"action":"exec","args":["git.exe","--version"]}
```

Expected: `git version 2.x.x.windows.x`

**Step 2 — confirm install path:**

```json
system {"action":"exec","args":["powershell.exe","-Command","(Get-Command git).Source"]}
```

**Step 3 — launch Git Bash and run `uname`:**

For machine-scope installs (`C:\Program Files\Git`):

```json
app {"action":"launch","exe":"C:\\Program Files\\Git\\git-bash.exe","launch_wait":"new_window"}
```

For user-scope installs (`%LOCALAPPDATA%\Programs\Git`) — expand the env var first:

```json
system {"action":"exec","args":["powershell.exe","-Command","$cmd = Get-Command git-bash.exe -ErrorAction SilentlyContinue; if ($cmd) { $cmd.Source } else { \"$env:LOCALAPPDATA\\Programs\\Git\\git-bash.exe\" }"]}
```

Use the returned path in the launch:

```json
app {"action":"launch","exe":"<path from above>","launch_wait":"new_window"}
```

Git Bash uses mintty — UIA is unavailable; read output via vision:

```json
vision {"action":"window_layout","hwnd":"<hwnd of the new mintty window>"}
```

Type the command and press Enter:

```json
run_actions {"window":{"process":"mintty"},"steps":[{"intent":"run uname","action":{"type":"TypeText","scope":"window","selector":"*","text":"uname"},"expect":{"type":"Always"}},{"intent":"press Enter","action":{"type":"PressKey","scope":"window","selector":"*","key":"{enter}"},"expect":{"type":"Always"}}]}
```

Read the output via vision:

```json
vision {"action":"window_layout","hwnd":"<hwnd>"}
```

Expected output: `MINGW64_NT-10.0-19045` (or similar build number). Confirms Git Bash is functional.
