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
    pub(crate) child: Mutex<Option<Child>>,
    install_gate: tokio::sync::Mutex<()>,
    pub(crate) window_style: Mutex<String>,
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
    /// MirroLeap's next-launch layout preference, NOT an active iOS connection.
    pub compact_mode: bool,
    pub window_style: String,
    pub quality_tier: String,
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
/// MirroLeap-owned preference; the third-party receiver's settings file
/// contains other user preferences that must be preserved verbatim.
#[cfg(target_os="windows")]
fn preference_file(app:&AppHandle)->Result<std::path::PathBuf,String> {
    Ok(app.path().app_local_data_dir()
       .map_err(|e|format!("Cannot locate MirroLeap preferences: {e}"))?
       .join("ios-airplay").join("clean-view.txt"))
}
#[cfg(target_os="windows")]
fn prefer_compact(app:&AppHandle)->Result<bool,String> {
    let path=preference_file(app)?;
    match std::fs::read_to_string(&path) {
        Ok(text)=>Ok(text.trim()!="classic"),
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(true),
        Err(error)=>Err(format!("Cannot load iOS window preference: {error}")),
    }
}
#[cfg(target_os="windows")]
fn save_compact_preference(app:&AppHandle,compact:bool)->Result<(),String> {
    let path=preference_file(app)?;
    if let Some(parent)=path.parent() {
        std::fs::create_dir_all(parent).map_err(|e|format!("Cannot create iOS preference folder: {e}"))?;
    }
    std::fs::write(path,if compact {"compact"} else {"classic"})
        .map_err(|e|format!("Cannot save iOS window preference: {e}"))
}
/// Preserve the third-party receiver's independent name/codec/placement
/// settings; only set its documented chromeless key, and never force fullscreen.
fn valid_window_style(value:&str)->bool {
    matches!(value,"device-auto"|"device-iphone"|"device-ipad"|"clean"|"classic")
}
fn valid_quality_tier(value:&str)->bool {
    matches!(value,"balanced"|"high"|"ultra")
}
/// Upstream AirPlay-Windows uses the announced dimensions for iOS's /info
/// sender handshake; its default 2560×1440 envelope downsizes portrait.
/// Square capability ceilings let each device keep its own aspect ratio.
fn quality_cap(tier:&str)->Option<(u32,u32)> {
    match tier {
        "balanced"=>Some((1920,1920)),
        "high"=>Some((2868,2868)),
        "ultra"=>Some((3840,3840)),
        _=>None,
    }
}
#[cfg(target_os="windows")]
fn extra_pref(app:&AppHandle,name:&str)->Result<std::path::PathBuf,String> {
    Ok(app.path().app_local_data_dir().map_err(|e|e.to_string())?
        .join("ios-airplay").join(name))
}
#[cfg(target_os="windows")]
fn read_pref(app:&AppHandle,name:&str,default:&str,valid:fn(&str)->bool)->Result<String,String>{
    match std::fs::read_to_string(extra_pref(app,name)?) {
        Ok(value)=> {
            let value=value.trim();
            Ok(if valid(value){value.to_string()}else{default.to_string()})
        }
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(default.to_string()),
        Err(error)=>Err(format!("Cannot read iOS preference: {error}")),
    }
}
#[cfg(target_os="windows")]
fn save_pref(app:&AppHandle,name:&str,value:&str)->Result<(),String>{
    let path=extra_pref(app,name)?;
    if let Some(parent)=path.parent(){std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;}
    if path.symlink_metadata().is_ok_and(|m|m.file_type().is_symlink()) {
        return Err("iOS preference is a symbolic link".into());
    }
    std::fs::write(path,value).map_err(|e|e.to_string())
}
#[cfg(target_os="windows")]
fn prefer_window_style(app:&AppHandle)->Result<String,String>{
    // Keep an existing user's explicit original UI preference.
    let legacy=if prefer_compact(app)? {"device-auto"} else {"classic"};
    read_pref(app,"window-style.txt",legacy,valid_window_style)
}
#[cfg(target_os="windows")]
fn prefer_quality_tier(app:&AppHandle)->Result<String,String>{
    read_pref(app,"quality-tier.txt","high",valid_quality_tier)
}

fn update_receiver_ini(existing:&str,compact:bool)->String {
    let mut lines=Vec::new();
    let mut found=false;
    for line in existing.lines() {
        if line.split_once('=').is_some_and(|(key,_)|key.trim()=="chromeless") {
            if !found {
                lines.push(format!("chromeless={}",u8::from(compact)));
                found=true;
            }
        } else { lines.push(line.trim_end_matches('\r').to_owned()); }
    }
    if !found {lines.push(format!("chromeless={}",u8::from(compact)));}
    lines.join("\n")+"\n"
}
/// Change only upstream's advertised display ceiling and FPS. Never change
/// receiver name, saved window geometry, selected codec or hardware decoder.
fn update_receiver_quality_ini(existing:&str,tier:&str)->Result<String,String>{
    let (width,height)=quality_cap(tier).ok_or("Unsupported iOS quality tier")?;
    let mut lines=Vec::new();
    let mut found=std::collections::HashSet::new();
    let keys=[("mirror_width",width),("mirror_height",height),("max_fps",60)];
    for line in existing.lines(){
        if let Some((key,_))=line.split_once('=') {
            let key=key.trim();
            if let Some((_,value))=keys.iter().find(|(name,_)|*name==key){
                if found.insert(key.to_string()) {lines.push(format!("{key}={value}"));}
                continue;
            }
        }
        lines.push(line.trim_end_matches('\r').to_string());
    }
    for (key,value) in keys {if found.insert(key.to_string()) {lines.push(format!("{key}={value}"));}}
    Ok(lines.join("\n")+"\n")
}
#[cfg(target_os="windows")]
fn configure_receiver_window(app:&AppHandle,compact:bool,quality:&str)->Result<(),String> {
    // Upstream v0.4.1 uses FOLDERID_RoamingAppData / AirPlay-Windows /
    // settings.ini, loaded before SDL first creates its video window.
    let file=app.path().config_dir().map_err(|e|e.to_string())?
        .join("AirPlay-Windows").join("settings.ini");
    let old=match std::fs::read(&file) {
        Ok(bytes)=>{
            if bytes.len()>256*1024{return Err("AirPlay settings file is unexpectedly large".into());}
            String::from_utf8(bytes).map_err(|_|"AirPlay settings must be UTF-8".to_string())?
        }
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>String::new(),
        Err(error)=>return Err(format!("Cannot read AirPlay settings: {error}"))
    };
    let updated=update_receiver_quality_ini(&update_receiver_ini(&old,compact),quality)?;
    if updated==old {return Ok(());}
    if let Some(parent)=file.parent() {
        std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
    }
    if file.symlink_metadata().is_ok_and(|m|m.file_type().is_symlink()){
        return Err("AirPlay settings is a symbolic link; refusing to modify it".into());
    }
    std::fs::write(&file,updated).map_err(|e|format!("Cannot apply AirPlay view mode: {e}"))
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
            compact_mode: prefer_window_style(app)?!="classic",
            window_style: prefer_window_style(app)?,
            quality_tier: prefer_quality_tier(app)?,
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _=(app,state);
        Ok(IosAirplayStatus {
            supported: false, installed: false, running: false, installing: false,
            receiver_name: ENGINE_NAME.to_string(),
            engine_version: ENGINE_VERSION.to_string(),
            compact_mode: false,
            window_style: "clean".to_string(),
            quality_tier: "high".to_string(),
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
        let style=prefer_window_style(&app)?;
        configure_receiver_window(&app,style!="classic",&prefer_quality_tier(&app)?)?;
        let child=std::process::Command::new(root.join("airplay-windows.exe"))
            .current_dir(&root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e|format!("Cannot launch AirPlay receiver: {e}"))?;
        let pid=child.id();
        *state.child.lock().map_err(|e|e.to_string())?=Some(child);
        *state.window_style.lock().map_err(|e|e.to_string())?=style;
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        if !running(&state)? {
            return Err("AirPlay receiver quit immediately; check Windows firewall or antivirus".into());
        }
        // One passive, click-through native silhouette; it does not create
        // another user-facing settings page or capture the mirror's pixels.
        if let Err(error)=crate::ios_device_frame::start(&app,pid) {
            eprintln!("[iOS frame] Optional bezel unavailable: {error}");
        }
        drop(_gate);
        status_for(&app,&state)
    }
    #[cfg(not(target_os = "windows"))]
    {let _=(app,state); Err("AirPlay receiver requires Windows x64".into())}
}

/// Select the next launch layout without disconnecting an active iOS session.
/// AirPlay-Windows independently saves the 'H' shortcut state on shutdown.
#[tauri::command]
pub async fn ios_airplay_set_compact_mode(
    window:Window,app:AppHandle,state:State<'_,IosAirplayState>,compact:bool,
)->Result<IosAirplayStatus,String>{
    main_only(&window)?;
    #[cfg(target_os="windows")]
    {
        let _gate=state.install_gate.lock().await;
        if running(&state)? {
            return Err("Stop the receiver to change its default window layout".into());
        }
        save_compact_preference(&app,compact)?;
        save_pref(&app,"window-style.txt",if compact {"clean"}else{"classic"})?;
        drop(_gate);
        status_for(&app,&state)
    }
    #[cfg(not(target_os="windows"))]
    {let _=(app,state,compact);Err("Windows AirPlay receiver required".into())}
}

/// Switch clean/device styling from the homepage without an intermediate
/// modal. The classic upstream UI requires a receiver restart because H is
/// toggle-only and cannot be reliably read back over Win32 messages.
#[tauri::command]
pub async fn ios_airplay_set_window_style(
    window:Window,app:AppHandle,state:State<'_,IosAirplayState>,style:String,
)->Result<IosAirplayStatus,String>{
    main_only(&window)?;
    if !valid_window_style(&style) {return Err("Unsupported iOS window style".into());}
    #[cfg(target_os="windows")]
    {
        let _gate=state.install_gate.lock().await;
        if running(&state)? && (style=="classic" || prefer_window_style(&app)?=="classic") {
            return Err("Stop the receiver before switching to/from its original interface".into());
        }
        save_pref(&app,"window-style.txt",&style)?;
        save_compact_preference(&app,style!="classic")?;
        *state.window_style.lock().map_err(|e|e.to_string())?=style;
        drop(_gate);
        status_for(&app,&state)
    }
    #[cfg(not(target_os="windows"))]
    {let _=(app,state);Err("Windows AirPlay receiver required".into())}
}

#[tauri::command]
pub async fn ios_airplay_set_quality_tier(
    window:Window,app:AppHandle,state:State<'_,IosAirplayState>,tier:String,
)->Result<IosAirplayStatus,String>{
    main_only(&window)?;
    if !valid_quality_tier(&tier) {return Err("Unsupported iOS quality tier".into());}
    #[cfg(target_os="windows")]
    {
        let _gate=state.install_gate.lock().await;
        if running(&state)? {
            return Err("Stop the receiver before changing its advertised AirPlay resolution".into());
        }
        save_pref(&app,"quality-tier.txt",&tier)?;
        drop(_gate);
        status_for(&app,&state)
    }
    #[cfg(not(target_os="windows"))]
    {let _=(app,state);Err("Windows AirPlay receiver required".into())}
}

/// Send only two explicitly supported shortcuts to this process's OWN window,
/// never to another desktop application or the global foreground keyboard.
#[tauri::command]
pub fn ios_airplay_window_action(
    window:Window,app:AppHandle,state:State<'_,IosAirplayState>,action:&str,
)->Result<IosAirplayStatus,String>{
    main_only(&window)?;
    #[cfg(target_os="windows")]
    {
        use windows::Win32::Foundation::{WPARAM,LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW,WM_KEYDOWN,WM_KEYUP};
        let (virtual_key,scan_code):(usize,isize)=match action {
            "toggle_clean"=>(0x48,0x23), // upstream: H = Hide interface
            "fullscreen"=>(0x7A,0x57),   // upstream: F11 = fullscreen
            _=>return Err("Unsupported iOS receiver window action".into()),
        };
        if !running(&state)? {
            return Err("iOS receiver is not running".into());
        }
        let pid=state.child.lock().map_err(|e|e.to_string())?
            .as_ref().ok_or("iOS receiver process unavailable")?.id();
        let hwnd=crate::commands::find_window_by_pid(pid)
            .ok_or("Receiver video window is not ready; click it and press H or F11")?;
        let down=LPARAM(1|(scan_code<<16));
        let up=LPARAM(1|(scan_code<<16)|(1_isize<<30)|(1_isize<<31));
        unsafe {
            PostMessageW(Some(hwnd),WM_KEYDOWN,WPARAM(virtual_key),down)
                .map_err(|e|format!("Could not send receiver key: {e}"))?;
            PostMessageW(Some(hwnd),WM_KEYUP,WPARAM(virtual_key),up)
                .map_err(|e|format!("Could not release receiver key: {e}"))?;
        }
        // WM_KEYDOWN delivery is asynchronous, so don't claim the view
        // changed; the sender will show a request-queued feedback message.
        status_for(&app,&state)
    }
    #[cfg(not(target_os="windows"))]
    {let _=(app,state,action);Err("Windows AirPlay receiver required".into())}
}

#[tauri::command]
pub fn ios_airplay_stop(
    window: Window, app: AppHandle, state: State<'_, IosAirplayState>,
) -> Result<IosAirplayStatus, String> {
    main_only(&window)?;
    #[cfg(target_os = "windows")]
    {
        crate::ios_device_frame::close(&app);
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
    #[test]
    fn compact_settings_preserve_thirdparty_options_and_are_idempotent(){
        let sample="receiver_name=Office iPad\nmirror_hwaccel=1\nchromeless=0\nrecording_codec=1\n";
        let clean=update_receiver_ini(sample,true);
        assert!(clean.contains("chromeless=1\n"));
        assert!(clean.contains("receiver_name=Office iPad\n"));
        assert!(clean.contains("mirror_hwaccel=1\n"));
        assert!(clean.contains("recording_codec=1\n"));
        assert_eq!(clean,update_receiver_ini(&clean,true));
        let classic=update_receiver_ini(&clean,false);
        assert!(classic.contains("chromeless=0"));
        assert_eq!(update_receiver_ini("",true),"chromeless=1\n");
        assert_eq!(update_receiver_ini("chromeless=1\nchromeless=0\n",true),"chromeless=1\n");
    }
    #[test]
    fn compact_never_forces_fullscreen(){
        let value=update_receiver_ini("fullscreen=0\nmax_fps=60",true);
        assert!(value.contains("fullscreen=0\n"));
        assert!(value.contains("max_fps=60\n"));
    }
    #[test]
    fn portrait_resolution_envelopes_preserve_sender_aspect_and_unrelated_options(){
        let original="receiver_name=Office iPad\nmirror_width=1920\nmirror_height=1080\nmax_fps=30\nmirror_hwaccel=0\nchromeless=1\nrecording_codec=1\n";
        for (tier,side) in [("balanced",1920),("high",2868),("ultra",3840)] {
            let updated=update_receiver_quality_ini(original,tier).unwrap();
            assert!(updated.contains(&format!("mirror_width={side}\n")));
            assert!(updated.contains(&format!("mirror_height={side}\n")));
            assert!(updated.contains("max_fps=60\n"));
            assert!(updated.contains("receiver_name=Office iPad\n"));
            assert!(updated.contains("mirror_hwaccel=0\n"));
            assert!(updated.contains("recording_codec=1\n"));
            assert_eq!(updated,update_receiver_quality_ini(&updated,tier).unwrap());
        }
        assert!(update_receiver_quality_ini(original,"untrusted").is_err());
        assert!(!valid_window_style("../../malicious"));
        assert!(!valid_quality_tier("40000"));
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
