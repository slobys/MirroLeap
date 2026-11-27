//! Opt-in Windows AirPlay receiver adapter (not an Android/scrcpy backend).
//! Downloads a PINNED third-party GPL-3.0 binary only after a user click.
//! No unverified executables, arbitrary URLs or paths are accepted from JS.
use serde::Serialize;
use std::net::IpAddr;
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
    pub(crate) display_size: Mutex<DisplaySize>,
    pub(crate) landscape_camera_right: Mutex<bool>,
    active_quality_tier: Mutex<Option<String>>,
    pub(crate) frame_attached: AtomicBool,
    /// Exact SDL window of a newly started receiver. Adopted processes have
    /// no stdout/stderr channel and keep their own presentation state.
    #[cfg(target_os="windows")]
    pub(crate) managed_window: Mutex<Option<(u32,usize)>>,
    #[cfg(target_os="windows")]
    connection: Mutex<ReceiverConnection>,
    /// Sanitized reason for a missing bezel; never includes LAN/IP/path data.
    pub(crate) frame_error: Mutex<Option<String>>,
    /// Verified decoded texture dimensions from THIS receiver process only.
    /// The advertised AirPlay resolution is a ceiling, not actual pixels.
    pub(crate) source_pixels: Mutex<Option<(u32,u32)>>,
    last_display_size: Mutex<Option<(u32,u32)>>,
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
    pub next_receiver_name: Option<String>,
    pub receiver_ip: Option<IpAddr>,
    pub receiver_ready: bool,
    pub engine_version: String,
    /// MirroLeap's next-launch layout preference, NOT an active iOS connection.
    pub compact_mode: bool,
    pub window_style: String,
    pub quality_tier: String,
    pub window_size: String,
    pub quality_pending: bool,
    pub usb_status: &'static str,
    pub usb_source_width: Option<u32>,
    pub usb_source_height: Option<u32>,
    pub diagnostics: crate::ios_diagnostics::Status,
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
    matches!(value,"adaptive"|"balanced"|"high"|"ultra")
}

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub(crate) enum DisplaySize{Small,#[default] Adaptive,Medium,Large,Source}

fn valid_window_size(value:&str)->bool{
    matches!(value,"auto"|"small"|"medium"|"large"|"source")
}

fn resolve_display_size(size:&str,tier:&str)->DisplaySize{
    match (size,tier){
        ("source",_)=>DisplaySize::Source,
        ("small",_)|("auto","balanced")=>DisplaySize::Small,
        ("large",_)|("auto","ultra")=>DisplaySize::Large,
        ("auto","adaptive")=>DisplaySize::Adaptive,
        _=>DisplaySize::Medium,
    }
}

#[cfg(target_os="windows")]
pub(crate) fn load_usb_display_size(app:&AppHandle)->Result<(),String>{
    let state=app.state::<IosAirplayState>();
    let size=resolve_display_size(&prefer_window_size(app)?,&prefer_quality_tier(app)?);
    *state.display_size.lock().map_err(|error|error.to_string())?=size;
    Ok(())
}

#[cfg(target_os="windows")]
pub(crate) fn load_camera_position(app:&AppHandle)->Result<(),String>{
    let state=app.state::<IosAirplayState>();
    let mut right=state.landscape_camera_right.lock().map_err(|error|error.to_string())?;
    *right=read_pref(app,"camera-edge.txt","left",|value| matches!(value,"left"|"right"))?=="right";
    Ok(())
}

#[cfg(target_os="windows")]
pub(crate) fn flip_landscape_camera(app:&AppHandle)->Result<bool,String>{
    let state=app.state::<IosAirplayState>();
    let mut right=state.landscape_camera_right.lock().map_err(|error|error.to_string())?;
    let next=!*right;
    save_pref(app,"camera-edge.txt",if next{"right"}else{"left"})?;
    *right=next;
    Ok(next)
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
/// Use ~3% headroom over the *actual last SDL video client*, rather than
/// asking iOS to send 3840px for a ~1800px window. Earlier 12% headroom
/// still left >1.15x scale after rounding; 1:1-ish is more appropriate for
/// upstream SDL2's single bilinear pass, especially for high-contrast icons.
fn adaptive_quality_cap(display:Option<(u32,u32)>)->u32{
    let longest=display.and_then(|(w,h)|
        (w>=320 && h>=240 && w<=7680 && h<=7680).then_some(w.max(h))
    ).unwrap_or(1830);
    let desired=((longest as u64*103+99)/100).clamp(1280,3840) as u32;
    ((desired+63)/64*64).min(3840)
}
fn parse_last_display(text:&str)->Option<(u32,u32)>{
    let (w,h)=text.trim().split_once('x')?;
    if !w.chars().all(|c|c.is_ascii_digit()) ||
        !h.chars().all(|c|c.is_ascii_digit()){return None;}
    let w=w.parse().ok()?;
    let h=h.parse().ok()?;
    (w>=320 && h>=240 && w<=7680 && h<=7680).then_some((w,h))
}
#[cfg(target_os="windows")]
fn extra_pref(app:&AppHandle,name:&str)->Result<std::path::PathBuf,String> {
    Ok(app.path().app_local_data_dir().map_err(|e|e.to_string())?
        .join("ios-airplay").join(name))
}
#[cfg(target_os="windows")]
pub(crate) fn read_pref(app:&AppHandle,name:&str,default:&str,valid:fn(&str)->bool)->Result<String,String>{
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
pub(crate) fn save_pref(app:&AppHandle,name:&str,value:&str)->Result<(),String>{
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
    read_pref(app,"quality-tier.txt","adaptive",valid_quality_tier)
}

#[cfg(target_os="windows")]
fn prefer_window_size(app:&AppHandle)->Result<String,String>{
    read_pref(app,"window-size.txt","auto",valid_window_size)
}

fn valid_receiver_name(name:&str)->bool{
    // AirPlay-Windows reserves 13 bytes of the RAOP DNS-SD label for its
    // device ID prefix. Match the receiver's UTF-8 and label restrictions.
    !name.is_empty() && !name.chars().all(char::is_whitespace) && name.len()<=50 &&
        !name.chars().any(|character|character.is_control() || matches!(character,'.'|'\\'))
}

#[cfg(target_os="windows")]
fn prefer_receiver_name(app:&AppHandle)->Result<Option<String>,String>{
    let name=read_pref(app,"receiver-name.txt","",valid_receiver_name)?;
    Ok((!name.is_empty()).then_some(name))
}

fn receiver_name_from_ini(settings:&str)->Option<&str>{
    settings.lines().filter_map(|line|line.strip_prefix("receiver_name="))
        .map(|name|name.trim_matches(' ')).filter(|name|valid_receiver_name(name)).last()
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
/// Update the advertised display ceiling and FPS without touching other keys.
fn update_receiver_quality_ini(existing:&str,tier:&str)->Result<String,String>{
    let (width,height)=quality_cap(tier).ok_or("Unsupported iOS quality tier")?;
    Ok(update_receiver_quality_dims_ini(existing,width,height))
}
fn update_receiver_quality_dims_ini(existing:&str,width:u32,height:u32)->String{
    update_receiver_keys(existing,&[("mirror_width",width),("mirror_height",height),("max_fps",60)])
}

pub(crate) const WAITING_PHONE_SIZE:(u32,u32)=(360,780);

fn update_receiver_startup_ini(existing:&str)->String{
    update_receiver_keys(existing,&[
        ("window_placement_valid",1),("window_width",WAITING_PHONE_SIZE.0),
        ("window_height",WAITING_PHONE_SIZE.1),("fullscreen",0),
    ])
}

fn update_receiver_keys<T:std::fmt::Display>(existing:&str,keys:&[(&str,T)])->String{
    let mut lines=Vec::new();
    let mut found=std::collections::HashSet::new();
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
    lines.join("\n")+"\n"
}
#[cfg(target_os="windows")]
fn receiver_settings_file(app:&AppHandle)->Result<std::path::PathBuf,String>{
    // Upstream v0.4.1 uses FOLDERID_RoamingAppData / AirPlay-Windows /
    // settings.ini, loaded before SDL first creates its video window.
    Ok(app.path().config_dir().map_err(|e|e.to_string())?
        .join("AirPlay-Windows").join("settings.ini"))
}

#[cfg(target_os="windows")]
fn read_receiver_settings(file:&std::path::Path)->Result<String,String>{
    match std::fs::read(file) {
        Ok(bytes)=>{
            if bytes.len()>256*1024{return Err("AirPlay settings file is unexpectedly large".into());}
            String::from_utf8(bytes).map_err(|_|"AirPlay settings must be UTF-8".to_string())
        }
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(String::new()),
        Err(error)=>Err(format!("Cannot read AirPlay settings: {error}"))
    }
}

#[cfg(target_os="windows")]
fn configure_receiver_window(app:&AppHandle,compact:bool,quality:&str)->Result<(),String> {
    let file=receiver_settings_file(app)?;
    let old=read_receiver_settings(&file)?;
    let mut base=update_receiver_ini(&old,compact);
    if compact{base=update_receiver_startup_ini(&base);}
    if let Some(name)=prefer_receiver_name(app)?{
        base=update_receiver_keys(&base,&[("receiver_name",name.as_str())]);
    }
    let updated=if quality=="adaptive"{
        let last=std::fs::read_to_string(extra_pref(app,"last-display-size.txt")?)
            .ok().and_then(|text|parse_last_display(&text));
        let side=adaptive_quality_cap(last);
        update_receiver_quality_dims_ini(&base,side,side)
    }else{
        update_receiver_quality_ini(&base,quality)?
    };
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
const SDL_ADAPTER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/airplay-sdl-background.dll"));
#[cfg(target_os = "windows")]
const SDL_ORIGINAL_SHA256: &str =
    "d698174b2370588935acbe2847806cb340ef70597b009c490fde8c7b5efecad4";

#[cfg(target_os = "windows")]
fn background_runtime(root: &std::path::Path) -> Option<std::path::PathBuf> {
    // Keep the path already authorized by Windows for the v0.4.1 receiver.
    // A DLL rebuild only changes the DLL, not which executable needs access.
    (!SDL_ADAPTER.is_empty()).then(||root.with_file_name("v0.4.1-background-d2d9735f398f015d"))
}

#[cfg(target_os = "windows")]
fn prepare_receiver_executable(root: &std::path::Path) -> Result<std::path::PathBuf, String> {
    use sha2::{Digest, Sha256};
    use std::fs;

    let Some(runtime) = background_runtime(root) else {
        return Ok(root.join("airplay-windows.exe"));
    };
    let executable = runtime.join("airplay-windows.exe");
    let original_sdl = root.join("SDL2.dll");
    let valid_runtime = || {
        executable.is_file()
            && runtime.join("SDL2_original.dll").is_file()
            && fs::read(runtime.join("SDL2.dll")).is_ok_and(|bytes| bytes == SDL_ADAPTER)
    };
    if valid_runtime() {
        return Ok(executable);
    }
    let original = fs::read(original_sdl).map_err(|e| format!("Cannot read the AirPlay SDL library: {e}"))?;
    if format!("{:x}", Sha256::digest(&original)) != SDL_ORIGINAL_SHA256 {
        return Err("The AirPlay SDL library does not match the verified receiver release".into());
    }
    // Keep user settings, pairing identity and recordings in the existing
    // working directory. The parallel runtime contains only the pinned binaries.
    let binaries = [
        "airplay-windows.exe", "avcodec-62.dll", "avformat-62.dll", "avutil-60.dll",
        "brotlicommon.dll", "brotlidec.dll", "bz2.dll", "freetype.dll", "libcrypto-3-x64.dll",
        "libpng16.dll", "plist-2.0.dll", "SDL2_ttf.dll", "swscale-9.dll", "zlib1.dll",
    ];
    if binaries.iter().all(|name|runtime.join(name).is_file())
        && fs::read(runtime.join("SDL2_original.dll")).is_ok_and(|bytes|bytes==original)
    {
        let adapter = runtime.join("SDL2.dll");
        let temporary = runtime.join(format!("SDL2-{}.tmp",std::process::id()));
        fs::write(&temporary, SDL_ADAPTER).map_err(|e|format!("Cannot stage the AirPlay adapter: {e}"))?;
        let result = fs::rename(&temporary, &adapter);
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(format!("Cannot update the AirPlay adapter: {error}"));
        }
        return Ok(executable);
    }
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?.as_nanos();
    let staging = runtime.with_extension(format!("staging-{}-{timestamp}", std::process::id()));
    fs::create_dir(&staging).map_err(|e| format!("Cannot stage background AirPlay runtime: {e}"))?;
    let result = (|| {
        for name in binaries {
            fs::copy(root.join(name), staging.join(name))?;
        }
        fs::write(staging.join("SDL2_original.dll"), original)?;
        fs::write(staging.join("SDL2.dll"), SDL_ADAPTER)?;
        fs::rename(&staging, &runtime)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    if valid_runtime() {
        return Ok(executable);
    }
    Err(result.err().map_or_else(
        || "Background AirPlay runtime is incomplete".into(),
        |error| format!("Cannot prepare background AirPlay runtime: {error}"),
    ))
}

#[cfg(target_os = "windows")]
fn verified_receiver(
    root: &std::path::Path, tracked: Option<u32>,
) -> Option<(u32, std::path::PathBuf)> {
    background_runtime(root).into_iter().chain(std::iter::once(root.to_path_buf()))
        .map(|directory| directory.join("airplay-windows.exe"))
        .find_map(|path| {
            let pid = tracked.filter(|pid| crate::ios_receiver_process::verified_pid_alive(*pid, &path))
                .or_else(|| crate::ios_receiver_process::verified_receiver_pid(&path))?;
            Some((pid, path))
        })
}

#[cfg(target_os = "windows")]
fn running(app:&AppHandle,state: &IosAirplayState) -> Result<bool, String> {
    let root = engine_root(app)?;
    if !installed(&root) {
        *state.adopted_pid.lock().map_err(|e|e.to_string())?=None;
        return Ok(false);
    }
    let mut process = state.child.lock().map_err(|e| e.to_string())?;
    if let Some(child) = process.as_mut() {
        match child.try_wait() {
            Ok(None) => return Ok(true),
            Ok(Some(_)) => {
                *process = None;
                *state.managed_window.lock().map_err(|e|e.to_string())?=None;
                *state.source_pixels.lock().map_err(|e|e.to_string())?=None;
                *state.connection.lock().map_err(|e|e.to_string())?=ReceiverConnection::default();
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
    let recovered = verified_receiver(&root, tracked);
    *state.adopted_pid.lock().map_err(|e|e.to_string())?=recovered.as_ref().map(|(pid, _)| *pid);
    if let Some((pid, path)) = recovered.as_ref() {
        let mut job=state.receiver_job.lock().map_err(|e|e.to_string())?;
        if job.is_none(){
            if let Ok(handle)=crate::ios_receiver_process::kill_on_exit_pid(*pid,path) {
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

#[cfg(any(target_os="windows",test))]
#[derive(Default)]
struct ReceiverConnection {
    name:Option<String>,
    ip:Option<IpAddr>,
    ready:bool,
}

#[cfg(any(target_os="windows",test))]
impl ReceiverConnection {
    fn read_startup_log(&mut self,line:&str){
        // The pinned receiver logs its own name/address at startup. Ignore
        // arbitrary messages and never forward raw logs or sender addresses.
        if self.ready{return;}
        let Some((stamp,body))=line.split_once("] INFO   ") else{return;};
        if !stamp.starts_with('[') || stamp.len()!=13{return;}
        if let Some(name)=body.strip_prefix("name="){
            let name=name.trim();
            if valid_receiver_name(name){
                self.name=Some(name.into());
            }
        }else if let Some(address)=body.strip_prefix("ip=").or_else(||body.strip_prefix("ipv6=")){
            if let Ok(ip)=address.parse::<IpAddr>(){
                if !ip.is_unspecified() && !ip.is_loopback() && !ip.is_multicast() &&
                    (self.ip.is_none() || ip.is_ipv4()){
                    self.ip=Some(ip);
                }
            }
        }else if body=="Ready. Close the window or press Ctrl-C to exit."{
            self.ready=true;
        }
    }
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
        if let Some(size)=display.filter(|_|source.is_some()) {
            let mut observed=state.last_display_size.lock().map_err(|e|e.to_string())?;
            if *observed!=Some(size) {
                // Persist only this user's window dimensions, not sender IP,
                // filename, contents or raw AirPlay traffic.
                if save_pref(app,"last-display-size.txt",
                    &format!("{}x{}",size.0,size.1)).is_ok(){
                    *observed=Some(size);
                }
            }
        }
        let quality=prefer_quality_tier(app)?;
        let usb_source=crate::ios_usb::video().map(|(_,pixels)|pixels);
        let airplay_phase = if source.is_some() { "received" } else if is_running { "waiting" } else { "idle" };
        crate::ios_diagnostics::observe(crate::ios_diagnostics::Channel::AirplayVideo, airplay_phase, None);
        let quality_pending=is_running && state.active_quality_tier.lock()
            .map_err(|e|e.to_string())?.as_ref().is_some_and(|active|active!=&quality);
        let connection=state.connection.lock().map_err(|e|e.to_string())?;
        let adopted=is_running && state.adopted_pid.lock().map_err(|e|e.to_string())?.is_some();
        let preferred=prefer_receiver_name(app)?;
        let ini_name=||{
            let settings=read_receiver_settings(&receiver_settings_file(app).ok()?).ok()?;
            receiver_name_from_ini(&settings).map(str::to_owned)
        };
        let receiver_name=if is_running{
            connection.name.clone().or_else(ini_name).or_else(||preferred.clone())
        }else{
            preferred.clone().or_else(ini_name)
        }
            .unwrap_or_else(||ENGINE_NAME.to_string());
        let next_receiver_name=preferred.filter(|name|is_running && name!=&receiver_name);
        Ok(IosAirplayStatus {
            supported: true,
            installed: installed(&root),
            running: is_running,
            installing: state.install_gate.try_lock().is_err(),
            receiver_name,
            next_receiver_name,
            receiver_ip:connection.ip.filter(|_|is_running),
            receiver_ready:is_running && (connection.ready || adopted),
            engine_version: ENGINE_VERSION.to_string(),
            compact_mode: prefer_window_style(app)?!="classic",
            window_style: prefer_window_style(app)?,
            quality_tier: quality,
            window_size: prefer_window_size(app)?,
            quality_pending,
            usb_status: crate::ios_usb::status(),
            usb_source_width: usb_source.map(|(width,_)|width),
            usb_source_height: usb_source.map(|(_,height)|height),
            diagnostics: crate::ios_diagnostics::status(),
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
            next_receiver_name:None,
            receiver_ip:None,receiver_ready:false,
            engine_version: ENGINE_VERSION.to_string(),
            compact_mode: false,
            window_style: "clean".to_string(),
            quality_tier: "high".to_string(),
            window_size: "auto".to_string(), quality_pending:false, usb_status:"not_bundled",
            usb_source_width:None, usb_source_height:None,
            diagnostics: crate::ios_diagnostics::Status::default(),
            source_width: None, source_height:None,
            display_width:None, display_height:None,
            frame_attached: false,
            frame_error: None,
        })
    }
}

#[tauri::command]
pub async fn ios_airplay_export_diagnostics(
    window: Window, app: AppHandle, state: State<'_, IosAirplayState>,
) -> Result<String, String> {
    main_only(&window)?;
    let _ = status_for(&app, &state)?;
    let report = crate::ios_diagnostics::report(&app.package_info().version.to_string())?;
    let name = format!("mirroleap-ios-{}.json", chrono::Utc::now().format("%Y%m%d-%H%M%S-%9f"));
    crate::commands::save_report(window, app, report, name).await
}

#[tauri::command]
pub async fn ios_airplay_status(
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
        let gate=state.install_gate.lock().await;
        start_receiver(&app,&state).await?;
        drop(gate);
        status_for(&app,&state)
    }
    #[cfg(not(target_os = "windows"))]
    {let _=(app,state); Err("AirPlay receiver requires Windows x64".into())}
}

#[cfg(target_os = "windows")]
async fn start_receiver(app:&AppHandle,state:&IosAirplayState)->Result<(),String>{
    let root=engine_root(app)?;
    if !installed(&root) {return Err("Install the verified AirPlay receiver first".into());}
    if running(app,state)? {
        // MirroLeap can resume control of its verified receiver even
        // when an older GUI version exited without killing the stream.
        if active_receiver_pid(state).is_some_and(|pid|
            !crate::ios_device_frame::active_for(pid)){
            if let Some(pid)=active_receiver_pid(state){
                *state.window_style.lock().map_err(|e|e.to_string())?=
                    prefer_window_style(app)?;
                if let Err(e)=crate::ios_device_frame::start(app,pid){
                    eprintln!("[iOS frame] Could not attach recovered receiver: {e}");
                }
            }
        }
        return Ok(());
    }
    use std::os::windows::process::CommandExt;
    let style=prefer_window_style(app)?;
    let compact=style!="classic";
    let quality=prefer_quality_tier(app)?;
    let display_size=resolve_display_size(&prefer_window_size(app)?,&quality);
    let runtime_root = root.clone();
    let executable = tokio::task::spawn_blocking(move || prepare_receiver_executable(&runtime_root))
        .await.map_err(|e| e.to_string())??;
    configure_receiver_window(app,compact,&quality)?;
    // Read the receiver's own startup details and video dimensions.
    // The upstream --log switch does NOT create a console if stderr is
    // piped; unfiltered logs may contain private LAN details, so never
    // persist them or surface raw lines in the UI.
    let mut command=std::process::Command::new(executable);
    // SDL 2.32 supports per-monitor DPI awareness before video init.
    // Render into physical pixels to avoid Windows resampling a bitmap
    // already scaled once by SDL on 125% or 150% displays.
    command.env("SDL_WINDOWS_DPI_AWARENESS","permonitorv2")
        .env("SDL_WINDOWS_DPI_SCALING","0");
    command.show_window(0);
    let mut child=command.arg("--log")
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
    *state.connection.lock().map_err(|e|e.to_string())?=ReceiverConnection::default();
    *state.managed_window.lock().map_err(|e|e.to_string())?=None;
    *state.child.lock().map_err(|e|e.to_string())?=Some(child);
    *state.window_style.lock().map_err(|e|e.to_string())?=style;
    *state.display_size.lock().map_err(|e|e.to_string())?=display_size;
    *state.active_quality_tier.lock().map_err(|e|e.to_string())?=Some(quality);
    if let Some(stderr)=stderr {
        let app_logs=app.clone();
        std::thread::spawn(move || {
            use std::io::{BufRead,BufReader};
            for bytes in BufReader::new(stderr).split(b'\n').map_while(Result::ok) {
                let Ok(line)=std::str::from_utf8(&bytes) else {continue;};
                let state=app_logs.state::<IosAirplayState>();
                let Ok(child)=state.child.lock() else{break;};
                if child.as_ref().is_none_or(|process|process.id()!=pid){break;}
                if let Ok(mut info)=state.connection.lock(){info.read_startup_log(line.trim_end());}
                let dimension=decoded_texture_from_line(line);
                let cleared=line.contains("VideoRenderer clear_session:");
                if dimension.is_some() || cleared {
                    if let Ok(mut pixels)=state.source_pixels.lock(){
                        *pixels=if cleared {None} else {dimension};
                    }
                }
            }
        });
    }
    let prepared=tokio::task::spawn_blocking(move||crate::ios_device_frame::prepare_background_window(pid,compact))
        .await.map_err(|e|e.to_string()).and_then(|result|result);
    match prepared{
        Ok(hwnd)=>{
            *state.managed_window.lock().map_err(|e|e.to_string())?=Some((pid,hwnd));
        }
        Err(error)=>{
            if let Ok(mut slot)=state.child.lock(){
                if let Some(mut child)=slot.take(){let _=child.kill();let _=child.wait();}
            }
            if let Ok(mut job)=state.receiver_job.lock(){job.take();}
            return Err(error);
        }
    }
    if !running(app,state)? {
        return Err("AirPlay-Windows exited. Another receiver may already be running outside MirroLeap; close that AirPlay window with Alt+F4 and try again.".into());
    }
    // One passive, click-through native silhouette; it does not create
    // another user-facing settings page or capture the mirror's pixels.
    if let Err(error)=crate::ios_device_frame::start(app,pid) {
        let _=stop_owned_receiver(app,state);
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub async fn ios_airplay_set_receiver_name(
    window:Window,app:AppHandle,state:State<'_,IosAirplayState>,name:String,
)->Result<IosAirplayStatus,String>{
    main_only(&window)?;
    let name=name.trim_matches(' ');
    if !valid_receiver_name(name){
        return Err("Invalid AirPlay receiver name".into());
    }
    #[cfg(target_os="windows")]
    {
        let gate=state.install_gate.lock().await;
        let owned=running(&app,&state)? && state.child.lock().map_err(|e|e.to_string())?.is_some();
        let restart=owned && {
            let connection=state.connection.lock().map_err(|e|e.to_string())?;
            connection.ready && connection.name.as_deref()!=Some(name)
        } &&
            state.source_pixels.lock().map_err(|e|e.to_string())?.is_none();
        save_pref(&app,"receiver-name.txt",name)?;
        if restart{
            stop_owned_receiver(&app,&state)?;
            start_receiver(&app,&state).await?;
        }
        drop(gate);
        status_for(&app,&state)
    }
    #[cfg(not(target_os="windows"))]
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
        let size=prefer_window_size(&app)?;
        save_pref(&app,"quality-tier.txt",&tier)?;
        *state.display_size.lock().map_err(|e|e.to_string())?=resolve_display_size(&size,&tier);
        drop(_gate);
        status_for(&app,&state)
    }
    #[cfg(not(target_os="windows"))]
    {let _=(app,state);Err("Windows AirPlay receiver required".into())}
}

#[tauri::command]
pub async fn ios_airplay_set_window_size(
    window:Window,app:AppHandle,state:State<'_,IosAirplayState>,size:String,
)->Result<IosAirplayStatus,String>{
    main_only(&window)?;
    if !valid_window_size(&size){return Err("Unsupported iOS window size".into());}
    #[cfg(target_os="windows")]
    {
        let gate=state.install_gate.lock().await;
        let tier=prefer_quality_tier(&app)?;
        save_pref(&app,"window-size.txt",&size)?;
        *state.display_size.lock().map_err(|e|e.to_string())?=resolve_display_size(&size,&tier);
        drop(gate);
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
        if !running(&app,&state)? {
            return Err("iOS receiver is not running".into());
        }
        let pid=active_receiver_pid(&state)
            .ok_or("iOS receiver process unavailable")?;
        let hwnd=crate::commands::find_window_by_pid(pid)
            .ok_or("Receiver video window is not ready; click it and press H or F11")?;
        send_window_action(hwnd,action)?;
        // WM_KEYDOWN delivery is asynchronous, so don't claim the view
        // changed; the sender will show a request-queued feedback message.
        status_for(&app,&state)
    }
    #[cfg(not(target_os="windows"))]
    {let _=(app,state,action);Err("Windows AirPlay receiver required".into())}
}

#[cfg(target_os="windows")]
pub(crate) fn send_window_action(hwnd:windows::Win32::Foundation::HWND,action:&str)->Result<(),String>{
    use windows::Win32::Foundation::{WPARAM,LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW,WM_KEYDOWN,WM_KEYUP};
    let (virtual_key,scan_code):(usize,isize)=match action {
        "toggle_clean"=>(0x48,0x23),
        "fullscreen"=>(0x7A,0x57),
        "record"=>(0x52,0x13),
        _=>return Err("Unsupported iOS receiver window action".into()),
    };
    unsafe {
        PostMessageW(Some(hwnd),WM_KEYDOWN,WPARAM(virtual_key),LPARAM(1|(scan_code<<16)))
            .map_err(|e|format!("Could not send receiver key: {e}"))?;
        PostMessageW(Some(hwnd),WM_KEYUP,WPARAM(virtual_key),LPARAM(1|(scan_code<<16)|(1_isize<<30)|(1_isize<<31)))
            .map_err(|e|format!("Could not release receiver key: {e}"))?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn stop_owned_receiver(app:&AppHandle,state:&IosAirplayState)->Result<(),String>{
    // Close the child bezel BEFORE terminating the third-party SDL parent.
    // There must never be a stale WebView2 HWND pointing to dead video.
    crate::ios_device_frame::close(app);
    if let Ok(mut managed)=state.managed_window.lock(){managed.take();}
    *state.active_quality_tier.lock().map_err(|e|e.to_string())?=None;
    state.frame_attached.store(false,Ordering::Release);
    let child=state.child.lock().map_err(|e|e.to_string())?.take();
    *state.source_pixels.lock().map_err(|e|e.to_string())?=None;
    *state.connection.lock().map_err(|e|e.to_string())?=ReceiverConnection::default();
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
    // A prior MirroLeap version may have orphaned a pinned receiver. A
    // verified exact executable path is required before killing any PID.
    let root = engine_root(app)?;
    if let Some((pid, expected)) = verified_receiver(&root, adopted) {
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
    fn receiver_names_follow_utf8_label_limits_and_preserve_other_settings(){
        assert!(valid_receiver_name("书房 iPad"));
        assert!(valid_receiver_name(&"屏".repeat(16)));
        assert!(valid_receiver_name(&"a".repeat(50)));
        for invalid in ["", "　", "Hall.AirPlay", "Room\\TV", "new\r\nfullscreen=1", "\tOffice"]{
            assert!(!valid_receiver_name(invalid),"{invalid:?}");
        }
        assert!(!valid_receiver_name(&"屏".repeat(17)));
        let original="# keep\r\nreceiver_name=Old\r\nchromeless=1\r\nreceiver_name=Studio\r\n";
        assert_eq!(receiver_name_from_ini(original),Some("Studio"));
        let updated=update_receiver_keys(original,&[("receiver_name","书房 iPad")]);
        assert_eq!(updated,"# keep\nreceiver_name=书房 iPad\nchromeless=1\n");
        assert_eq!(receiver_name_from_ini(&updated),Some("书房 iPad"));
    }

    #[test]
    fn startup_details_use_only_the_receivers_name_primary_ip_and_ready_message(){
        let mut connection=ReceiverConnection::default();
        connection.read_startup_log("[08:12:01.001] INFO   name=书房 iPad");
        connection.read_startup_log("[08:12:01.001] INFO   ip=0.0.0.0");
        connection.read_startup_log("[08:12:01.001] INFO   ipv6=fd00::1234");
        assert_eq!(connection.ip,Some("fd00::1234".parse().unwrap()));
        connection.read_startup_log("[08:12:01.001] INFO   ip=192.168.2.107");
        connection.read_startup_log("[08:12:01.001] INFO   ipv6=fd00::789a");
        assert!(!connection.ready);
        connection.read_startup_log("[08:12:01.201] INFO   Ready. Close the window or press Ctrl-C to exit.");
        connection.read_startup_log("[08:12:03.001] INFO   ip=192.168.2.17");
        assert_eq!(connection.name.as_deref(),Some("书房 iPad"));
        assert_eq!(connection.ip,Some("192.168.2.107".parse().unwrap()));
        assert!(connection.ready);
    }
    #[test]
    fn compact_launch_uses_phone_size_and_preserves_other_receiver_options(){
        let initial="window_width=1240\nwindow_height=800\nwindow_x=-640\nfullscreen=1\nreceiver_name=Studio\nwindow_width=900\n";
        let updated=update_receiver_startup_ini(initial);
        assert!(updated.contains("window_width=360\nwindow_height=780\n"));
        assert!(updated.contains("window_x=-640\nfullscreen=0\nreceiver_name=Studio\n"));
        assert_eq!(updated.matches("window_width=").count(),1);
        assert!(updated.contains("window_placement_valid=1\n"));
        assert_eq!(update_receiver_startup_ini(&updated),updated);
    }

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
    #[test]
    fn adaptive_cap_prevents_excessive_4k_minification_for_small_windows(){
        assert_eq!(adaptive_quality_cap(None),1920);
        assert_eq!(adaptive_quality_cap(Some((1272,1830))),1920);
        assert_eq!(adaptive_quality_cap(Some((3840,2160))),3840);
        assert_eq!(adaptive_quality_cap(Some((2,3))),1920);
        assert_eq!(parse_last_display("1272x1830\n"),Some((1272,1830)));
        assert_eq!(parse_last_display("../1900x2000"),None);
        assert_eq!(parse_last_display("400000x1800"),None);
        assert!(valid_quality_tier("adaptive"));
        assert!(!valid_quality_tier("adaptive/../../bad"));
        let adaptive=update_receiver_quality_dims_ini("receiver_name=Office\n",2112,2112);
        assert!(adaptive.contains("mirror_width=2112\n"));
        assert!(adaptive.contains("mirror_height=2112\n"));
        assert!(adaptive.contains("receiver_name=Office\n"));
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
