use crate::Result;
use anyhow::bail;

/// Look up a process name (without .exe) by PID.
#[cfg(target_os = "windows")]
pub fn get_process_name(pid: i32) -> Result<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        if snapshot.is_invalid() {
            bail!("Invalid snapshot handle");
        }

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_err() {
            let _ = CloseHandle(snapshot);
            bail!("Failed to iterate processes");
        }

        loop {
            if entry.th32ProcessID == pid as u32 {
                let _ = CloseHandle(snapshot);
                let name_slice = &entry.szExeFile;
                let len = name_slice
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(name_slice.len());
                let name = String::from_utf16_lossy(&name_slice[..len]);
                let clean = name
                    .strip_suffix(".exe")
                    .or_else(|| name.strip_suffix(".EXE"))
                    .unwrap_or(&name)
                    .to_string();
                return Ok(clean);
            }
            if Process32NextW(snapshot, &mut entry).is_err() {
                break;
            }
        }

        let _ = CloseHandle(snapshot);
        bail!("PID {pid} not found")
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_process_name(_pid: i32) -> Result<String> {
    bail!("Windows only")
}

/// Return all running (pid, name-without-.exe) pairs.
#[cfg(target_os = "windows")]
pub fn list_processes() -> Vec<(u32, String)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let mut out = Vec::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_err() {
            let _ = CloseHandle(snapshot);
            return out;
        }
        loop {
            let name_slice = &entry.szExeFile;
            let len = name_slice
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(name_slice.len());
            let name = String::from_utf16_lossy(&name_slice[..len]);
            let clean = name
                .strip_suffix(".exe")
                .or_else(|| name.strip_suffix(".EXE"))
                .unwrap_or(&name)
                .to_string();
            out.push((entry.th32ProcessID, clean));
            if Process32NextW(snapshot, &mut entry).is_err() {
                break;
            }
        }
        let _ = CloseHandle(snapshot);
    }
    out
}

#[cfg(not(target_os = "windows"))]
pub fn list_processes() -> Vec<(u32, String)> {
    vec![]
}

/// Force-terminate a process by PID.
#[cfg(target_os = "windows")]
pub fn kill_process_by_pid(pid: u32) -> Result<()> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|e| anyhow::anyhow!("OpenProcess({pid}) failed: {e}"))?;
        let result = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);
        result.map_err(|e| anyhow::anyhow!("TerminateProcess({pid}) failed: {e}"))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn kill_process_by_pid(_pid: u32) -> Result<()> {
    bail!("Windows only")
}
