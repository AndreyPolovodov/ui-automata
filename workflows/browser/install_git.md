# Install Git for Windows

## Steps

1. Run `browser/download_git.yml` to download the installer and capture its filename.
2. Tell the user: **"A UAC prompt will appear — please click Yes to allow the install."**
3. Launch the installer in silent mode and wait for it to complete.
4. Verify installation.

> **Key constraint:** The Git installer always requests elevation (UAC) regardless of install
> directory. Instruct the user to click **Yes** before launching — UIA is unavailable on the
> UAC dialog.

## MCP Tool Call Reference

### Download Installer

Run the workflow to navigate to gitforwindows.org, click Download, and capture the installer filename:

```json
workflow {"file":"browser/download_git.yml"}
```

The workflow outputs `installer_filename` (e.g. `Git-2.53.0.2-64-bit.exe`).

### Running the Installer

**Run the installer silently:**

The Git for Windows installer supports Inno Setup silent flags. Replace the filename with the
actual version from the workflow output.

```json
system {"action":"exec","args":["cmd.exe","%USERPROFILE%\\Downloads\\Git-2.53.0.2-64-bit.exe /VERYSILENT /NORESTART"],"timeout_secs":120}
```

A UAC prompt will appear — the user must click **Yes**. The command will wait until the install
completes.

### Verify Installation

**Step 1 — check git.exe is on PATH:**

```json
system {"action":"exec","args":["git.exe","--version"]}
```

Expected: `git version 2.x.x.windows.x`

**Step 2 — confirm install path:**

```json
system {"action":"exec","args":["powershell.exe","-Command","Get-Command git | Select-Object -ExpandProperty Source"]}
```

**Step 3 — launch Git Bash and run `uname`:**

```json
app {"action":"launch","exe":"C:\\Program Files\\Git\\git-bash.exe","launch_wait":"new_window"}
```

Git Bash uses mintty — UIA is unavailable; read output via vision:

```json
vision {"action":"window_layout","hwnd":"<hwnd from launch>"}
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
