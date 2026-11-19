//! Windows-only verified receiver process recovery and kill-on-exit ownership.
//! AirPlay-Windows 0.4.1 uses a session-wide SINGLE-INSTANCE mutex. Launching
//! another copy merely activates the existing one and exits with code 0.
//! MirroLeap must adopt ONLY its own pinned installed executable by exact path;
//! never kill a different user's similarly named AirPlay receiver.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::process::Child;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{HANDLE, WAIT_TIMEOUT};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW,
    PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
    JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SET_QUOTA, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

fn normalized_path(path: &str) -> String {
    path.trim_start_matches(r"\\?\").replace('/', "\\").to_lowercase()
}

fn same_executable(actual: &str, expected: &Path) -> bool {
    let expected = expected.to_string_lossy();
    normalized_path(actual) == normalized_path(&expected)
}

fn full_path(handle: HANDLE) -> Option<String> {
    let mut name = [0u16; 32768];
    let mut size = name.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32,
            PWSTR(name.as_mut_ptr()), &mut size)
    }.ok()?;
    Some(String::from_utf16_lossy(&name[..size as usize]))
}

fn process_handle(pid: u32) -> Option<OwnedHandle> {
    if pid == 0 { return None; }
    let handle = unsafe { OpenProcess(
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA |
            PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
        false, pid
    ) }.ok()?;
    Some(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

pub fn verified_receiver_pid(path: &Path) -> Option<u32> {
    if !path.is_file() { return None; }
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot.0) };
    let snapshot_handle = HANDLE(snapshot.as_raw_handle());
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snapshot_handle, &mut entry) }.is_err() { return None; }
    loop {
        let end = entry.szExeFile.iter().position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if name.eq_ignore_ascii_case("airplay-windows.exe") {
            let pid = entry.th32ProcessID;
            if let Some(handle) = process_handle(pid) {
                let handle = HANDLE(handle.as_raw_handle());
                if unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT
                    && full_path(handle).as_deref().is_some_and(|f|same_executable(f,path))
                {
                    return Some(pid);
                }
            }
        }
        if unsafe { Process32NextW(snapshot_handle, &mut entry) }.is_err() { break; }
    }
    None
}

pub fn verified_pid_alive(pid: u32, path: &Path) -> bool {
    let Some(handle) = process_handle(pid) else { return false; };
    let handle = HANDLE(handle.as_raw_handle());
    unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT
        && full_path(handle).as_deref().is_some_and(|f|same_executable(f,path))
}

pub fn stop_verified_pid(pid: u32, path: &Path) -> Result<bool, String> {
    let Some(handle) = process_handle(pid) else { return Ok(false); };
    let handle = HANDLE(handle.as_raw_handle());
    // Verify the full path AFTER obtaining an open process handle, making
    // PID recycling unable to kill a different executable.
    if !full_path(handle).as_deref().is_some_and(|f|same_executable(f,path)) {
        return Ok(false);
    }
    if unsafe { WaitForSingleObject(handle, 0) } != WAIT_TIMEOUT { return Ok(false); }
    unsafe { TerminateProcess(handle, 0) }
        .map_err(|e|format!("Could not stop verified AirPlay receiver: {e}"))?;
    let _ = unsafe { WaitForSingleObject(handle, 2000) };
    Ok(true)
}

/// Kernel job ownership survives abrupt/crash exits: the OS closes the job
/// handle on MirroLeap termination and kills only assigned receiver children.
/// This avoids relying solely on Rust Drop, which doesn't run on TerminateProcess.
fn create_kill_on_exit_job(process:HANDLE) -> Result<OwnedHandle, String> {
    let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .map_err(|e|format!("CreateJobObjectW: {e}"))?;
    let handle = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let job = HANDLE(handle.as_raw_handle());
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job, JobObjectExtendedLimitInformation,
            (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32
        )
    }.map_err(|e|format!("SetInformationJobObject: {e}"))?;
    unsafe { AssignProcessToJobObject(job, process) }
        .map_err(|e|format!("AssignProcessToJobObject: {e}"))?;
    Ok(handle)
}

pub fn kill_on_exit_job(child: &Child) -> Result<OwnedHandle, String> {
    create_kill_on_exit_job(HANDLE(child.as_raw_handle()))
}

pub fn kill_on_exit_pid(pid: u32, path: &Path) -> Result<OwnedHandle, String> {
    let process = process_handle(pid).ok_or("No accessible existing AirPlay receiver")?;
    let handle = HANDLE(process.as_raw_handle());
    if !full_path(handle).as_deref().is_some_and(|f|same_executable(f,path)) {
        return Err("Existing AirPlay process has a different executable path".into());
    }
    create_kill_on_exit_job(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_normalized_path_required_to_adopt_or_terminate() {
        let path = Path::new(r"C:\Users\Test\AppData\Local\MirroLeap\ios-airplay\v0.4.1\airplay-windows.exe");
        assert!(same_executable(
            r"\\?\C:\USERS\Test\AppData\Local\MirroLeap\ios-airplay\v0.4.1\airplay-windows.exe",path
        ));
        assert!(!same_executable(
            r"C:\OtherApp\airplay-windows.exe",path
        ));
        assert!(!same_executable(
            r"C:\Users\Test\AppData\Local\MirroLeap\ios-airplay\v0.4.1\airplay-windows.exe.bak",path
        ));
    }
    #[test]
    fn no_pid_zero_or_unrelated_process_is_manageable() {
        assert!(process_handle(0).is_none());
    }
}
