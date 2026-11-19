//! Opt-in Windows AirPlay receiver adapter (not an Android/scrcpy backend).
//! Downloads a PINNED third-party GPL-3.0 binary only after a user click.
//! No unverified executables, arbitrary URLs or paths are accepted from JS.
use serde::Serialize;
use std::process::Child;
use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};
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
    /// A previous MirroLeap instance may have left the exact verified
    /// receiver running. Keep its PID separately; std::process::Child only
    /// represents newly spawned processes in THIS process.
    adopted_pid: Mutex<Option<u32>>,
    /// Prevent status polling from repeatedly recreating a failed bezel
    /// around a recovered orphaned AirPlay process.
    frame_attempted_pid: Mutex<Option<u32>>,
    /// Windows kernel job object closes on any process exit, even crashes,
    /// automatically killing the assigned AirPlay receiver.
    #[cfg(target_os="windows")]
    receiver_job: Mutex<Option<std::os::windows::io::OwnedHandle>>,
    install_gate: tokio::sync::Mutex<()>,
    pub(crate) window_style: Mutex<String>,
    pub(crate) frame_attached: AtomicBool,
    /// Sanitized reason for a missing bezel; never includes LAN/IP/path data.
    pub(crate) frame_error: Mutex<Option<String>>,
    /// Verified decoded texture dimensions from THIS receiver process only.
    /// The advertised AirPlay resolution is a ceiling, not actual pixels.
    source_pixels: Mutex<Option<(u32,u32)>>,
}
impl Drop for IosAirplayState {
    fn drop(&mut self) {
        #[cfg(target_os="windows")]
        if let Ok(mut job) = self.receiver_job.lock() {
            // Kill-on-close job makes receiver cleanup OS-enforced.
            job.take();
        }
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
    pub source_width: Option<u32>,
    pub source_height: Option<u32>,
    pub display_width: Option<u32>,
    pub display_height: Option<u32>,
    /// True only while the native mouse-through rim is tracking the SDL
    /// video client; it is NOT a cross-process child WebView.
    pub frame_attached: bool,
    pub frame_error: Option<String>,
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
fn running(app:&AppHandle,state: &IosAirplayState) -> Result<bool, String> {
    let path=engine_root(app)?.join("airplay-windows.exe");
    if !installed(&engine_root(app)?) {
        *state.adopted_pid.lock().map_err(|e|e.to_string())?=None;
        return Ok(false);
    }
    let mut process = state.child.lock().map_err(|e| e.to_string())?;
    if let Some(child) = process.as_mut() {
        match child.try_wait() {
            Ok(None) => return Ok(true),
            Ok(Some(_)) => {
                *process = None;
                // Job has no living original process, release it.
                if let Ok(mut job)=state.receiver_job.lock(){job.take();}
            }
            Err(e) => return Err(format!("Cannot inspect AirPlay receiver process: {e}")),
        }
    }
    drop(process);
    // Handle AirPlay-Windows's single-instance semantics: a second invocation
    // exits 0 while the original instance stays alive. Only adopt an exact
    // path match to MirroLeap's pinned, hash-verified component directory.
    let tracked=*state.adopted_pid.lock().map_err(|e|e.to_string())?;
    if let Some(pid)=tracked {
        if crate::ios_receiver_process::verified_pid_alive(pid,&path){return Ok(true);}
    }
    let recovered=crate::ios_receiver_process::verified_receiver_pid(&path);
    *state.adopted_pid.lock().map_err(|e|e.to_string())?=recovered;
    if let Some(pid)=recovered {
        let mut job=state.receiver_job.lock().map_err(|e|e.to_string())?;
        if job.is_none(){
            if let Ok(handle)=crate::ios_receiver_process::kill_on_exit_pid(pid,&path) {
                *job=Some(handle);
            }
        }
    }
    Ok(recovered.is_some())
}
pub(crate) fn active_receiver_pid(state:&IosAirplayState)->Option<u32>{
    state.child.lock().ok().and_then(|child|child.as_ref().map(|c|c.id()))
        .or_else(||state.adopted_pid.lock().ok().and_then(|p|*p))
}
/// AirPlay-Windows v0.4.1 logs the actual decoded texture when --log is set:
/// "VideoRenderer texture (NV12) created 1640x2360". This is distinct from
/// the square resolution limit we advertise in /info. Collect ONLY numbers;
/// never store/forward receiver logs containing sender names or LAN IPs.
fn decoded_texture_from_line(line:&str)->Option<(u32,u32)> {
    let raw=line.split("VideoRenderer texture (").nth(1)?
        .split(") created ").nth(1)?
        .split_whitespace().next()?;
    let (w,h)=raw.split_once('x')?;
    let w=w.parse::<u32>().ok()?;
    let h=h.parse::<u32>().ok()?;
    (w>=320 && h>=240 && w<=7680 && h<=7680).then_some((w,h))
}
fn status_for(app: &AppHandle, state: &IosAirplayState) -> Result<IosAirplayStatus, String> {
    #[cfg(target_os = "windows")]
    {
        let root=engine_root(app)?;
        let is_running=running(app,state)?;
        let source=if is_running {
            *state.source_pixels.lock().map_err(|e|e.to_string())?
        } else {None};
        let display=if is_running {
            active_receiver_pid(state).and_then(crate::ios_device_frame::receiver_client_size)
        }else{None};
        Ok(IosAirplayStatus {
            supported: true,
            installed: installed(&root),
            running: is_running,
            installing: state.install_gate.try_lock().is_err(),
            // Users may change this in the receiver's OPTIONS > Receiver Name.
            receiver_name: ENGINE_NAME.to_string(),
            engine_version: ENGINE_VERSION.to_string(),
            compact_mode: prefer_window_style(app)?!="classic",
            window_style: prefer_window_style(app)?,
            quality_tier: prefer_quality_tier(app)?,
            source_width: source.map(|(width,_)|width),
            source_height: source.map(|(_,height)|height),
            display_width: display.map(|(width,_)|width),
            display_height: display.map(|(_,height)|height),
            frame_attached: is_running && state.frame_attached.load(Ordering::Acquire),
            frame_error: if is_running {
                state.frame_error.lock().map_err(|e|e.to_string())?.clone()
            } else {None},
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
            source_width: None, source_height:None,
            display_width:None, display_height:None,
            frame_attached: false,
            frame_error: None,
        })
    }
}

#[tauri::command]
pub fn ios_airplay_status(
    window: Window, app: AppHandle, state: State<'_, IosAirplayState>,
) -> Result<IosAirplayStatus, String> {
    main_only(&window)?;
    #[cfg(target_os="windows")]
    {
        let current=status_for(&app,&state)?;
        if current.running && current.installed {
            if let Some(pid)=active_receiver_pid(&state){
                let mut attempted=state.frame_attempted_pid.lock().map_err(|e|e.to_string())?;
                if *attempted!=Some(pid) && !crate::ios_device_frame::active_for(pid){
                    *attempted=Some(pid);
                    drop(attempted);
                    *state.window_style.lock().map_err(|e|e.to_string())?=
                        prefer_window_style(&app)?;
                    if let Err(error)=crate::ios_device_frame::start(&app,pid){
                        if let Ok(mut reason)=state.frame_error.lock(){
                            *reason=Some(format!("无法重新连接真机边框：{error}"));
                        }
                    }
                }
            }
        }
    }
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
        if running(&app,&state)? {
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
        if running(&app,&state)? {
            // MirroLeap can resume control of its verified receiver even
            // when an older GUI version exited without killing the stream.
            if active_receiver_pid(&state).is_some_and(|pid|
                !crate::ios_device_frame::active_for(pid)){
                if let Some(pid)=active_receiver_pid(&state){
                    *state.window_style.lock().map_err(|e|e.to_string())?=
                        prefer_window_style(&app)?;
                    if let Err(e)=crate::ios_device_frame::start(&app,pid){
                        eprintln!("[iOS frame] Could not attach recovered receiver: {e}");
                    }
                }
            }
            drop(_gate);
            return status_for(&app,&state);
        }
        let style=prefer_window_style(&app)?;
        configure_receiver_window(&app,style!="classic",&prefer_quality_tier(&app)?)?;
        // Capture only the receiver's documented video texture dimensions.
        // The upstream --log switch does NOT create a console if stderr is
        // piped; unfiltered logs may contain private LAN details, so never
        // persist them or surface raw lines in the UI.
        let mut child=std::process::Command::new(root.join("airplay-windows.exe"))
            .arg("--log")
            .current_dir(&root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e|format!("Cannot launch AirPlay receiver: {e}"))?;
        let pid=child.id();
        let stderr=child.stderr.take();
        // A kernel job with KILL_ON_JOB_CLOSE means a Windows crash or
        // forced termination of MirroLeap cannot leave its receiver orphaned.
        let job=match crate::ios_receiver_process::kill_on_exit_job(&child) {
            Ok(job)=>Some(job),
            Err(error)=>{
                eprintln!("[iOS receiver] Job assignment unavailable; using explicit shutdown cleanup: {error}");
                None
            },
        };
        *state.receiver_job.lock().map_err(|e|e.to_string())?=job;
        *state.adopted_pid.lock().map_err(|e|e.to_string())?=None;
        *state.frame_attempted_pid.lock().map_err(|e|e.to_string())?=Some(pid);
        *state.source_pixels.lock().map_err(|e|e.to_string())?=None;
        *state.child.lock().map_err(|e|e.to_string())?=Some(child);
        *state.window_style.lock().map_err(|e|e.to_string())?=style;
        if let Some(stderr)=stderr {
            let app_logs=app.clone();
            std::thread::spawn(move || {
                use std::io::{BufRead,BufReader};
                for line in BufReader::new(stderr).lines() {
                    let Ok(line)=line else {break};
                    let dimension=decoded_texture_from_line(&line);
                    let cleared=line.contains("VideoRenderer clear_session:");
                    if dimension.is_none() && !cleared {continue;}
                    let state=app_logs.state::<IosAirplayState>();
                    let is_current=state.child.lock().ok().and_then(
                        |child|child.as_ref().map(|active|active.id()==pid)
                    ).unwrap_or(false);
                    if is_current {
                        if let Ok(mut pixels)=state.source_pixels.lock(){
                            *pixels=if cleared {None} else {dimension};
                        }
                    }
                }
            });
        }
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        if !running(&app,&state)? {
            return Err("AirPlay-Windows exited. Another receiver may already be running outside MirroLeap; close that AirPlay window with Alt+F4 and try again.".into());
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
        if running(&app,&state)? {
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
        if running(&app,&state)? && (style=="classic" || prefer_window_style(&app)?=="classic") {
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
        if running(&app,&state)? {
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
        if !running(&app,&state)? {
            return Err("iOS receiver is not running".into());
        }
        let pid=active_receiver_pid(&state)
            .ok_or("iOS receiver process unavailable")?;
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

#[cfg(target_os = "windows")]
fn stop_owned_receiver(app:&AppHandle,state:&IosAirplayState)->Result<(),String>{
    // Close the child bezel BEFORE terminating the third-party SDL parent.
    // There must never be a stale WebView2 HWND pointing to dead video.
    crate::ios_device_frame::close(app);
    *state.source_pixels.lock().map_err(|e|e.to_string())?=None;
    state.frame_attached.store(false,Ordering::Release);
    let child=state.child.lock().map_err(|e|e.to_string())?.take();
    let adopted=state.adopted_pid.lock().map_err(|e|e.to_string())?.take();
    *state.frame_attempted_pid.lock().map_err(|e|e.to_string())?=None;
    let mut error=None;
    if let Some(mut child)=child {
        match child.try_wait() {
            Ok(None)=>{
                if let Err(e)=child.kill(){
                    error=Some(format!("Cannot stop AirPlay receiver: {e}"));
                }
                let _=child.wait();
            }
            Ok(Some(_))=>{},
            Err(e)=>{
                error=Some(format!("Cannot inspect AirPlay process: {e}"));
                let _=child.kill();
                let _=child.wait();
            }
        }
    }
    let expected=engine_root(app)?.join("airplay-windows.exe");
    // A prior MirroLeap version may have orphaned a pinned receiver. A
    // verified exact executable path is required before killing any PID.
    let matching=adopted.or_else(||crate::ios_receiver_process::verified_receiver_pid(&expected));
    if let Some(pid)=matching {
        if let Err(e)=crate::ios_receiver_process::stop_verified_pid(pid,&expected){
            error=Some(e);
        }
    }
    // Dropping the job is an extra OS-enforced fallback for its assigned PID.
    if let Ok(mut job)=state.receiver_job.lock(){job.take();}
    error.map_or(Ok(()),Err)
}

/// Called both on main-window close and RunEvent::Exit. Unlike relying on
/// Drop alone, a normal Tauri shutdown now stops the pinned AirPlay receiver.
#[cfg(target_os = "windows")]
pub fn shutdown(app:&AppHandle){
    if let Some(state)=app.try_state::<IosAirplayState>(){
        if let Err(error)=stop_owned_receiver(app,&state){
            eprintln!("[iOS receiver] Shutdown cleanup failed: {error}");
        }
    }
}

#[tauri::command]
pub fn ios_airplay_stop(
    window: Window, app: AppHandle, state: State<'_, IosAirplayState>,
) -> Result<IosAirplayStatus, String> {
    main_only(&window)?;
    #[cfg(target_os = "windows")]
    {
        stop_owned_receiver(&app,&state)?;
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
    #[test]
    fn actual_decoder_dimensions_are_strictly_extracted_without_exposing_logs(){
        assert_eq!(decoded_texture_from_line(
            "[12:30:01.235] INFO  VideoRenderer texture (NV12) created 1640x2360"),
            Some((1640,2360)));
        assert_eq!(decoded_texture_from_line(
            "[12:30:02.100] INFO  VideoRenderer texture (GPU BGRA) created 2048x2732"),
            Some((2048,2732)));
        assert_eq!(decoded_texture_from_line(
            "[12:31:00.123] INFO  VideoRenderer clear_session: reset cover"),None);
        assert_eq!(decoded_texture_from_line(
            "[12:30] INFO receiver 192.168.1.22 name=Office"),None);
        assert_eq!(decoded_texture_from_line(
            "VideoRenderer texture (NV12) created 999999999x1"),None);
        assert_eq!(decoded_texture_from_line(
            "VideoRenderer texture (NV12) created garbage"),None);
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
