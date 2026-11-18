//! Opt-in Windows AirPlay receiver adapter (not an Android/scrcpy backend).
//! Downloads a PINNED third-party GPL-3.0 binary only after a user click.
//! No unverified executables, arbitrary URLs or paths are accepted from JS.
use serde::Serialize;
use std::process::Child;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State, Window};

pub const ENGINE_NAME: &str = "AirPlay-Windows";
pub const ENGINE_VERSION: &str = "v0.4.1";
pub const ENGINE_SOURCE: &str = "https://github.com/moieric11/AirPlay-Windows";
pub const ENGINE_RELEASE: &str =
    "https://github.com/moieric11/AirPlay-Windows/releases/download/v0.4.1/airplay-windows-v0.4.1-x64.zip";
pub const ENGINE_ARCHIVE_SHA256: &str =
    "5f1792a96efffe84b220edfa7286fa7670806226aa4d0b4b6c7e33ddd7747bf9";

#[derive(Default)]
pub struct IosAirplayState {
    child: Mutex<Option<Child>>,
    install_gate: tokio::sync::Mutex<()>,
}
impl Drop for IosAirplayState {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            if let Some(mut process) = child.take() {
                let _ = process.kill();
                let _ = process.wait();
            }
        }
    }
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IosAirplayStatus {
    pub supported: bool,
    pub installed: bool,
    /// This is the RECEIVER PROCESS state, not proof of a connected iPhone.
    pub running: bool,
    pub installing: bool,
    pub receiver_name: String,
    pub engine_version: String,
}

fn main_only(window: &Window) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Only the main MirroLeap window may manage the iOS receiver".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn engine_root(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app.path().app_local_data_dir()
        .map_err(|e| format!("Cannot find user-local app data: {e}"))?
        .join("ios-airplay").join(ENGINE_VERSION))
}
#[cfg(target_os = "windows")]
fn installed(root: &std::path::Path) -> bool {
    root.join("airplay-windows.exe").is_file()
        && root.join("LICENSE").is_file()
        && root.join("THIRD_PARTY_NOTICES.md").is_file()
        && std::fs::read_to_string(root.join(".mirroleap-verified"))
            .ok().as_deref() == Some(ENGINE_ARCHIVE_SHA256)
}
#[cfg(target_os = "windows")]
fn running(state: &IosAirplayState) -> Result<bool, String> {
    let mut process = state.child.lock().map_err(|e| e.to_string())?;
    if let Some(child) = process.as_mut() {
        match child.try_wait() {
            Ok(None) => return Ok(true),
            Ok(Some(_)) => { *process = None; }
            Err(e) => return Err(format!("Cannot inspect AirPlay receiver process: {e}")),
        }
    }
    Ok(false)
}
fn status_for(app: &AppHandle, state: &IosAirplayState) -> Result<IosAirplayStatus, String> {
    #[cfg(target_os = "windows")]
    {
        let root=engine_root(app)?;
        Ok(IosAirplayStatus {
            supported: true,
            installed: installed(&root),
            running: running(state)?,
            installing: state.install_gate.try_lock().is_err(),
            // Users may change this in the receiver's OPTIONS > Receiver Name.
            receiver_name: ENGINE_NAME.to_string(),
            engine_version: ENGINE_VERSION.to_string(),
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _=(app,state);
        Ok(IosAirplayStatus {
            supported: false, installed: false, running: false, installing: false,
            receiver_name: ENGINE_NAME.to_string(),
            engine_version: ENGINE_VERSION.to_string(),
        })
    }
}

#[tauri::command]
pub fn ios_airplay_status(
    window: Window, app: AppHandle, state: State<'_, IosAirplayState>,
) -> Result<IosAirplayStatus, String> {
    main_only(&window)?;
    status_for(&app,&state)
}

#[cfg(target_os = "windows")]
fn safe_zip_member(name: &str) -> bool {
    let normalized=name.trim_end_matches('/');
    !normalized.is_empty() && !name.starts_with('/') && !name.contains('\\')
        && !name.contains(':') && !name.contains('\0')
        && normalized.split('/').all(|segment| !segment.is_empty()
            && segment != "." && segment != "..")
}
#[cfg(target_os = "windows")]
fn unpack_pinned_receiver(bytes: &[u8], root: &std::path::Path) -> Result<(), String> {
    use sha2::{Digest,Sha256};
    use std::io::{Cursor,Write};
    let digest=format!("{:x}",Sha256::digest(bytes));
    if digest!=ENGINE_ARCHIVE_SHA256 {
        return Err("AirPlay component SHA-256 mismatch; installation refused".into());
    }
    let mut archive=zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e|format!("Invalid AirPlay receiver ZIP: {e}"))?;
    if archive.len() > 120 {return Err("Receiver archive has too many entries".into());}
    let parent=root.parent().ok_or("AirPlay installation folder is unavailable")?;
    std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
    let staging=parent.join("v0.4.1-incomplete");
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e|e.to_string())?;
    }
    std::fs::create_dir(&staging).map_err(|e|e.to_string())?;
    let mut extract=||->Result<(),String>{
        let mut total: u64=0;
        for i in 0..archive.len() {
            let mut item=archive.by_index(i).map_err(|e|e.to_string())?;
            let name=item.name().to_string();
            if !safe_zip_member(&name) {
                return Err(format!("Unsafe AirPlay receiver archive member: {name}"));
            }
            if item.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000) {
                return Err("Receiver archive contains a symbolic link".into());
            }
            total=total.checked_add(item.size()).ok_or("Receiver archive too large")?;
            if total>180_000_000 || item.size()>95_000_000 {
                return Err("Receiver archive exceeds extraction limits".into());
            }
            let output=staging.join(name.trim_end_matches('/'));
            if item.is_dir() {
                std::fs::create_dir_all(output).map_err(|e|e.to_string())?;
            } else {
                if let Some(parent)=output.parent() {
                    std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
                }
                let mut target=std::fs::OpenOptions::new()
                    .create_new(true).write(true).open(&output)
                    .map_err(|e|format!("Cannot create AirPlay runtime file: {e}"))?;
                std::io::copy(&mut item,&mut target).map_err(|e|e.to_string())?;
                target.flush().map_err(|e|e.to_string())?;
            }
        }
        if !staging.join("airplay-windows.exe").is_file()
            || !staging.join("LICENSE").is_file()
            || !staging.join("THIRD_PARTY_NOTICES.md").is_file() {
            return Err("Downloaded AirPlay runtime lacks executable or GPL notices".into());
        }
        std::fs::write(staging.join(".mirroleap-verified"), ENGINE_ARCHIVE_SHA256)
            .map_err(|e|e.to_string())?;
        Ok(())
    };
    if let Err(error)=extract() {
        let _=std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    if root.exists() {
        std::fs::remove_dir_all(root).map_err(|e|e.to_string())?;
    }
    std::fs::rename(&staging,root).map_err(|e|e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn ios_airplay_install(
    window: Window, app: AppHandle, state: State<'_, IosAirplayState>,
) -> Result<IosAirplayStatus, String> {
    main_only(&window)?;
    #[cfg(target_os = "windows")]
    {
        let _gate=state.install_gate.lock().await;
        if running(&state)? {
            return Err("Stop iOS mirroring before updating its receiver component".into());
        }
        let root=engine_root(&app)?;
        if installed(&root) {drop(_gate);return status_for(&app,&state);}
        let client=reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(100)).build()
            .map_err(|e|format!("AirPlay download client error: {e}"))?;
        let response=client.get(ENGINE_RELEASE).send().await
            .map_err(|e|format!("Could not download AirPlay from GitHub: {e}"))?
            .error_for_status().map_err(|e|format!("AirPlay download HTTP error: {e}"))?;
        if response.content_length().is_some_and(|n|n>26_000_000) {
            return Err("AirPlay download exceeds the approved size".into());
        }
        let bytes=response.bytes().await.map_err(|e|e.to_string())?;
        if bytes.len()>26_000_000 {return Err("AirPlay download too large".into());}
        tokio::task::spawn_blocking(move || unpack_pinned_receiver(&bytes,&root))
            .await.map_err(|e|e.to_string())??;
        drop(_gate);
        status_for(&app,&state)
    }
    #[cfg(not(target_os = "windows"))]
    {let _=(app,state); Err("AirPlay receiver is available for Windows x64 only".into())}
}

#[tauri::command]
pub async fn ios_airplay_start(
    window: Window, app: AppHandle, state: State<'_, IosAirplayState>,
) -> Result<IosAirplayStatus, String> {
    main_only(&window)?;
    #[cfg(target_os = "windows")]
    {
        let _gate=state.install_gate.lock().await;
        let root=engine_root(&app)?;
        if !installed(&root) {return Err("Install the verified AirPlay receiver first".into());}
        if running(&state)? {drop(_gate);return status_for(&app,&state);}
        let child=std::process::Command::new(root.join("airplay-windows.exe"))
            .current_dir(&root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e|format!("Cannot launch AirPlay receiver: {e}"))?;
        *state.child.lock().map_err(|e|e.to_string())?=Some(child);
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        if !running(&state)? {
            return Err("AirPlay receiver quit immediately; check Windows firewall or antivirus".into());
        }
        drop(_gate);
        status_for(&app,&state)
    }
    #[cfg(not(target_os = "windows"))]
    {let _=(app,state); Err("AirPlay receiver requires Windows x64".into())}
}

#[tauri::command]
pub fn ios_airplay_stop(
    window: Window, app: AppHandle, state: State<'_, IosAirplayState>,
) -> Result<IosAirplayStatus, String> {
    main_only(&window)?;
    #[cfg(target_os = "windows")]
    {
        if let Some(mut child)=state.child.lock().map_err(|e|e.to_string())?.take() {
            if child.try_wait().map_err(|e|e.to_string())?.is_none() {
                child.kill().map_err(|e|format!("Cannot stop AirPlay receiver: {e}"))?;
                let _=child.wait();
            }
        }
        status_for(&app,&state)
    }
    #[cfg(not(target_os = "windows"))]
    {let _=state; status_for(&app,&state)}
}

#[tauri::command]
pub fn ios_airplay_open_source(window: Window, app: AppHandle) -> Result<(), String> {
    main_only(&window)?;
    use tauri_plugin_shell::ShellExt;
    #[allow(deprecated)]
    app.shell().open(ENGINE_SOURCE,None)
        .map_err(|e|format!("Cannot open AirPlay source: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn engine_pin_and_source_cannot_be_supplied_by_webview() {
        assert!(ENGINE_RELEASE.starts_with("https://github.com/moieric11/AirPlay-Windows/releases/download/v0.4.1/"));
        assert_eq!(ENGINE_ARCHIVE_SHA256.len(),64);
        assert_eq!(ENGINE_VERSION,"v0.4.1");
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn reject_zip_traversal_and_drive_paths() {
        for p in ["../evil.exe","C:/evil.exe","/tmp/evil","docs/../bad",
            "a\\b.exe","./fake","//x",""] {
            assert!(!safe_zip_member(p),"{p}");
        }
        for p in ["LICENSE","THIRD_PARTY_NOTICES.md","docs/README.md",
            "airplay-windows.exe","docs/"] {assert!(safe_zip_member(p),"{p}");}
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn altered_zip_is_never_extracted() {
        let folder=std::env::temp_dir().join("ml-airplay-no-install-test");
        let err=unpack_pinned_receiver(b"unverified exe",&folder).unwrap_err();
        assert!(err.contains("SHA-256 mismatch"));
        assert!(!folder.exists());
    }
}
