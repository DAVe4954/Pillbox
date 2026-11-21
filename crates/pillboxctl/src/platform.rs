//! The only `unsafe` in pillboxctl: a thin wrapper around the process list.

use windows::Win32::{
    Foundation::CloseHandle,
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    },
};

/// One running process: id, parent id, lower-case executable file name.
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    pub exe: String,
}

/// A snapshot of all processes; empty if Windows will not give one.
pub fn processes() -> Vec<Proc> {
    let mut out = Vec::new();
    // SAFETY: the snapshot handle is closed before returning. `entry` is a local with its
    // `dwSize` set as the API requires, and the strings are read only up to their first NUL
    // within the fixed-size array.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snap, &mut entry).is_ok();
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            out.push(Proc {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                exe: String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase(),
            });
            more = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}
