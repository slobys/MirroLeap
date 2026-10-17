//! Bounded Android screen-orientation override for the user's explicit
//! floating-rail Rotate action. scrcpy MOD+R is merely a request and may be
//! undone immediately by auto-rotation; this uses Android's window-manager CLI.
//! MirroLeap remembers the original free/locked state and restores it on exit.
//! Unlike --orientation / MOD+arrow, this changes the *real phone pixels*.

use crate::ScrcpyState;
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::{io::AsyncReadExt, sync::Mutex, time::{timeout, Duration}};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RotationMode {
    Free,
    Locked(u8),
}

#[derive(Debug, Clone)]
struct Override {
    original: RotationMode,
    applied: u8,
    adb: String,
}

static OVERRIDES: OnceLock<Mutex<HashMap<String, Override>>> = OnceLock::new();
fn overrides() -> &'static Mutex<HashMap<String, Override>> {
    OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn parse_mode(output: &str) -> Option<RotationMode> {
    let words: Vec<_> = output.split_whitespace().collect();
    match words.as_slice() {
        ["free"] => Some(RotationMode::Free),
        ["lock", value] => value.parse::<u8>().ok().filter(|n| *n < 4).map(RotationMode::Locked),
        _ => None,
    }
}

fn parse_physical_size(output: &str) -> Option<(u32,u32)> {
    let raw = output.lines()
        .find_map(|line| line.trim().strip_prefix("Physical size:"))?.trim();
    let (a,b) = raw.split_once('x')?;
    let (w,h) = (a.trim().parse::<u32>().ok()?, b.trim().parse::<u32>().ok()?);
    if w == 0 || h == 0 { None } else { Some((w,h)) }
}

fn choose_target(screen:(u32,u32), natural_landscape:bool) -> u8 {
    // Rotate to the opposite visible orientation. Natural-landscape tablets
    // use rotation=0 for landscape, unlike portrait-native phones.
    let screen_landscape = screen.0 > screen.1;
    match (screen_landscape, natural_landscape) {
        (false, false) => 1,
        (true, false) => 0,
        (false, true) => 0,
        (true, true) => 1,
    }
}

fn png_size(bytes: &[u8]) -> Option<(u32,u32)> {
    if bytes.len() < 45 || bytes.len() > 24*1024*1024 ||
        !bytes.starts_with(b"\x89PNG\r\n\x1a\n") ||
        &bytes[12..16] != b"IHDR" { return None; }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    if !(1..=16_384).contains(&w) || !(1..=16_384).contains(&h) ||
        w as u64 * h as u64 > 24_000_000 { return None; }
    Some((w,h))
}

async fn adb(adb: &str, device: &str, args: &[&str]) -> Result<String,String> {
    let output = timeout(Duration::from_secs(7), crate::commands::create_command(adb)
        .arg("-s").arg(device).args(args).kill_on_drop(true).output()).await
        .map_err(|_| "Android direction command timed out".to_string())?
        .map_err(|e| format!("Cannot start ADB: {e}"))?;
    if !output.status.success() {
        return Err(format!("Android rejected rotation command: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(140).collect::<String>()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn rotation_command(adb_path: &str, device: &str, args: &[&str]) -> Result<String,String> {
    let mut command = vec!["shell", "cmd", "window", "user-rotation"];
    command.extend_from_slice(args);
    match adb(adb_path,device,&command).await {
        Ok(output) => Ok(output),
        Err(_) => {
            // Some older Android versions expose the same window-manager CLI
            // via wm rather than cmd window.
            let mut fallback = vec!["shell", "wm", "user-rotation"];
            fallback.extend_from_slice(args);
            adb(adb_path,device,&fallback).await
        }
    }
}

async fn get_mode(adb_path: &str, device: &str) -> Result<RotationMode,String> {
    let text = rotation_command(adb_path,device,&[]).await?;
    if let Some(mode) = parse_mode(&text) { return Ok(mode); }
    // Some OEM wrappers return successful shell exit codes for help text.
    // Probe the legacy alias before reporting unsupported window-manager CLI.
    if let Ok(fallback) = adb(adb_path,device,&["shell","wm","user-rotation"]).await {
        if let Some(mode) = parse_mode(&fallback) { return Ok(mode); }
    }
    Err(format!(
        "Android did not report a supported rotation state: {}",
        text.chars().take(90).collect::<String>()))
}

async fn set_mode(adb_path: &str,device: &str,mode:RotationMode) -> Result<(),String> {
    match mode {
        RotationMode::Free => { rotation_command(adb_path,device,&["free"]).await?; }
        RotationMode::Locked(value) => {
            let angle = value.to_string();
            rotation_command(adb_path,device,&["lock",&angle]).await?;
        }
    }
    Ok(())
}

async fn screenshot_size(adb_path: &str,device: &str) -> Result<(u32,u32),String> {
    let mut child = crate::commands::create_command(adb_path)
        .arg("-s").arg(device).args(["exec-out","screencap","-p"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true).spawn()
        .map_err(|e| format!("Cannot capture Android orientation: {e}"))?;
    let snapshot = async {
        let mut bytes = Vec::new();
        let stdout = child.stdout.take().ok_or("Android screenshot has no data stream")?;
        stdout.take(24*1024*1024+1).read_to_end(&mut bytes).await
            .map_err(|e| format!("Cannot read Android screen: {e}"))?;
        let status = child.wait().await
            .map_err(|e| format!("Android screencap failed: {e}"))?;
        if !status.success() { return Err("Android screenshot command failed".into()); }
        png_size(&bytes).ok_or("Android screenshot has invalid PNG dimensions".into())
    };
    timeout(Duration::from_secs(12),snapshot).await
        .map_err(|_| "Android screenshot timed out".to_string())?
}

/// Change real Android orientation, validate by ADB PNG dimensions and remember
/// the pre-existing mode. The manual override remains only during mirroring.
pub(crate) async fn rotate(state: &ScrcpyState, device: &str) -> Result<(),String> {
    crate::commands::validate_capture_device(device)?;
    let adb_path = {
        state.adb_paths.lock().map_err(|e|e.to_string())?
            .get(device).cloned()
    }.ok_or("Could not locate the ADB binary for this mirroring session")?;
    let mut entries = overrides().lock().await;
    let current = get_mode(&adb_path,device).await?;
    let prior = match entries.get(device) {
        Some(saved) if saved.adb == adb_path &&
            current == RotationMode::Locked(saved.applied) => saved.original,
        _ => current,
    };
    let before = screenshot_size(&adb_path,device).await?;
    let natural = adb(&adb_path,device,&["shell","wm","size"]).await
        .ok().and_then(|output| parse_physical_size(&output))
        .map(|(w,h)| w > h).unwrap_or(false);
    let target = choose_target(before,natural);
    set_mode(&adb_path,device,RotationMode::Locked(target)).await?;
    // Track immediately after the successful setting: if verification times
    // out AND restoration fails, a later normal stop can still restore it.
    entries.insert(device.to_owned(), Override {
        original: prior, applied: target, adb: adb_path.clone(),
    });
    let rotated = timeout(Duration::from_secs(14), async {
        for _ in 0..9 {
            tokio::time::sleep(Duration::from_millis(300)).await;
            if let Ok(after) = screenshot_size(&adb_path,device).await {
                if (before.0 > before.1) != (after.0 > after.1) {
                    return true;
                }
            }
        }
        false
    }).await.unwrap_or(false);
    if rotated { return Ok(()); }
    // Do not leave an ineffective override enabled on a direction-locked app.
    // A reset error is reported, and the stored restore state is retained.
    set_mode(&adb_path,device,prior).await.map_err(|e|
        format!("手机画面未切换，且恢复旋转设置失败：{e}"))?;
    entries.remove(device);
    Err("Android 当前应用或系统未允许横竖屏切换。已恢复之前的自动旋转设置。".into())
}

/// Best effort when a mirror stops, closes its native SDL window, or quits.
/// Do not override a user's intervening manual rotation changes.
pub(crate) async fn restore(_state: &ScrcpyState, device: &str) -> Result<(),String> {
    let mut entries = overrides().lock().await;
    let Some(saved) = entries.get(device).cloned() else { return Ok(()); };
    let current = get_mode(&saved.adb,device).await?;
    if current == RotationMode::Locked(saved.applied) {
        set_mode(&saved.adb,device,saved.original).await?;
    }
    entries.remove(device);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_only_canonical_rotation_states() {
        assert_eq!(parse_mode("free\n"),Some(RotationMode::Free));
        assert_eq!(parse_mode("lock 1\n"),Some(RotationMode::Locked(1)));
        assert_eq!(parse_mode("lock 3\n"),Some(RotationMode::Locked(3)));
        for invalid in ["", "locked 0", "lock 9", "free 0", "lock -1", "oops free"] {
            assert_eq!(parse_mode(invalid),None);
        }
    }
    #[test]
    fn chooses_real_display_orientation_for_phone_and_tablet() {
        assert_eq!(choose_target((1080,2400),false),1);
        assert_eq!(choose_target((2400,1080),false),0);
        assert_eq!(choose_target((800,1280),true),0);
        assert_eq!(choose_target((1280,800),true),1);
    }
    #[test]
    fn reads_physical_dimensions_and_rejects_malformed_images() {
        assert_eq!(parse_physical_size("Physical size: 1080x2400\nOverride size: 720x1600"),
            Some((1080,2400)));
        assert_eq!(parse_physical_size("error"),None);
        assert_eq!(png_size(b"bad PNG"),None);
    }
}
