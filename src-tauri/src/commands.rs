use tauri::{State, Window, Emitter, Manager};
use std::process::{Command as StdCommand, Stdio};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use serde_json::json;
use crate::ScrcpyState;
use tokio::process::Command as TokioCommand;
use tokio::io::{BufReader, AsyncBufReadExt, AsyncReadExt};
use tokio::time::{timeout, Duration};
use serde::Deserialize;
use crate::video_decoder::{self, DecoderDecision, VideoDecoderSupport};

const AUDIO_FALLBACK_CHAIN: &[&str] = &["aac", "flac", "raw"];

fn require_main_window(window: &Window) -> Result<(), String> {
    if window.label() != "main" {
        return Err("This device operation is only available from the main MirroLeap window.".into());
    }
    Ok(())
}

fn is_audio_codec_error(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains("could not create default audio encoder")
        || lower.contains("failed to initialize audio")
    {
        return true;
    }
    let mentions_codec = lower.contains("audio encoder") || lower.contains("audio codec");
    let mentions_failure = lower.contains("fail")
        || lower.contains("error")
        || lower.contains("could not")
        || lower.contains("not available")
        || lower.contains("not supported");
    mentions_codec && mentions_failure
}

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

fn get_binary_path(binary_name: &str, custom_folder: Option<String>) -> String {
    let exe_ext = std::env::consts::EXE_EXTENSION;
    let binary_filename = if exe_ext.is_empty() {
        binary_name.to_string()
    } else {
        format!("{}.{}", binary_name, exe_ext)
    };

    // 1. Check custom folder if provided
    if let Some(folder) = custom_folder {
        if !folder.trim().is_empty() {
            let full_path = Path::new(&folder).join(&binary_filename);
            if full_path.exists() && full_path.is_file() {
                return full_path.to_string_lossy().to_string();
            }
        }
    }
    
    // 2. Check local scrcpy-bin folder (relative to executable for portable/production)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
             let local_bin = exe_dir.join("scrcpy-bin").join(&binary_filename);
             if local_bin.exists() && local_bin.is_file() {
                 return local_bin.to_string_lossy().to_string();
             }
        }
    }

    // 3. Fallback to current working directory scrcpy-bin
    if let Ok(current_dir) = std::env::current_dir() {
        let local_bin = current_dir.join("scrcpy-bin").join(&binary_filename);
        if local_bin.exists() && local_bin.is_file() {
            return local_bin.to_string_lossy().to_string();
        }
    }

    // 4. Return simple name to rely on system PATH
    binary_name.to_string()
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    let src_ref = src.as_ref();
    let dst_ref = dst.as_ref();
    
    // Canonicalize dst path if it exists to get absolute base path
    let canonical_dst = if dst_ref.exists() {
        dst_ref.canonicalize()?
    } else {
        std::fs::create_dir_all(dst_ref)?;
        dst_ref.canonicalize()?
    };
    
    for entry in std::fs::read_dir(src_ref)? {
        let entry = entry?;
        let entry_path = entry.path();
        let file_name = entry.file_name();
        
        // Prevent directory traversal elements in entry name
        let file_name_str = file_name.to_string_lossy();
        if file_name_str.contains("..") || file_name_str.contains('/') || file_name_str.contains('\\') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Directory entry contains directory traversal components",
            ));
        }
        
        let target_path = dst_ref.join(&file_name);
        
        // Ensure destination path is strictly within the target base directory
        if let Some(parent_dir) = target_path.parent() {
            if parent_dir.exists() {
                let canonical_parent = parent_dir.canonicalize()?;
                if !canonical_parent.starts_with(&canonical_dst) && canonical_parent != canonical_dst {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "Path traversal attempt detected during directory copy",
                    ));
                }
            }
        }
        
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry_path, &target_path)?;
        } else {
            std::fs::copy(&entry_path, &target_path)?;
        }
    }
    Ok(())
}



#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn create_command<S: AsRef<std::ffi::OsStr>>(program: S) -> TokioCommand {
    let cmd = TokioCommand::new(program);
    #[cfg(target_os = "windows")]
    {
        let mut cmd = cmd;
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }
    #[cfg(not(target_os = "windows"))]
    cmd
}

#[tauri::command]
pub async fn check_scrcpy(custom_path: Option<String>) -> serde_json::Value {
    let exe_path = get_binary_path("scrcpy", custom_path);
    
    // Check version to verify it exists and is runnable
    let output = create_command(&exe_path)
        .arg("--version")
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
             json!({ "found": true, "message": "Scrcpy Ready" })
        },
        Ok(_) => {
             json!({ "found": false, "message": "Failed to start scrcpy (Exit Code != 0)" })
        },
        Err(_) => {
             json!({ "found": false, "message": "Scrcpy not found" })
        }
    }
}

/// ADB device states that can appear in the second column of `adb devices -l`
/// output. Only "device" means the device is connected and authorized; every
/// other state (offline, mid-pairing, unauthorized, ...) is not usable yet.
const KNOWN_ADB_STATES: &[&str] = &[
    "device",
    "offline",
    "unauthorized",
    "authorizing",
    "unknown",
    "bootloader",
    "recovery",
    "sideload",
    "host",
    "connecting",
];

/// Parses one data line of `adb devices -l` output into `(serial, state)`.
///
/// The serial and state are whitespace-separated, but the serial itself can
/// contain a space: an mDNS wireless-debugging serial in "conflict" form
/// (when two devices would otherwise resolve to the same mDNS name) looks
/// like `adb-SERIAL (2)._adb-tls-connect._tcp`. Splitting on the first
/// whitespace run, or assuming a specific serial shape, would misparse that
/// case. Instead this finds the first token that is a *known ADB state* and
/// treats everything before it as the serial, so it corresponds to the state
/// ADB actually reports, regardless of the serial's shape (USB ID, IP:port,
/// or mDNS name). Any further `-l` fields (`product:... model:...
/// transport_id:...`) are simply ignored.
fn parse_adb_device_line(line: &str) -> Option<(String, String)> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let state_idx = tokens.iter().position(|t| KNOWN_ADB_STATES.contains(t))?;
    if state_idx == 0 {
        return None; // no serial before the state
    }
    Some((tokens[..state_idx].join(" "), tokens[state_idx].to_string()))
}

/// Extracts the `model:` field from a `adb devices -l` line, if present, for
/// a more user-friendly device label than the raw serial (e.g. "SM S911B"
/// instead of a USB serial or IP:port). Adb replaces spaces in the model name
/// with underscores in this raw form; those are turned back into spaces,
/// which is as far as this goes without an extra `getprop` round-trip per
/// device -- still closer to a real name than a serial.
fn extract_device_model(line: &str) -> Option<String> {
    let model = line
        .split_whitespace()
        .find_map(|t| t.strip_prefix("model:"))?;
    if model.is_empty() {
        return None;
    }
    Some(model.replace('_', " "))
}

#[tauri::command]
pub async fn get_devices(custom_path: Option<String>) -> serde_json::Value {
    let adb_path = get_binary_path("adb", custom_path);

    let output = create_command(&adb_path)
        .arg("devices")
        .arg("-l")
        .output()
        .await;

    match output {
        Ok(o) => {
             if o.status.success() {
                 let out_str = String::from_utf8_lossy(&o.stdout);
                 let mut devices: Vec<String> = Vec::new();
                 let mut pending = Vec::new();
                 let mut device_models = serde_json::Map::new();
                 for line in out_str.lines().skip(1) {
                     let Some((serial, state)) = parse_adb_device_line(line) else { continue };
                     if state != "device" {
                         // Keep these visible for troubleshooting, but never send
                         // them to scrcpy as usable targets.
                         pending.push(json!({ "serial": serial, "state": state }));
                         continue;
                     }
                     if let Some(model) = extract_device_model(line) {
                         device_models.insert(serial.clone(), json!(model));
                     }
                     devices.push(serial);
                 }

                 json!({ "error": false, "devices": devices, "pendingDevices": pending, "deviceModels": device_models })
             } else {
                 json!({ "error": true, "message": "ADB returned error" })
             }
        },
        Err(e) => {
             json!({ "error": true, "message": e.to_string() })
         }
    }
}

#[tauri::command]
pub async fn get_mdns_devices(custom_path: Option<String>) -> serde_json::Value {
    let adb_path = get_binary_path("adb", custom_path);
    
    let output = create_command(&adb_path)
        .arg("mdns")
        .arg("services")
        .output()
        .await;

    match output {
        Ok(o) => {
            if o.status.success() {
                let out_str = String::from_utf8_lossy(&o.stdout);
                let mut services = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for line in out_str.lines().skip(1) {
                    if let Some((name, service, address)) = parse_mdns_service_line(line) {
                        let key = format!("{}|{}|{}", name, service, address);
                        if seen.insert(key) {
                            services.push(json!({
                                "name": name,
                                "service": service,
                                "address": address
                            }));
                        }
                    }
                }
                json!({ "error": false, "services": services })
            } else {
                json!({ "error": true, "message": "ADB mdns returned error" })
            }
        },
        Err(e) => {
            json!({ "error": true, "message": e.to_string() })
        }
    }
}

/// ADB uses either two columns (service FQDN, IP:port) or three
/// (instance, service type, IP:port), depending on platform-tools version.
/// Normalize both without mistaking the pairing port for the connect port.
fn parse_mdns_service_line(line: &str) -> Option<(String, &'static str, String)> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 2 { return None; }
    let address = *fields.last()?;
    let (host, port) = address.rsplit_once(':')?;
    if host.is_empty() || port.parse::<u16>().ok().filter(|p| *p > 0).is_none() {
        return None;
    }
    let kind = fields[..fields.len() - 1].iter().find_map(|field| {
        if field.contains("_adb-tls-connect._tcp") {
            Some("_adb-tls-connect._tcp")
        } else if field.contains("_adb-tls-pairing._tcp") {
            Some("_adb-tls-pairing._tcp")
        } else {
            None
        }
    })?;
    let instance = fields[0].split("._adb-tls-").next()?.trim_end_matches('.');
    if instance.is_empty() { return None; }
    Some((instance.to_owned(), kind, address.to_owned()))
}

fn mdns_connect_candidates(output: &str, requested: &str) -> Vec<String> {
    let host = requested.rsplit_once(':').map(|(h, _)| h).unwrap_or(requested);
    let mut candidates = Vec::new();
    for line in output.lines() {
        let Some((_, service, address)) = parse_mdns_service_line(line) else { continue; };
        if service != "_adb-tls-connect._tcp" { continue; }
        if address.rsplit_once(':').map(|(h, _)| h) == Some(host)
            && address != requested && !candidates.contains(&address) {
            candidates.push(address);
        }
    }
    candidates
}

async fn attempt_adb_connect(adb_path: &str, address: &str) -> Result<(bool, String), String> {
    let mut command = create_command(adb_path);
    command.arg("connect").arg(address).kill_on_drop(true);
    let output = timeout(Duration::from_secs(10), command.output())
        .await
        .map_err(|_| format!("Connection to {} timed out after 10s", address))?
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let message = if stdout.is_empty() { stderr.clone() } else if stderr.is_empty() { stdout.clone() } else { format!("{}\\n{}", stdout, stderr) };
    let lower = message.to_lowercase();
    let success = output.status.success()
        && (lower.contains("connected to") || lower.contains("already connected to"))
        && !lower.contains("cannot connect") && !lower.contains("failed to connect");
    Ok((success, message))
}

#[tauri::command]
pub async fn adb_connect(window: Window, ip: String, custom_path: Option<String>, silent: Option<bool>) -> Result<serde_json::Value, String> {
    require_main_window(&window)?;
    let silent = silent.unwrap_or(false);
    let adb_path = get_binary_path("adb", custom_path);
    // Exact unicast endpoint only. Never forward raw user text that could
    // be interpreted as an adb option or additional argument.
    let Some(endpoint) = parse_diagnostic_endpoint(&ip) else {
        return Ok(json!({ "success": false, "message": "Enter a valid unicast IPv4 address and optional TCP port" }));
    };
    let requested_address = endpoint.to_string();
    let requested = requested_address.as_str();
    if !silent { let _ = window.emit("scrcpy-log", format!("[SYSTEM] Connecting to {}...", requested)); }

    let (success, mut message) = attempt_adb_connect(&adb_path, requested).await?;
    if success {
        if !silent { let _ = window.emit("scrcpy-log", format!("[ADB] {}", message)); }
        return Ok(json!({ "success": true, "message": message, "address": requested }));
    }

    // On Android 11+, the TLS debugging port changes. Recover from a stale
    // 5555/history entry using ADB's advertised connect port for the SAME IP.
    let mut mdns = create_command(&adb_path);
    mdns.arg("mdns").arg("services").kill_on_drop(true);
    if let Ok(Ok(output)) = timeout(Duration::from_secs(5), mdns.output()).await {
        if output.status.success() {
            let listing = String::from_utf8_lossy(&output.stdout);
            for candidate in mdns_connect_candidates(&listing, requested) {
                if !silent { let _ = window.emit("scrcpy-log", format!("[ADB] Retrying discovered wireless debugging port {}", candidate)); }
                match attempt_adb_connect(&adb_path, &candidate).await {
                    Ok((true, result)) => {
                        if !silent { let _ = window.emit("scrcpy-log", format!("[ADB] {}", result)); }
                        return Ok(json!({ "success": true, "message": result, "address": candidate }));
                    }
                    Ok((false, reason)) => message = reason,
                    Err(reason) => message = reason,
                }
            }
        }
    }
    if !silent { let _ = window.emit("scrcpy-log", format!("[ADB] {}", message)); }
    Ok(json!({ "success": false, "message": format!("{}. Check the phone's current Wireless debugging IP & port; pairing and connection ports are different.", message) }))
}

#[cfg(test)]
mod wireless_connection_tests {
    use super::{mdns_connect_candidates, parse_mdns_service_line};

    #[test]
    fn selects_current_tls_port_for_same_ip_only() {
        let output = r#"List of discovered mdns services
adb-phone._adb-tls-pairing._tcp. _adb-tls-pairing._tcp. 192.168.2.149:38912
adb-phone._adb-tls-connect._tcp. _adb-tls-connect._tcp. 192.168.2.149:42137
adb-other._adb-tls-connect._tcp. _adb-tls-connect._tcp. 192.168.2.150:44444"#;
        assert_eq!(
            mdns_connect_candidates(output, "192.168.2.149:5555"),
            vec!["192.168.2.149:42137"]
        );
    }

    #[test]
    fn parses_two_column_adb_mdns_services() {
        assert_eq!(
            parse_mdns_service_line("adb-phone._adb-tls-connect._tcp.    192.168.2.226:42351"),
            Some(("adb-phone".to_owned(), "_adb-tls-connect._tcp", "192.168.2.226:42351".to_owned()))
        );
    }

    #[test]
    fn does_not_confuse_pairing_with_connection() {
        let output = r#"List of discovered mdns services
adb-phone._adb-tls-pairing._tcp. 192.168.2.226:30001"#;
        assert!(mdns_connect_candidates(output, "192.168.2.226:5555").is_empty());
    }

    #[test]
    fn ignores_invalid_mdns_rows() {
        assert_eq!(parse_mdns_service_line("List of discovered mdns services"), None);
        assert_eq!(parse_mdns_service_line("adb-x._adb-tls-connect._tcp. 10.0.0.4:bad"), None);
    }
}

fn valid_wifi_ip(raw: &str) -> Option<String> {
    let ip = raw.trim().split('/').next()?.parse::<std::net::Ipv4Addr>().ok()?;
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast()
        || ip.is_link_local() || ip.is_broadcast() {
        return None;
    }
    Some(ip.to_string())
}

fn parse_wifi_ip(output: &str) -> Option<String> {
    let words: Vec<&str> = output.split_whitespace().collect();
    words.windows(2).find_map(|pair| {
        if pair[0] == "inet" || pair[0] == "src" {
            valid_wifi_ip(pair[1])
        } else {
            None
        }
    })
}

async fn adb_output(adb_path: &str, args: &[&str]) -> Result<(bool, String), String> {
    let mut command = create_command(adb_path);
    command.args(args).kill_on_drop(true);
    let output = timeout(Duration::from_secs(10), command.output())
        .await
        .map_err(|_| "ADB command timed out".to_string())?
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let message = match (stdout.is_empty(), stderr.is_empty()) {
        (false, false) => format!("{}\n{}", stdout, stderr),
        (false, true) => stdout,
        (true, _) => stderr,
    };
    Ok((output.status.success(), message))
}

/// Enable classic adb-over-TCP/IP through an authorized USB device.
/// This does not require mDNS or Android 11+ TLS wireless pairing.
#[tauri::command]
pub async fn adb_usb_to_wireless(
    window: Window,
    device: String,
    custom_path: Option<String>,
) -> Result<serde_json::Value, String> {
    require_main_window(&window)?;
    if device.trim().is_empty() || device.contains(':')
        || device.starts_with("adb-") || device.starts_with("emulator-")
        || device.chars().any(char::is_whitespace) {
        return Ok(json!({"success": false, "message": "Select an authorized USB device."}));
    }
    let adb_path = get_binary_path("adb", custom_path);
    let (ok, listing) = adb_output(&adb_path, &["devices", "-l"]).await?;
    let authorized = ok && listing.lines().skip(1).any(|line| {
        matches!(parse_adb_device_line(line), Some((serial, state))
            if serial == device && state == "device")
    });
    if !authorized {
        return Ok(json!({"success": false, "message": "USB device not connected or not authorized."}));
    }

    // Read the phone's Wi-Fi IPv4 address before restarting adbd in TCP mode.
    let mut wifi_ip = None;
    for args in [
        vec!["-s", device.as_str(), "shell", "ip", "-f", "inet", "addr", "show", "wlan0"],
        vec!["-s", device.as_str(), "shell", "ip", "route", "show", "dev", "wlan0"],
    ] {
        if let Ok((true, result)) = adb_output(&adb_path, &args).await {
            wifi_ip = parse_wifi_ip(&result);
            if wifi_ip.is_some() { break; }
        }
    }
    if wifi_ip.is_none() {
        if let Ok((true, result)) = adb_output(
            &adb_path, &["-s", device.as_str(), "shell", "getprop", "dhcp.wlan0.ipaddress"]
        ).await {
            wifi_ip = valid_wifi_ip(&result);
        }
    }
    let Some(wifi_ip) = wifi_ip else {
        return Ok(json!({"success": false, "message": "Phone Wi-Fi IPv4 address not found. Enable Wi-Fi and keep USB connected."}));
    };

    let address = format!("{}:5555", wifi_ip);
    let _ = window.emit("scrcpy-log", format!("[ADB] Switching {} to TCP/IP at {}", device, address));
    let (started, message) = adb_output(
        &adb_path, &["-s", device.as_str(), "tcpip", "5555"]
    ).await?;
    if !started {
        return Ok(json!({"success": false, "message": format!("adb tcpip 5555 failed: {}", message)}));
    }
    let mut last_error = String::new();
    for attempt in 0..4 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(800)).await;
        }
        match attempt_adb_connect(&adb_path, &address).await {
            Ok((true, message)) => {
                let _ = window.emit("scrcpy-log", format!("[ADB] Connected wirelessly: {}", address));
                return Ok(json!({"success": true, "address": address, "message": message}));
            },
            Ok((false, message)) => last_error = message,
            Err(message) => last_error = message,
        }
    }
    Ok(json!({
        "success": false,
        "address": address,
        "message": format!("TCP/IP enabled but connection to {} failed: {}. Check Wi-Fi/LAN reachability.", address, last_error)
    }))
}

#[cfg(test)]
mod usb_wireless_tests {
    use super::{parse_wifi_ip, valid_wifi_ip};
    #[test]
    fn extracts_wlan_address_from_android_ip_addr() {
        assert_eq!(
            parse_wifi_ip("3: wlan0: <UP> mtu 1500\n inet 192.168.2.226/24 brd 192.168.2.255"),
            Some("192.168.2.226".to_owned())
        );
    }
    #[test]
    fn extracts_wlan_address_from_route() {
        assert_eq!(
            parse_wifi_ip("192.168.2.0/24 dev wlan0 proto kernel scope link src 192.168.2.226"),
            Some("192.168.2.226".to_owned())
        );
    }
    #[test]
    fn invalid_ips_are_rejected() {
        assert_eq!(valid_wifi_ip("127.0.0.1"), None);
        assert_eq!(valid_wifi_ip("999.1.1.1"), None);
        assert_eq!(valid_wifi_ip("192.168.2.226/24"), Some("192.168.2.226".to_owned()));
    }
}

/// Only probe the explicitly requested IPv4 endpoint; never scan the LAN.
fn parse_diagnostic_endpoint(raw: &str) -> Option<std::net::SocketAddrV4> {
    let trimmed = raw.trim();
    if trimmed.is_empty() { return None; }
    let (host, port) = match trimmed.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().ok()?),
        None => (trimmed, 5555),
    };
    if port == 0 { return None; }
    let ip = host.parse::<std::net::Ipv4Addr>().ok()?;
    if ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() {
        return None;
    }
    Some(std::net::SocketAddrV4::new(ip, port))
}

/// Classify the failure rather than asking users to retry a dead 5555 port.
/// A TCP probe checks reachability, not ADB authorization or successful pairing.
#[tauri::command]
pub async fn diagnose_adb_connection(
    window: Window,
    ip: String,
    custom_path: Option<String>,
) -> serde_json::Value {
    if let Err(message) = require_main_window(&window) {
        return json!({ "code": "error", "address": "", "alternatives": [], "adbError": message });
    }
    let Some(endpoint) = parse_diagnostic_endpoint(&ip) else {
        return json!({"code": "invalid", "address": ip, "alternatives": []});
    };
    let address = endpoint.to_string();
    let adb_path = get_binary_path("adb", custom_path);
    let mut adb_error = None;
    let mut current_state: Option<String> = None;
    match adb_output(&adb_path, &["devices", "-l"]).await {
        Ok((true, listing)) => {
            for line in listing.lines().skip(1) {
                if let Some((serial, state)) = parse_adb_device_line(line) {
                    if serial == address { current_state = Some(state); break; }
                }
            }
        }
        Ok((false, message)) => adb_error = Some(message),
        Err(message) => adb_error = Some(message),
    }
    if let Some(state) = current_state {
        let code = if state == "device" { "connected" }
            else if state == "unauthorized" { "unauthorized" } else { "offline" };
        return json!({"code": code, "address": address, "alternatives": [], "adbError": adb_error});
    }

    let mut alternatives = Vec::<String>::new();
    if let Ok((true, listing)) = adb_output(&adb_path, &["mdns", "services"]).await {
        alternatives = mdns_connect_candidates(&listing, &address);
    }

    // Do not perform a UDP or broad port scan. This is one user-requested TCP
    // attempt, with a tight timeout.
    let probe = timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(endpoint)).await;
    let code = match probe {
        Ok(Ok(_)) => "port_open",
        Ok(Err(ref error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => "refused",
        Ok(Err(ref error)) if error.kind() == std::io::ErrorKind::TimedOut => "timeout",
        Ok(Err(_)) => "unreachable",
        Err(_) => "timeout",
    };
    let code = if !alternatives.is_empty() { "port_changed" } else { code };
    json!({"code": code, "address": address, "alternatives": alternatives, "adbError": adb_error})
}

#[cfg(test)]
mod connection_diagnostics_tests {
    use super::parse_diagnostic_endpoint;

    #[test]
    fn defaults_plain_ipv4_to_legacy_port() {
        assert_eq!(parse_diagnostic_endpoint("192.168.2.226").unwrap().to_string(), "192.168.2.226:5555");
    }
    #[test]
    fn preserves_explicit_wireless_debugging_port() {
        assert_eq!(parse_diagnostic_endpoint("192.168.2.226:42783").unwrap().to_string(), "192.168.2.226:42783");
    }
    #[test]
    fn rejects_invalid_inputs() {
        for invalid in ["", "10.0.0.1:0", "10.0.0.1:99999", "example.com:5555", "256.0.0.1:5", "0.0.0.0:5555",
            "-s 192.168.1.1", "224.0.0.5:5555", "255.255.255.255:5555", "192.168.1.1;echo bad"] {
            assert!(parse_diagnostic_endpoint(invalid).is_none(), "{invalid}");
        }
    }
}

/// Pairing uses the separate, explicitly displayed Android 11+ pairing port;
/// do not treat a regular connection endpoint or arbitrary ADB arguments as
/// a valid pairing instruction.
fn checked_pairing_inputs(ip: &str, code: &str) -> Result<(String, String), String> {
    if !ip.contains(':') {
        return Err("Enter the IP and pairing port shown by Android Wireless debugging.".into());
    }
    let Some(endpoint) = parse_diagnostic_endpoint(&ip) else {
        return Err("Invalid wireless pairing IP and port.".into());
    };
    let pairing_code = code.trim();
    if pairing_code.len() != 6 || !pairing_code.bytes().all(|c| c.is_ascii_digit()) {
        return Err("Enter the six-digit pairing code shown on your phone.".into());
    }
    Ok((endpoint.to_string(), pairing_code.to_string()))
}

#[cfg(test)]
mod wireless_pairing_inputs_tests {
    use super::checked_pairing_inputs;

    #[test]
    fn pairing_requires_exact_phone_ip_and_six_digit_code() {
        assert_eq!(
            checked_pairing_inputs("192.168.2.226:36547", " 123456 ").unwrap(),
            ("192.168.2.226:36547".into(), "123456".into())
        );
        for invalid in ["192.168.2.226", "-x", "example.com:12345",
            "192.168.2.226:0", "192.168.2.226:70000", "10.0.0.1:1234;rm -rf /",
            "224.0.0.1:12345"] {
            assert!(checked_pairing_inputs(invalid, "123456").is_err(), "{invalid}");
        }
        for code in ["", "123", "1234567", "-12345", "12345x", "123 456", "123456\nexit"] {
            assert!(checked_pairing_inputs("192.168.2.226:36547", code).is_err(), "{code}");
        }
    }
}

#[tauri::command]
pub async fn adb_pair(window: Window, ip: String, code: String, custom_path: Option<String>) -> Result<serde_json::Value, String> {
    require_main_window(&window)?;
    let (address, pairing_code) = match checked_pairing_inputs(&ip, &code) {
        Ok(values) => values,
        Err(message) => return Ok(json!({"success": false, "message": message})),
    };
    let adb_path = get_binary_path("adb", custom_path);
    let _ = window.emit("scrcpy-log", format!("[SYSTEM] Pairing with {}...", address));

    let mut command = create_command(&adb_path);
    command.arg("pair").arg(&address).arg(&pairing_code).kill_on_drop(true);
    let output = timeout(Duration::from_secs(20), command.output())
        .await
        .map_err(|_| "Wireless pairing timed out after 20 seconds.".to_string())?
        .map_err(|e| e.to_string())?;

    let out_text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let err_text = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !out_text.is_empty() { let _ = window.emit("scrcpy-log", format!("[ADB] {}", out_text)); }
    if !err_text.is_empty() { let _ = window.emit("scrcpy-log", format!("[ADB ERROR] {}", err_text)); }

    let success = output.status.success() && (out_text.contains("Successfully paired") || err_text.contains("Successfully paired"));

    Ok(json!({ "success": success, "message": if out_text.is_empty() { err_text } else { out_text } }))
}

#[tauri::command]
pub async fn adb_shell(window: Window, device: String, command: String, custom_path: Option<String>) -> serde_json::Value {
    if let Err(message) = require_main_window(&window) {
        return json!({ "success": false, "message": message });
    }
    let adb_path = get_binary_path("adb", custom_path);
    
    let output = create_command(&adb_path)
        .arg("-s")
        .arg(&device)
        .arg("shell")
        .arg(&command)
        .output()
        .await;

    match output {
        Ok(o) => {
             json!({ "success": o.status.success(), "output": String::from_utf8_lossy(&o.stdout).to_string() })
        },
        Err(e) => json!({ "success": false, "message": e.to_string() })
    }
}

/// MirroCapture v1 saves the ORIGINAL Android screenshot. Separate WebView2
/// pen annotations are NOT included. Only an explicit user click triggers ADB
/// and no phone screenshot bytes are sent over the network by MirroLeap.
const MAX_SCREENSHOT_PNG_SIZE: usize = 24 * 1024 * 1024;

fn validate_capture_device(device: &str) -> Result<(), String> {
    if device.is_empty() || device.len() > 160 || device.starts_with('-') ||
        !device.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
    {
        return Err("Invalid Android device identifier.".into());
    }
    Ok(())
}

fn valid_android_screenshot_png(bytes: &[u8]) -> bool {
    if bytes.len() < 45 || bytes.len() > MAX_SCREENSHOT_PNG_SIZE ||
        !bytes.starts_with(b"\x89PNG\r\n\x1a\n") ||
        bytes[8..12] != [0, 0, 0, 13] ||
        &bytes[12..16] != b"IHDR" ||
        bytes[bytes.len() - 12..bytes.len() - 8] != [0, 0, 0, 0] ||
        &bytes[bytes.len() - 8..bytes.len() - 4] != b"IEND"
    {
        return false;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap_or([0; 4]));
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap_or([0; 4]));
    (1..=16_384).contains(&width) && (1..=16_384).contains(&height)
}

async fn capture_device_png(device: &str, custom_path: Option<String>) -> Result<Vec<u8>, String> {
    let adb = get_binary_path("adb", custom_path);
    let mut command = create_command(&adb);
    command.arg("-s").arg(device).args(["exec-out", "screencap", "-p"])
        .stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    let mut child = command.spawn().map_err(|e| format!("Cannot start ADB screenshot: {e}"))?;
    let capture = async {
        let stdout = child.stdout.take().ok_or("Cannot read ADB screenshot output")?;
        let mut png = Vec::new();
        stdout.take((MAX_SCREENSHOT_PNG_SIZE + 1) as u64).read_to_end(&mut png)
            .await.map_err(|e| format!("Cannot receive screenshot: {e}"))?;
        if png.len() > MAX_SCREENSHOT_PNG_SIZE {
            return Err("Screenshot too large (24 MiB limit).".into());
        }
        let status = child.wait().await.map_err(|e| format!("ADB screenshot failed: {e}"))?;
        if !status.success() || !valid_android_screenshot_png(&png) {
            return Err("Android did not return a valid PNG. Check device authorization and connection.".into());
        }
        Ok::<Vec<u8>, String>(png)
    };
    timeout(Duration::from_secs(20), capture).await
        .map_err(|_| "Android screenshot timed out after 20 seconds.".to_string())?
}

fn save_screenshot_to_downloads(
    app_handle: &tauri::AppHandle, png: &[u8], prefix: &str,
) -> Result<String, String> {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    let downloads = app_handle.path().download_dir()
        .map_err(|e| format!("Downloads directory unavailable: {e}"))?;
    fs::create_dir_all(&downloads).map_err(|e| format!("Cannot open Downloads directory: {e}"))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    // Names are deterministic and never expose the phone serial. create_new
    // prevents overwriting existing files or following a symlink.
    for attempt in 0..100 {
        let suffix = if attempt == 0 { String::new() } else { format!("-{attempt}") };
        let path = downloads.join(format!("{prefix}-{stamp}{suffix}.png"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut output) => {
                if let Err(error) = output.write_all(&png).and_then(|_| output.sync_all()) {
                    drop(output);
                    let _ = fs::remove_file(&path);
                    return Err(format!("Cannot save screenshot: {error}"));
                }
                return Ok(path.to_string_lossy().into_owned());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Cannot create screenshot file: {e}")),
        }
    }
    Err("Too many screenshots with the same timestamp. Retry shortly.".into())
}

#[tauri::command]
pub async fn save_device_screenshot(
    window: Window, app_handle: tauri::AppHandle,
    device: String, custom_path: Option<String>,
) -> Result<String, String> {
    require_main_window(&window)?;
    validate_capture_device(&device)?;
    let png = capture_device_png(&device, custom_path).await?;
    save_screenshot_to_downloads(&app_handle, &png, "MirroLeap-Screenshot")
}

/// Preview ONLY screenshots previously saved by MirroCapture under the
/// canonical Downloads folder. A caller may not fetch arbitrary local image
/// paths (or exploit a symlink/junction to read outside the Downloads area).
fn allowed_capture_preview_name(filename: &str) -> bool {
    if filename.len() > 128 || filename.is_empty() {
        return false;
    }
    let body = filename.strip_suffix(".png").unwrap_or("");
    let suffix = body.strip_prefix("MirroLeap-Screenshot-")
        .or_else(|| body.strip_prefix("MirroLeap-Annotated-"));
    suffix.is_some_and(|value| {
        let parts: Vec<&str> = value.split('-').collect();
        (parts.len() == 2 || parts.len() == 3) &&
        parts[0].len() == 8 && parts[0].bytes().all(|b| b.is_ascii_digit()) &&
        parts[1].len() == 6 && parts[1].bytes().all(|b| b.is_ascii_digit()) &&
        (parts.len() == 2 || (
            (1..=2).contains(&parts[2].len()) &&
            parts[2].bytes().all(|b| b.is_ascii_digit()) &&
            parts[2].parse::<u32>().is_ok_and(|n| (1..100).contains(&n))
        ))
    })
}

fn trusted_preview_path(downloads: &std::path::Path, requested: &str)
    -> Result<std::path::PathBuf, String>
{
    let path = std::path::Path::new(requested);
    let name = path.file_name().and_then(|name| name.to_str())
        .ok_or("Screenshot path has no valid filename")?;
    if !path.is_absolute() || !allowed_capture_preview_name(name) {
        return Err("Only files created by MirroCapture may be previewed".into());
    }
    let home = downloads.canonicalize()
        .map_err(|e| format!("Downloads folder unavailable: {e}"))?;
    let file = path.canonicalize()
        .map_err(|e| format!("Screenshot is no longer available: {e}"))?;
    // A generated-looking filename symlink must never reveal a different
    // local image, even when both happen to be under the Downloads folder.
    if file.file_name().and_then(|name| name.to_str()) != Some(name) {
        return Err("Screenshot was moved or replaced; capture it again".into());
    }
    if file.parent() != Some(home.as_path()) {
        return Err("Screenshot must be directly inside your Downloads folder".into());
    }
    Ok(file)
}

#[tauri::command]
pub async fn preview_saved_screenshot(
    window: Window,
    app_handle: tauri::AppHandle,
    path: String,
) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    require_main_window(&window)?;
    if path.len() > 1200 {
        return Err("Screenshot path is too long".into());
    }
    let folder = app_handle.path().download_dir()
        .map_err(|e| format!("Cannot resolve Downloads folder: {e}"))?;
    let source = trusted_preview_path(&folder, &path)?;
    const PREVIEW_LIMIT: u64 = 12 * 1024 * 1024;
    let bytes = std::fs::metadata(&source)
        .map_err(|e| format!("Cannot inspect screenshot: {e}"))?
        .len();
    if bytes > PREVIEW_LIMIT {
        return Err("Screenshot was saved, but exceeds the 12 MiB preview limit.".into());
    }
    let screenshot = tokio::fs::read(&source).await
        .map_err(|e| format!("Could not load preview: {e}"))?;
    if screenshot.len() as u64 > PREVIEW_LIMIT || !valid_android_screenshot_png(&screenshot) {
        return Err("Saved PNG is invalid or too large to preview.".into());
    }
    // The PNG may be heavily compressed: limit the *decoded* pixel count
    // before passing bytes into WebView2's PNG decoder, not just file length.
    let width = u32::from_be_bytes(screenshot[16..20].try_into()
        .map_err(|_| "PNG width is unavailable")?) as u64;
    let height = u32::from_be_bytes(screenshot[20..24].try_into()
        .map_err(|_| "PNG height is unavailable")?) as u64;
    if width.saturating_mul(height) > 16_000_000 {
        return Err("Screenshot saved, but its dimensions exceed the preview limit.".into());
    }
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(&screenshot)))
}

#[cfg(test)]
mod saved_screenshot_preview_tests {
    use super::{allowed_capture_preview_name, trusted_preview_path};

    #[test]
    fn only_generated_screenshot_names_are_allowed() {
        for name in [
            "MirroLeap-Screenshot-20261016-120000.png",
            "MirroLeap-Annotated-20261016-120000-42.png",
        ] {
            assert!(allowed_capture_preview_name(name));
        }
        for name in [
            "secret.png", ".hidden.png", "MirroLeap-Screenshot-abc.png",
            "MirroLeap-Screenshot-20261016-120000.exe", "../MirroLeap-Screenshot-20261016-120000.png",
            "MirroLeap-Screenshot-20261016-120000.png:stream",
            "MirroLeap-Screenshot-20261016-120000-000.png",
            "MirroLeap-Annotated-20261016-120000-999.png",
            "MirroLeap-Screenshot-20261016-120000-.png",
        ] {
            assert!(!allowed_capture_preview_name(name), "{name}");
        }
    }

    #[test]
    fn rejects_relative_paths_and_directories_before_opening() {
        let cwd = std::env::current_dir().unwrap();
        assert!(trusted_preview_path(&cwd,
            "MirroLeap-Screenshot-20261016-120000.png").is_err());
        assert!(trusted_preview_path(&cwd, "../secret.png").is_err());
    }
}

/// Main-window-only presentation tools, scoped to an existing Android session
/// and its own native phone-frame-*. Never inject an arbitrary device/JS label.
#[tauri::command]
pub async fn presentation_set_mode(
    window: Window,
    app_handle: tauri::AppHandle,
    device: String,
    mode: String,
) -> Result<String, String> {
    require_main_window(&window)?;
    validate_capture_device(&device)?;
    #[cfg(target_os = "windows")]
    {
        crate::device_frame::set_presentation_mode(&app_handle, &device, &mode).await
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app_handle, mode);
        Err("MirroPresent requires the Windows live phone frame".into())
    }
}

#[tauri::command]
pub fn presentation_get_mode(window: Window, device: String) -> Result<String, String> {
    require_main_window(&window)?;
    validate_capture_device(&device)?;
    #[cfg(target_os = "windows")]
    { Ok(crate::device_frame::presentation_mode(&device)) }
    #[cfg(not(target_os = "windows"))]
    { Ok("off".into()) }
}

/// The snapshot magnifier is EXPLICITLY STILL (not a frame stream). Fetch
/// one Android screencap in memory, verify its dimensions & orientation,
/// then wait for WebView2 Image.onload before reporting success.
#[tauri::command]
pub async fn presentation_refresh_snapshot(
    window: Window,
    app_handle: tauri::AppHandle,
    device: String,
    custom_path: Option<String>,
) -> Result<(), String> {
    require_main_window(&window)?;
    validate_capture_device(&device)?;
    #[cfg(target_os = "windows")]
    {
        if crate::device_frame::presentation_mode(&device) != "magnifier" {
            return Err("Enable the MirroPresent magnifier before refreshing its still image".into());
        }
        let png = capture_device_png(&device, custom_path).await?;
        crate::device_frame::set_presentation_snapshot(&app_handle, &device, &png).await
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app_handle, custom_path);
        Err("MirroPresent snapshot magnifier requires Windows".into())
    }
}

/// Export only user-drawn annotations composited onto an Android screencap.
/// If no active Windows phone frame/drawing exists, return a clear error;
/// never silently save an unannotated or incorrectly rotated screenshot.
#[tauri::command]
pub async fn save_annotated_screenshot(
    window: Window, app_handle: tauri::AppHandle,
    device: String, custom_path: Option<String>,
) -> Result<String, String> {
    require_main_window(&window)?;
    validate_capture_device(&device)?;
    #[cfg(target_os = "windows")]
    {
        let snapshot = crate::device_frame::capture_ink_snapshot(&app_handle, &device).await?;
        if snapshot.stroke_count == 0 {
            return Err("Draw at least one stroke before saving an annotated image".into());
        }
        let raw_png = capture_device_png(&device, custom_path).await?;
        let annotated = tokio::task::spawn_blocking(move || {
            crate::capture_compositor::composite_annotated_png(
                &raw_png, &snapshot.png_data_url, &snapshot.orientation
            )
        }).await.map_err(|e| format!("Annotation image compositor stopped unexpectedly: {e}"))??;
        save_screenshot_to_downloads(&app_handle, &annotated, "MirroLeap-Annotated")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app_handle, custom_path);
        Err("Annotated screenshot requires the Windows phone frame overlay".into())
    }
}

#[cfg(test)]
mod screenshot_capture_tests {
    use super::{validate_capture_device, valid_android_screenshot_png, MAX_SCREENSHOT_PNG_SIZE};
    fn synthetic_png() -> Vec<u8> {
        let mut bytes = vec![0u8; 60];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes[8..12].copy_from_slice(&13_u32.to_be_bytes());
        bytes[12..16].copy_from_slice(b"IHDR");
        bytes[16..20].copy_from_slice(&1_u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&1_u32.to_be_bytes());
        bytes[52..56].copy_from_slice(b"IEND");
        bytes
    }
    #[test]
    fn device_identifier_cannot_become_an_adb_option_or_path() {
        let very_long = "a".repeat(200);
        for serial in ["192.168.2.226:5555", "emulator-5554", "adb-XXXX._adb-tls-connect._tcp"] {
            assert!(validate_capture_device(serial).is_ok());
        }
        for serial in ["", "-a", "../secret", "a/b", "a\\b", "x\ny", "abc\0def", very_long.as_str()] {
            assert!(validate_capture_device(serial).is_err(), "accepted {serial:?}");
        }
    }
    #[test]
    fn screenshot_accepts_only_bounded_png_with_dimensions_and_iend() {
        let good = synthetic_png();
        assert!(valid_android_screenshot_png(&good));
        assert!(!valid_android_screenshot_png(&good[..36]));
        let mut bad = good.clone();
        bad[0] = b'x';
        assert!(!valid_android_screenshot_png(&bad));
        bad = good.clone();
        bad[20..24].copy_from_slice(&0_u32.to_be_bytes());
        assert!(!valid_android_screenshot_png(&bad));
        bad = good.clone();
        bad[52..56].copy_from_slice(b"FAIL");
        assert!(!valid_android_screenshot_png(&bad));
        bad = good.clone();
        bad[8..12].copy_from_slice(&10_u32.to_be_bytes());
        assert!(!valid_android_screenshot_png(&bad));
        bad = good.clone();
        bad[48..52].copy_from_slice(&2_u32.to_be_bytes());
        assert!(!valid_android_screenshot_png(&bad));
        assert!(!valid_android_screenshot_png(&vec![0u8; MAX_SCREENSHOT_PNG_SIZE + 1]));
    }
}

/// The advanced terminal intentionally accepts arbitrary ADB/scrcpy arguments,
/// but ONLY from the main application window. The translucent phone webviews
/// must not be able to invoke it, even if a future CSP regression occurs.
fn parse_terminal_input(raw: &str) -> Result<Vec<String>, String> {
    if raw.len() > 2048 || raw.chars().any(|c| matches!(c, '\r' | '\n' | '\0')) {
        return Err("Command must be one line, at most 2048 bytes.".into());
    }
    let parts = split_args(raw)?;
    if parts.is_empty() {
        return Err("No command provided".into());
    }
    Ok(parts)
}

fn capped_console_output(bytes: &[u8]) -> String {
    const MAX_OUTPUT: usize = 64 * 1024;
    let head = &bytes[..bytes.len().min(MAX_OUTPUT)];
    let mut rendered = String::from_utf8_lossy(head).into_owned();
    if bytes.len() > MAX_OUTPUT {
        rendered.push_str("\n[output truncated at 64 KiB]");
    }
    rendered
}

#[tauri::command]
pub async fn run_terminal_command(window: Window, device: Option<String>, cmd: String, custom_path: Option<String>) -> serde_json::Value {
    if window.label() != "main" {
        return json!({ "success": false, "message": "Only the main window can run terminal commands." });
    }
    let mut parts = match parse_terminal_input(&cmd) {
        Ok(parts) => parts,
        Err(e) => return json!({"success": false, "message": e}),
    };

    let first_part = parts[0].to_lowercase();
    let is_scrcpy = first_part == "scrcpy";
    let is_adb = first_part == "adb";

    let binary_name = if is_scrcpy { "scrcpy" } else { "adb" };
    let exe_path = get_binary_path(binary_name, custom_path);

    // If they explicitly typed "adb" or "scrcpy", remove it from arguments
    if is_adb || is_scrcpy {
        parts.remove(0);
    }

    let mut args = Vec::new();
    
    // Auto-inject device ID for ADB/Scrcpy commands if a device is active and not already specified
    let has_serial_flag = parts.contains(&"-s".to_string()) || parts.contains(&"--serial".to_string());
    
    if !has_serial_flag {
        if let Some(ref d) = device {
            if !d.is_empty() {
                // For ADB, don't inject for certain global commands
                let is_global_adb = binary_name == "adb" && !parts.is_empty() && (parts[0] == "devices" || parts[0] == "connect" || parts[0] == "pair");
                
                if !is_global_adb {
                    args.push("-s".to_string());
                    args.push(d.clone());
                }
            }
        }
    }

    for part in parts {
        args.push(part);
    }

    let mut process = create_command(&exe_path);
    process.args(&args).kill_on_drop(true);
    // Explicitly bound even for commands like 'adb logcat'. Unbounded output
    // and dangling ADB children must not stall the GUI indefinitely.
    let output = timeout(Duration::from_secs(20), process.output()).await;

    match output {
        Ok(Ok(o)) => {
             json!({
                 "success": o.status.success(),
                 "binary": binary_name,
                 "stdout": capped_console_output(&o.stdout),
                 "stderr": capped_console_output(&o.stderr)
             })
        },
        Ok(Err(e)) => json!({ "success": false, "message": e.to_string() }),
        Err(_) => json!({ "success": false, "message": "Command timed out after 20 seconds. The child process was stopped." })
    }
}

fn split_args(s: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let chars = s.chars();

    for c in chars {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                args.push(current.clone());
                current.clear();
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    if in_quotes {
        return Err("Unclosed quotes".to_string());
    }
    Ok(args)
}

#[tauri::command]
pub async fn push_file(window: Window, device: String, file_path: String, custom_path: Option<String>) -> serde_json::Value {
    if let Err(message) = require_main_window(&window) {
        return json!({ "success": false, "message": message });
    }
    let adb_path = get_binary_path("adb", custom_path);
    
    let output = create_command(&adb_path)
        .arg("-s")
        .arg(&device)
        .arg("push")
        .arg(&file_path)
        .arg("/sdcard/Download/")
        .output()
        .await;

    match output {
         Ok(o) => {
             if o.status.success() {
                 json!({ "success": true, "message": "File pushed to Downloads" })
             } else {
                 json!({ "success": false, "message": "Transfer failed" })
             }
         },
         Err(e) => json!({ "success": false, "message": e.to_string() })
    }
}

#[tauri::command]
pub async fn install_apk(window: Window, device: String, file_path: String, custom_path: Option<String>) -> serde_json::Value {
    if let Err(message) = require_main_window(&window) {
        return json!({ "success": false, "message": message });
    }
    let adb_path = get_binary_path("adb", custom_path);
    
    let output = create_command(&adb_path)
        .arg("-s")
        .arg(&device)
        .arg("install")
        .arg(&file_path)
        .output()
        .await;

     match output {
        Ok(o) => {
             let out_text = String::from_utf8_lossy(&o.stdout);
             let err_text = String::from_utf8_lossy(&o.stderr);
             
             if o.status.success() {
                 json!({ "success": true, "message": out_text.trim() })
             } else {
                 json!({ "success": false, "message": err_text.trim() })
             }
        },
        Err(e) => json!({ "success": false, "message": e.to_string() })
    }
}

#[tauri::command]
pub async fn kill_adb(window: Window, custom_path: Option<String>) -> Result<serde_json::Value, String> {
    require_main_window(&window)?;
    let adb_path = get_binary_path("adb", custom_path);
    let _ = window.emit("scrcpy-log", "[SYSTEM] Terminating ADB stack...".to_string());
    
    let mut child = create_command(&adb_path)
        .arg("kill-server")
        .spawn()
        .map_err(|e| e.to_string())?;
        
    let _ = child.wait().await;

    // Force kill adb process via OS specifics
    #[cfg(target_os = "windows")]
    {
        let _ = TokioCommand::new("taskkill")
            .args(&["/F", "/IM", "adb.exe", "/T"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .await;
    }

    #[cfg(not(target_os = "windows"))]
    {
         let _ = TokioCommand::new("pkill")
            .arg("adb")
            .output()
            .await;
    }

    let _ = window.emit("scrcpy-log", "[SYSTEM] ADB Stack Terminated.".to_string());
    Ok(json!({ "success": true, "message": "ADB Stack Terminated" }))
}

#[tauri::command]
pub async fn list_scrcpy_options(device: String, arg: String, custom_path: Option<String>) -> serde_json::Value {
    let exe_path = get_binary_path("scrcpy", custom_path);
    
    let mut command = create_command(&exe_path);
    command.arg("-s").arg(&device).arg(&arg);
    
    let output = command.output().await;

    match output {
        Ok(o) => {
             let out_text = String::from_utf8_lossy(&o.stdout);
             let err_text = String::from_utf8_lossy(&o.stderr); // scrcpy often prints lists to stderr
             let combined = format!("{}{}", out_text, err_text);
             json!({ "success": o.status.success(), "output": combined })
        },
        Err(e) => json!({ "success": false, "message": e.to_string() })
    }
}

fn detect_host_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

fn render_driver_label(driver: &str) -> &'static str {
    match driver {
        "direct3d" => "D3D11 (Direct3D)",
        "opengl" => "OpenGL",
        "opengles2" => "OpenGL ES 2",
        "opengles" => "OpenGL ES",
        "metal" => "Metal",
        "software" => "Software",
        "vulkan" => "Vulkan",
        _ => "Custom",
    }
}

fn is_driver_allowed_on_os(driver: &str, host_os: &str) -> bool {
    match host_os {
        "windows" => driver != "metal",
        "macos" => driver != "direct3d",
        "linux" => driver != "direct3d" && driver != "metal",
        _ => true,
    }
}

fn detect_render_drivers_from_help(help_output: &str) -> Vec<String> {
    let known = [
        "direct3d",
        "opengl",
        "opengles2",
        "opengles",
        "metal",
        "software",
        "vulkan",
    ];

    let lower = help_output.to_lowercase();
    let render_context = if let Some(start) = lower.find("--render-driver") {
        let end = (start + 1600).min(lower.len());
        lower[start..end].to_string()
    } else {
        String::new()
    };

    known
        .iter()
        .filter(|driver| {
            render_context.contains(&format!("\"{}\"", driver))
                || render_context.contains(&format!("'{}'", driver))
                || render_context.contains(*driver)
        })
        .map(|driver| (*driver).to_string())
        .collect()
}

#[tauri::command]
pub async fn get_render_drivers(custom_path: Option<String>) -> serde_json::Value {
    let exe_path = get_binary_path("scrcpy", custom_path);
    let host_os = detect_host_os();

    let output = create_command(&exe_path).arg("--help").output().await;

    match output {
        Ok(o) => {
            let out_text = String::from_utf8_lossy(&o.stdout);
            let err_text = String::from_utf8_lossy(&o.stderr);
            let combined = format!("{}{}", out_text, err_text);
            let lower = combined.to_lowercase();

            let supports_render_driver = lower.contains("--render-driver");
            if !supports_render_driver {
                return json!({
                    "success": true,
                    "hostOs": host_os,
                    "supportsRenderDriver": false,
                    "detectedDrivers": [],
                    "supportedDrivers": [],
                    "diagnostics": "scrcpy does not advertise --render-driver in --help output"
                });
            }

            let detected_drivers = detect_render_drivers_from_help(&combined);
            let supported_drivers: Vec<serde_json::Value> = detected_drivers
                .iter()
                .filter(|driver| is_driver_allowed_on_os(driver, host_os))
                .map(|driver| {
                    json!({
                        "id": driver,
                        "label": render_driver_label(driver)
                    })
                })
                .collect();

            json!({
                "success": o.status.success(),
                "hostOs": host_os,
                "supportsRenderDriver": true,
                "detectedDrivers": detected_drivers,
                "supportedDrivers": supported_drivers,
                "diagnostics": "render drivers parsed from scrcpy --help"
            })
        }
        Err(e) => json!({
            "success": false,
            "hostOs": host_os,
            "supportsRenderDriver": false,
            "detectedDrivers": [],
            "supportedDrivers": [],
            "message": e.to_string()
        }),
    }
}

async fn probe_video_decoder_support(exe_path: &str) -> Result<VideoDecoderSupport, String> {
    async fn probe(exe_path: &str, argument: &str) -> Result<String, String> {
        let mut command = create_command(exe_path);
        command.arg(argument).kill_on_drop(true);
        let output = timeout(Duration::from_secs(5), command.output())
            .await.map_err(|_| format!("scrcpy {argument} timed out after 5 seconds"))?
            .map_err(|error| format!("Cannot inspect installed scrcpy core: {error}"))?;
        if !output.status.success() {
            return Err(format!("scrcpy {argument} failed; core unavailable or damaged"));
        }
        if output.stdout.len().saturating_add(output.stderr.len()) > 128 * 1024 {
            return Err("scrcpy capability output exceeds 128 KiB".into());
        }
        Ok(format!("{}\n{}", String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)))
    }
    let version = probe(exe_path, "--version").await?;
    let help = probe(exe_path, "--help").await?;
    Ok(video_decoder::analyze_support(&version, &help, video_decoder::host_os()))
}

#[tauri::command]
pub async fn get_video_decoder_support(
    window: Window,
    custom_path: Option<String>,
) -> Result<VideoDecoderSupport, String> {
    require_main_window(&window)?;
    let exe_path = get_binary_path("scrcpy", custom_path);
    probe_video_decoder_support(&exe_path).await
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScrcpyConfig {
    device: String,
    session_mode: String,
    // ... other fields matching JS ...
    bitrate: Option<u32>,
    fps: Option<u32>,
    stay_awake: Option<bool>,
    turn_off: Option<bool>,
    audio_enabled: Option<bool>,
    audio_codec: Option<String>,
    always_on_top: Option<bool>,
    fullscreen: Option<bool>,
    borderless: Option<bool>,
    /// Draw a click-through iPhone-inspired bezel above the live SDL window (Windows only).
    device_frame: Option<bool>,
    record: Option<bool>,
    record_path: Option<String>,
    scrcpy_path: Option<String>,
    otg_pure: Option<bool>,
    camera_facing: Option<String>,
    camera_id: Option<String>,
    codec: Option<String>,
    camera_ar: Option<String>,
    camera_high_speed: Option<bool>,
    vd_width: Option<u32>,
    vd_height: Option<u32>,
    vd_dpi: Option<u32>,
    rotation: Option<String>,
    res: Option<String>,
    hid_keyboard: Option<bool>,
    hid_mouse: Option<bool>,
    render_driver: Option<String>,
    /// Host-side video decoding, independent of the Android encoder and SDL
    /// renderer: auto (default), disabled (software), or OS-specific hwdec.
    hardware_decoder: Option<String>,
    /// Ask the installed scrcpy core to print measured FPS to its log.
    print_fps: Option<bool>,
    // v4 features
    flex_display: Option<bool>,
    camera_torch: Option<bool>,
    camera_zoom: Option<f32>,
    background_color: Option<String>,
    keep_active: Option<bool>,
    /// Force SDL renderer VSync on/off (anti-tearing). scrcpy 4.0 (SDL3)
    /// disables renderer VSync by default; this re-asserts it via the SDL hint.
    vsync: Option<bool>,
    /// Last known screen position of the scrcpy mirror window. Captured (client
    /// area top-left) while a windowed session runs and restored via
    /// --window-x / --window-y on the next launch, so the window reopens where
    /// the user left it, including when relaunching borderless.
    window_x: Option<i32>,
    window_y: Option<i32>,
    // v4.1 features
    /// Ignore video encoder size constraints entirely (scrcpy v4.1+).
    /// Useful when the device reports incorrect encoder limits that prevent
    /// scrcpy from starting.
    ignore_video_encoder_constraints: Option<bool>,
}

fn resolve_audio_codec_flag<'a>(config: &'a ScrcpyConfig, audio_codec_override: Option<&'a str>) -> Option<&'a str> {
    if let Some(c) = audio_codec_override {
        let trimmed = c.trim();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    config.audio_codec.as_deref().and_then(|c| {
        let trimmed = c.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Parses `KEY=value` shell output (as printed by `xdotool ... --shell`) and
/// returns the integer value for `key`. Only compiled where it is used (the
/// Linux capture path, and the unit tests).
#[cfg(any(test, target_os = "linux"))]
fn parse_shell_var_int(text: &str, key: &str) -> Option<i32> {
    let prefix = format!("{}=", key);
    text.lines()
        .find(|l| l.starts_with(&prefix))
        .and_then(|l| l[prefix.len()..].trim().parse().ok())
}

/// Windows: finds the first visible, unowned top-level window belonging to
/// process `pid` (matches .NET's `MainWindowHandle` heuristic), i.e. scrcpy's
/// SDL mirror window rather than some owned tool/popup window. Shared by the
/// position-capture and recenter paths so both agree on which window they
/// mean.
#[cfg(target_os = "windows")]
pub(crate) fn find_window_by_pid(pid: u32) -> Option<windows::Win32::Foundation::HWND> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowThreadProcessId, IsWindowVisible, GW_OWNER,
    };

    // State threaded through EnumWindows via LPARAM.
    struct Search {
        target_pid: u32,
        found: HWND,
    }

    // WNDENUMPROC: BOOL(1) keeps enumerating, BOOL(0) stops.
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam.0 as *mut Search);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        if GetWindow(hwnd, GW_OWNER).map(|o| !o.0.is_null()).unwrap_or(false) {
            return BOOL(1);
        }
        let mut wnd_pid: u32 = 0;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut wnd_pid as *mut u32));
        if wnd_pid == search.target_pid {
            search.found = hwnd;
            return BOOL(0); // found it -> stop enumeration
        }
        BOOL(1)
    }

    let mut search = Search {
        target_pid: pid,
        found: HWND::default(),
    };
    // EnumWindows returns Err both on real failure and when the callback
    // returns BOOL(0) (our intentional early stop), so ignore the Result and
    // read the captured handle instead.
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut search as *mut Search as isize));
    }
    (!search.found.0.is_null()).then_some(search.found)
}

/// Queries the on-screen position of the scrcpy SDL window owned by process
/// `pid`, returning the client-area top-left in screen pixels. Returns `None`
/// silently whenever the window can't be read (not created yet, minimized,
/// tooling missing, permission denied); every failure mode is non-fatal.
///
/// The reference frame differs per platform and is reconciled afterwards by
/// [`resolve_window_pos_to_persist`], rather than by hardcoding decoration
/// sizes:
/// - Windows: Win32 `ClientToScreen` -> client origin (matches scrcpy/SDL).
/// - Linux/X11 (+XWayland): `xdotool` -> window origin (usually the client area).
/// - macOS: `osascript`/System Events -> frame origin (title-bar top).
fn try_capture_window_pos(pid: u32) -> Option<(i32, i32)> {
    #[cfg(target_os = "windows")]
    {
        // Native Win32: this replaces spawning PowerShell + `Add-Type` (a C#
        // JIT) on every poll, it is a couple of direct syscalls with no child
        // process.
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsZoomed};

        if let Some(hwnd) = find_window_by_pid(pid) {
            // A minimized window reports an off-screen sentinel origin
            // (~ -32000,-32000) and a maximized one the monitor edge; neither is
            // the windowed position we want, so skip them and keep the last good
            // sample.
            let special = unsafe { IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool() };
            if !special {
                // ClientToScreen maps the client-area origin (0,0) into screen
                // pixels; GetWindowRect would return the outer frame and creep
                // the window up by the title-bar height on every relaunch.
                let mut pt = POINT { x: 0, y: 0 };
                if unsafe { ClientToScreen(hwnd, &mut pt) }.as_bool() {
                    return Some((pt.x, pt.y));
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        let pid_str = pid.to_string();
        let output = StdCommand::new("xdotool")
            .args(["search", "--pid", &pid_str, "getwindowgeometry", "--shell"])
            .output()
            .ok()?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            let x = parse_shell_var_int(&text, "X")?;
            let y = parse_shell_var_int(&text, "Y")?;
            return Some((x, y));
        }
    }
    #[cfg(target_os = "macos")]
    {
        // System Events needs Accessibility permission for the app (one-time
        // grant in System Settings). The try block swallows every error (no
        // window yet, permission denied, ...) so this stays a silent no-op
        // until the permission is granted.
        let script = format!(
            concat!(
                "tell application \"System Events\"\n",
                "try\n",
                "set proc to first process whose unix id is {}\n",
                "set pos to position of first window of proc\n",
                "((item 1 of pos) as string) & \" \" & ((item 2 of pos) as string)\n",
                "end try\n",
                "end tell"
            ),
            pid
        );
        let output = StdCommand::new("osascript")
            .args(["-e", &script])
            .output()
            .ok()?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            let mut parts = text.split_whitespace();
            let x: i32 = parts.next()?.parse().ok()?;
            let y: i32 = parts.next()?.parse().ok()?;
            return Some((x, y));
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    let _ = pid;
    None
}

/// Moves the scrcpy mirror window owned by `pid` to the centre of the primary
/// screen and brings it to the front. A safety net for a persisted position
/// that no longer lands on any connected monitor (e.g. it was saved while a
/// second monitor was attached, which has since been unplugged) -- the window
/// can reopen fully off-screen, with no title bar to drag back if borderless.
/// Best-effort and silent: does nothing if the window can't be found, is
/// already maximized (it fills its monitor; there is nothing to recover), or
/// the move fails.
fn recenter_window(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::RECT;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, GetWindowRect, IsIconic, IsZoomed, SetForegroundWindow,
            SetWindowPos, ShowWindow, SM_CXSCREEN, SM_CYSCREEN, SWP_NOSIZE, SWP_NOZORDER,
            SW_RESTORE,
        };

        let Some(hwnd) = find_window_by_pid(pid) else {
            return;
        };
        if unsafe { IsZoomed(hwnd) }.as_bool() {
            return;
        }
        if unsafe { IsIconic(hwnd) }.as_bool() {
            let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
        }
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
            return;
        }
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        let (screen_w, screen_h) =
            unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                None,
                (screen_w - w) / 2,
                (screen_h - h) / 2,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            )
        };
        let _ = unsafe { SetForegroundWindow(hwnd) };
    }
    #[cfg(target_os = "linux")]
    {
        let pid_str = pid.to_string();
        let Ok(search_out) = StdCommand::new("xdotool").args(["search", "--pid", &pid_str]).output() else {
            return;
        };
        let Some(window_id) = String::from_utf8_lossy(&search_out.stdout).lines().next().map(str::to_string) else {
            return;
        };

        let Ok(geom_out) = StdCommand::new("xdotool")
            .args(["getwindowgeometry", "--shell", &window_id])
            .output()
        else {
            return;
        };
        let geom_text = String::from_utf8_lossy(&geom_out.stdout);
        let (Some(w), Some(h)) = (
            parse_shell_var_int(&geom_text, "WIDTH"),
            parse_shell_var_int(&geom_text, "HEIGHT"),
        ) else {
            return;
        };

        let Ok(display_out) = StdCommand::new("xdotool").arg("getdisplaygeometry").output() else {
            return;
        };
        let display_text = String::from_utf8_lossy(&display_out.stdout);
        let mut parts = display_text.split_whitespace();
        let (Some(Ok(screen_w)), Some(Ok(screen_h))) = (
            parts.next().map(str::parse::<i32>),
            parts.next().map(str::parse::<i32>),
        ) else {
            return;
        };

        let x = ((screen_w - w).max(0) / 2).to_string();
        let y = ((screen_h - h).max(0) / 2).to_string();
        let _ = StdCommand::new("xdotool").args(["windowmove", &window_id, &x, &y]).output();
        let _ = StdCommand::new("xdotool").args(["windowactivate", &window_id]).output();
    }
    #[cfg(target_os = "macos")]
    {
        // Same Accessibility-permission requirement as the position capture
        // above; a silent no-op until it is granted.
        let script = format!(
            concat!(
                "tell application \"System Events\"\n",
                "try\n",
                "set proc to first process whose unix id is {}\n",
                "set win to first window of proc\n",
                "set {{winW, winH}} to size of win\n",
                "tell application \"Finder\" to set screenBounds to bounds of window of desktop\n",
                "set screenW to (item 3 of screenBounds)\n",
                "set screenH to (item 4 of screenBounds)\n",
                "set position of win to {{((screenW - winW) / 2), ((screenH - winH) / 2)}}\n",
                "set frontmost of proc to true\n",
                "end try\n",
                "end tell"
            ),
            pid
        );
        let _ = StdCommand::new("osascript").args(["-e", &script]).output();
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    let _ = pid;
}

/// Re-centres the running mirror window for `device` on the primary screen.
/// Bound to a global OS shortcut (Ctrl+Alt+Shift+C, see `shortcuts.rs`) so a
/// window whose saved position ends up off-screen (see [`recenter_window`])
/// can be recovered without being able to see or focus it directly. A no-op
/// if `device` has no tracked process (e.g. nothing is running).
pub fn recenter_device(state: &ScrcpyState, device: &str) {
    let pid = state.processes.lock().unwrap().get(device).and_then(|c| c.id());
    if let Some(pid) = pid {
        recenter_window(pid);
    }
}

#[tauri::command]
pub fn recenter_scrcpy_window(state: State<'_, ScrcpyState>, device: String) -> Result<(), String> {
    recenter_device(&state, &device);
    Ok(())
}

/// Records which device the GUI currently considers selected, so the global
/// recentre shortcut (which fires with no window necessarily focused) knows
/// which mirror window to act on.
#[tauri::command]
pub fn set_active_device(state: State<'_, ScrcpyState>, device: Option<String>) -> Result<(), String> {
    *state.active_device.lock().unwrap() = device;
    Ok(())
}

/// Converts a raw captured position into the value to persist as
/// --window-x/--window-y so the window round-trips exactly on the next launch.
///
/// [`try_capture_window_pos`] reports coordinates in each OS tool's native
/// frame, which may differ from scrcpy's own (SDL) reference by the window
/// decorations, a difference that varies by platform, window manager, theme
/// and SDL version. Instead of hardcoding decoration sizes we learn the offset
/// live: when we launched at an explicit position (`requested`), the delta
/// between it and the *first* sample (`first`, taken before the user can move
/// the window) is the decoration offset; subtracting it from the last sample
/// yields a value that round-trips. Offsets larger than any plausible window
/// decoration are ignored (the window was moved before the first sample, or was
/// placed by the WM), falling back to the raw `last` position.
fn resolve_window_pos_to_persist(
    requested: Option<(i32, i32)>,
    first: Option<(i32, i32)>,
    last: (i32, i32),
) -> (i32, i32) {
    if let (Some((rx, ry)), Some((fx, fy))) = (requested, first) {
        let (dx, dy) = (fx - rx, fy - ry);
        if dx.abs() <= 80 && dy.abs() <= 120 {
            return (last.0 - dx, last.1 - dy);
        }
    }
    last
}

/// Emits the resolved window position (when one was captured) followed by the
/// session-stopped status. Shared by every monitor-loop exit path.
fn emit_window_pos_and_stop(
    window: &Window,
    device: &str,
    requested: Option<(i32, i32)>,
    first: Option<(i32, i32)>,
    last: Option<(i32, i32)>,
) {
    if let Some(last) = last {
        let (x, y) = resolve_window_pos_to_persist(requested, first, last);
        let _ = window.emit("scrcpy-window-pos", json!({ "device": device, "x": x, "y": y }));
    }
    let _ = window.emit("scrcpy-status", json!({ "device": device, "running": false }));
}

fn wants_device_frame(config: &ScrcpyConfig) -> bool {
    cfg!(target_os = "windows")
        && config.device_frame.unwrap_or(false)
        && config.session_mode == "mirror"
        && !config.fullscreen.unwrap_or(false)
        && !config.otg_pure.unwrap_or(false)
}

fn build_scrcpy_args(config: &ScrcpyConfig, video_dir_fallback: Option<String>, audio_codec_override: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();
    
    // Construct arguments based on config
    if !config.device.is_empty() {
        args.push("-s".to_string());
        args.push(config.device.clone());
    }

    let codec = config.codec.as_deref().unwrap_or("h264");
    args.push(format!("--video-codec={}", codec));

    let otg_pure = config.otg_pure.unwrap_or(false);
    let hid_keyboard = config.hid_keyboard.unwrap_or(false);
    let hid_mouse = config.hid_mouse.unwrap_or(false);

    if config.session_mode == "mirror" && (hid_keyboard || hid_mouse) && otg_pure {
        if config.device.contains('.') || config.device.contains(':') {
             args.push("--no-video".to_string());
             args.push("--no-audio".to_string());
             args.push("--keyboard=uhid".to_string());
             args.push("--mouse=uhid".to_string());
        } else {
             args.push("--otg".to_string());
        }
    } else {
        if hid_keyboard {
            args.push("--keyboard=uhid".to_string());
        }
        if hid_mouse {
            args.push("--mouse=uhid".to_string());
        }

        if let Some(render_driver) = &config.render_driver {
            let selected_driver = render_driver.trim();
            if !selected_driver.is_empty() && selected_driver != "auto" {
                args.push("--render-driver".to_string());
                args.push(selected_driver.to_string());
            }
        }

        if let Some(bitrate) = config.bitrate {
            args.push("--video-bit-rate".to_string());
            args.push(format!("{}M", bitrate));
        }
        
        let audio_enabled = config.audio_enabled.unwrap_or(true);
        if !audio_enabled { args.push("--no-audio".to_string()); }
        if audio_enabled {
            if let Some(codec) = resolve_audio_codec_flag(config, audio_codec_override) {
                args.push(format!("--audio-codec={}", codec));
            }
        }
        if let Some(aot) = config.always_on_top { if aot { args.push("--always-on-top".to_string()); } }
        if let Some(fs) = config.fullscreen { if fs { args.push("--fullscreen".to_string()); } }
        if config.borderless.unwrap_or(false) || wants_device_frame(config) {
            args.push("--window-borderless".to_string());
        }

        // Restore the last saved window position (client-area origin), for
        // mirror sessions only. Skipped in fullscreen (scrcpy ignores it) and
        // for camera/desktop, whose windows must not share, or overwrite, the
        // mirror window's remembered position.
        if config.session_mode == "mirror" && !config.fullscreen.unwrap_or(false) {
            if let Some(wx) = config.window_x {
                args.push(format!("--window-x={}", wx));
            }
            if let Some(wy) = config.window_y {
                args.push(format!("--window-y={}", wy));
            }
        }
        
        if let Some(rot) = &config.rotation {
            if rot != "0" {
                args.push("--orientation".to_string());
                args.push(rot.clone());
            }
        }

        let can_control = config.session_mode != "camera";
        if can_control {
             if let Some(sa) = config.stay_awake { if sa { args.push("--stay-awake".to_string()); } }
             if let Some(ka) = config.keep_active { if ka { args.push("--keep-active".to_string()); } }
             if let Some(to) = config.turn_off { if to { args.push("--turn-screen-off".to_string()); args.push("--no-power-on".to_string()); } }
        }

        if config.session_mode == "camera" {
            args.push("--video-source=camera".to_string());
            if let Some(cid) = &config.camera_id {
                if !cid.is_empty() { args.push(format!("--camera-id={}", cid)); }
                else if let Some(facing) = &config.camera_facing { args.push(format!("--camera-facing={}", facing)); }
            } else if let Some(facing) = &config.camera_facing { args.push(format!("--camera-facing={}", facing)); }
            
            // Resolution logic: Map res (e.g., "1920") to standard camera sizes (e.g., "1920x1080")
            if let Some(res) = &config.res {
                if res != "0" {
                    let camera_size = match res.as_str() {
                        "3840" => "3840x2160",
                        "2560" => "2560x1440",
                        "1920" => "1920x1080",
                        "1600" => "1600x900",
                        "1280" => "1280x720",
                        "1024" => "1024x576",
                        "800" => "800x480",
                        _ => "1920x1080",
                    };
                    args.push(format!("--camera-size={}", camera_size));
                } else {
                    // Safe fallback default to avoid camera configuration error on non-standard sensor photo sizes (e.g. 4080x3060)
                    args.push("--camera-size=1920x1080".to_string());
                }
            } else {
                args.push("--camera-size=1920x1080".to_string());
            }

            if let Some(ar) = &config.camera_ar { if ar != "0" { args.push(format!("--camera-ar={}", ar)); } }
            if let Some(chs) = config.camera_high_speed { if chs { args.push("--camera-high-speed".to_string()); } }
            // v4: camera torch and zoom
            if let Some(true) = config.camera_torch { args.push("--camera-torch".to_string()); }
            if let Some(zoom) = config.camera_zoom {
                if zoom > 1.005 { // avoid floating point noise at 1.0
                    args.push(format!("--camera-zoom={:.1}", zoom));
                }
            }
             // fps handled generically below
        } else if config.session_mode == "desktop" {
             let w = config.vd_width.unwrap_or(1920);
             let h = config.vd_height.unwrap_or(1080);
             let dpi = config.vd_dpi.unwrap_or(420);
             args.push(format!("--new-display={}x{}/{}", w, h, dpi));
             args.push("--video-buffer=100".to_string());
             // v4: flex display (resize virtual display with window)
             if let Some(true) = config.flex_display { args.push("--flex-display".to_string()); }
        }
        
        if let Some(fps) = config.fps {
            if fps > 0 {
                if config.session_mode == "camera" {
                    args.push("--camera-fps".to_string());
                } else {
                    args.push("--max-fps".to_string());
                }
                args.push(fps.to_string());
            }
        } else if config.session_mode == "camera" && config.camera_high_speed.unwrap_or(false) {
            args.push("--camera-fps".to_string());
            args.push("60".to_string());
        }

        // Shared resolution logic (applies to mirror and desktop)
        if config.session_mode != "camera" {
            if let Some(res) = &config.res { 
                if res != "0" { 
                    args.push("--max-size".to_string()); 
                    args.push(res.clone()); 
                } 
            }
        }
        
        if let Some(rec) = config.record {
             if rec {
                 let mut path = config.record_path.clone().unwrap_or_default();
                 
                 // If path is empty, try to get Video dir fallback
                 if path.trim().is_empty() {
                    path = video_dir_fallback.unwrap_or_else(|| ".".to_string());
                 }

                 let filename = format!("scrcpy_{}_{}.mkv", config.device.replace(":", "-"), chrono::Local::now().format("%Y%m%d_%H%M%S"));
                 let full_path = std::path::Path::new(&path).join(filename);
                 args.push(format!("--record={}", full_path.to_string_lossy()));
             }
        }

        // v4: background color (all modes)
        if let Some(ref color) = config.background_color {
            let trimmed = color.trim();
            if !trimmed.is_empty() {
                args.push(format!("--background-color={}", trimmed));
            }
        }

        // v4.1: ignore video encoder size constraints
        if let Some(true) = config.ignore_video_encoder_constraints {
            args.push("--ignore-video-encoder-constraints".to_string());
        }
    }
    
    args
}

fn build_scrcpy_args_with_decoder(
    config: &ScrcpyConfig,
    video_dir_fallback: Option<String>,
    audio_codec_override: Option<&str>,
    decoder: &DecoderDecision,
) -> Vec<String> {
    let mut args = build_scrcpy_args(config, video_dir_fallback, audio_codec_override);
    // Validated against the installed core's own --help, never pass
    // arbitrary localStorage strings as scrcpy CLI flags.
    if let Some(flag) = &decoder.flag { args.push(flag.clone()); }
    if decoder.print_fps { args.push("--print-fps".into()); }
    args
}

async fn spawn_scrcpy_streams(
    window: &Window,
    device: &str,
    exe_path: &str,
    adb_exe_path: &str,
    server_path: Option<&str>,
    args: &[String],
    vsync: bool,
) -> Result<(tokio::process::Child, Arc<AtomicBool>, Arc<AtomicBool>), String> {
    let command_str = format!("> scrcpy {}", args.join(" "));
    let _ = window.emit("scrcpy-log", command_str);

    let mut command = create_command(exe_path);
    command.args(args);
    command.env("ADB", adb_exe_path);
    if let Some(sp) = server_path {
        if Path::new(sp).exists() {
            command.env("SCRCPY_SERVER_PATH", sp);
        }
    }
    // scrcpy 4.0 migrated from SDL2 to SDL3, which leaves the renderer's VSync
    // disabled by default and reintroduced screen tearing for users who had
    // none on scrcpy 3.x (SDL2). Re-assert VSync through SDL's hint env var so
    // the renderer syncs to the display refresh; "0" lets the user opt out
    // (slightly lower input latency) via the in-app toggle.
    command.env("SDL_RENDER_VSYNC", if vsync { "1" } else { "0" });
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|e| e.to_string())?;

    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stderr = child.stderr.take().expect("Failed to capture stderr");

    let audio_error_flag = Arc::new(AtomicBool::new(false));
    let decoder_error_flag = Arc::new(AtomicBool::new(false));

    let window_stdout = window.clone();
    let stdout_device = device.to_string();
    let stdout_decoder_error = decoder_error_flag.clone();
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut buffer = Vec::new();
        let mut interval = tokio::time::interval(Duration::from_millis(100));

        loop {
            tokio::select! {
                line_res = lines.next_line() => {
                    match line_res {
                        Ok(Some(line)) => {
                            if video_decoder::is_hardware_decode_error(&line) {
                                stdout_decoder_error.store(true, Ordering::SeqCst);
                            }
                            if let Some(actual) = video_decoder::observed_decoder(&line) {
                                let _ = window_stdout.emit("scrcpy-video-decoder", json!({
                                    "device": stdout_device.as_str(), "mode": actual,
                                }));
                            }
                            buffer.push(line);
                        },
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
                _ = interval.tick() => {
                    if !buffer.is_empty() {
                        let combined = buffer.join("\n");
                        let _ = window_stdout.emit("scrcpy-log", combined);
                        buffer.clear();
                    }
                }
            }
        }
        if !buffer.is_empty() {
            let _ = window_stdout.emit("scrcpy-log", buffer.join("\n"));
        }
    });

    let window_stderr = window.clone();
    let flag_clone = audio_error_flag.clone();
    let stderr_device = device.to_string();
    let stderr_decoder_error = decoder_error_flag.clone();
    tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        let mut buffer = Vec::new();
        let mut interval = tokio::time::interval(Duration::from_millis(100));

        loop {
            tokio::select! {
                line_res = lines.next_line() => {
                    match line_res {
                        Ok(Some(line)) => {
                            if is_audio_codec_error(&line) {
                                flag_clone.store(true, Ordering::SeqCst);
                            }
                            if video_decoder::is_hardware_decode_error(&line) {
                                stderr_decoder_error.store(true, Ordering::SeqCst);
                            }
                            if let Some(actual) = video_decoder::observed_decoder(&line) {
                                let _ = window_stderr.emit("scrcpy-video-decoder", json!({
                                    "device": stderr_device.as_str(), "mode": actual,
                                }));
                            }
                            buffer.push(line);
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
                _ = interval.tick() => {
                    if !buffer.is_empty() {
                        let combined = buffer.join("\n");
                        let _ = window_stderr.emit("scrcpy-log", combined);
                        buffer.clear();
                    }
                }
            }
        }
        if !buffer.is_empty() {
            let _ = window_stderr.emit("scrcpy-log", buffer.join("\n"));
        }
    });

    Ok((child, audio_error_flag, decoder_error_flag))
}

enum AttemptOutcome {
    Exited(String),
    WaitError(String),
    UserStopped,
}

#[tauri::command]
pub async fn run_scrcpy(window: Window, state: State<'_, ScrcpyState>, config: ScrcpyConfig, app_handle: tauri::AppHandle) -> Result<(), String> {
    require_main_window(&window)?;

    let video_dir = app_handle.path().video_dir().ok().map(|p| p.to_string_lossy().to_string());

    let exe_path = get_binary_path("scrcpy", config.scrcpy_path.clone());

    // Log the session details for the user
    let mode_label = match config.session_mode.as_str() {
        "camera" => "Camera Mode",
        "desktop" => "Desktop Mode",
        _ => "Screen Mirroring",
    };

    let res_label = config.res.as_ref().map(|r| if r == "0" { "Original" } else { r }).unwrap_or("Original");
    let bitrate_label = format!("{}Mbps", config.bitrate.unwrap_or(8));
    let fps_label = format!("{}fps", config.fps.unwrap_or(60));

    let _ = window.emit("scrcpy-log", format!("[SYSTEM] Starting {} session...", mode_label));
    let _ = window.emit("scrcpy-log", format!("[SYSTEM] Target: {} | Config: {} @ {}, {}", config.device, res_label, bitrate_label, fps_label));

    if config.record.unwrap_or(false) {
        let path = config.record_path.as_ref().map(|p| if p.is_empty() { "Videos" } else { p }).unwrap_or("Videos");
        let _ = window.emit("scrcpy-log", format!("[SYSTEM] Recording enabled -> output to {}", path));
    }

    let adb_exe_path = get_binary_path("adb", config.scrcpy_path.clone());
    let server_path = if !exe_path.is_empty() && exe_path != "scrcpy" {
        Path::new(&exe_path).parent().map(|p| p.join("scrcpy-server").to_string_lossy().to_string())
    } else {
        None
    };

    let _ = window.emit("scrcpy-log", format!("[SYSTEM] Using scrcpy: {}", exe_path));
    let _ = window.emit("scrcpy-log", format!("[SYSTEM] Using adb: {}", adb_exe_path));

    // Decide whether automatic audio codec fallback should kick in on errors.
    let audio_enabled = config.audio_enabled.unwrap_or(true);
    let audio_codec_mode = config.audio_codec.as_deref().unwrap_or("auto").trim();
    let should_auto_fallback = audio_enabled
        && (audio_codec_mode.is_empty() || audio_codec_mode.eq_ignore_ascii_case("auto"));

    // Video decoding is HOST-SIDE; do not confuse it with Android's video
    // encoder or SDL render driver. Auto is the old unmodified launch path.
    // Only probe the core for explicitly requested FPS/decoder features.
    let requested_decoder = config.hardware_decoder.as_deref().unwrap_or("auto").trim();
    let request_fps = config.print_fps.unwrap_or(false);
    let video_enabled = !(config.session_mode == "mirror" &&
        config.otg_pure.unwrap_or(false) &&
        (config.hid_keyboard.unwrap_or(false) || config.hid_mouse.unwrap_or(false)));
    let capabilities = if (requested_decoder != "auto" && !requested_decoder.is_empty()) || request_fps {
        match probe_video_decoder_support(&exe_path).await {
            Ok(support) => Some(support),
            Err(reason) => {
                let _ = window.emit("scrcpy-log", format!(
                    "[DECODER] Capability probe unavailable: {reason}. Keeping original scrcpy video flags."
                ));
                None
            }
        }
    } else { None };
    let decoder_decision = video_decoder::choose_decoder(
        config.hardware_decoder.as_deref(), request_fps,
        capabilities.as_ref(), video_enabled,
    );
    if let Some(warning) = &decoder_decision.warning {
        let _ = window.emit("scrcpy-log", format!("[DECODER] {warning}"));
    }
    let forced_hardware_decoder = decoder_decision.flag.as_ref()
        .is_some_and(|value| value != "--hwdec=disabled");
    let _ = window.emit("scrcpy-video-decoder", json!({
        "device": config.device, "mode": "starting",
    }));

    // First attempt: honour the user's audio codec choice. In "auto" mode this
    // launches without --audio-codec, matching the previous default behaviour.
    let initial_args = build_scrcpy_args_with_decoder(&config, video_dir.clone(), None, &decoder_decision);

    let (child, audio_error_flag, decoder_error_flag) = spawn_scrcpy_streams(
        &window,
        &config.device,
        &exe_path,
        &adb_exe_path,
        server_path.as_deref(),
        &initial_args,
        config.vsync.unwrap_or(true),
    ).await?;

    let initial_scrcpy_pid = child.id();
    state.processes.lock().unwrap().insert(config.device.clone(), child);

    // Render a *transparent shell* over the native SDL window; video rendering,
    // keyboard and pointer control remain scrcpy's own. Never launch the shell
    // for camera/desktop/fullscreen/OTG sessions.
    #[cfg(target_os = "windows")]
    if wants_device_frame(&config) {
        if let Err(e) = crate::device_frame::start(
            &app_handle, &config.device, config.always_on_top.unwrap_or(false)
        ) {
            let _ = window.emit("scrcpy-log", format!("[FRAME] Phone shell disabled: {e}. Screen mirroring remains available."));
        }
    }
    let _ = window.emit("scrcpy-status", json!({ "device": config.device, "running": true }));

    // Monitor for exit (and orchestrate the audio codec fallback chain)
    let device_mon = config.device.clone();
    let window_mon = window.clone();
    let app_handle_mon = window.app_handle().clone();
    let video_dir_mon = video_dir;
    let exe_path_mon = exe_path;
    let adb_exe_path_mon = adb_exe_path;
    let server_path_mon = server_path;
    let config_mon = config.clone();
    let initial_decoder_decision = decoder_decision;

    // Position we asked scrcpy to open at this launch (if any). Used to learn
    // the decoration offset so the saved position round-trips exactly. See
    // resolve_window_pos_to_persist.
    let requested_pos: Option<(i32, i32)> = match (config_mon.window_x, config_mon.window_y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };
    // Only track and persist the position for windowed mirror sessions. A
    // fullscreen session has no meaningful position (it would report the
    // monitor origin, e.g. 0,0), and camera/desktop sessions must not overwrite
    // the mirror window's remembered position.
    let track_pos = config_mon.session_mode == "mirror" && !config_mon.fullscreen.unwrap_or(false);

    tokio::spawn(async move {
        let mut current_audio_flag = audio_error_flag;
        let mut current_decoder_error_flag = decoder_error_flag;
        let mut current_decoder_decision = initial_decoder_decision;
        let mut forced_decoder_retry_used = false;
        let mut current_audio_override: Option<String> = None;
        let mut chain_index: usize = 0;
        let mut current_scrcpy_pid = initial_scrcpy_pid;
        let mut first_window_pos: Option<(i32, i32)> = None;
        let mut last_window_pos: Option<(i32, i32)> = None;

        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;

            // Sample the live window position on every tick. This is the only
            // path that can catch a move made right before the user closes the
            // mirror window itself (rather than through stop_scrcpy, which
            // grabs its own guaranteed-fresh sample): SDL destroys the window
            // before the process exits, so by the time that exit is detected
            // below the position is unreadable, and this periodic sample is
            // all that is left to persist. Sampling every tick instead of
            // every few seconds keeps that worst case down to ~500ms stale
            // instead of several seconds.
            if track_pos {
                if let Some(pid) = current_scrcpy_pid {
                    if let Some(pos) = try_capture_window_pos(pid) {
                        if first_window_pos.is_none() {
                            first_window_pos = Some(pos);
                        }
                        last_window_pos = Some(pos);
                    }
                }
            }

            let outcome = {
                let state_mon = app_handle_mon.state::<ScrcpyState>();
                let mut processes = state_mon.processes.lock().unwrap();
                match processes.get_mut(&device_mon) {
                    Some(child) => match child.try_wait() {
                        Ok(Some(status)) => {
                            processes.remove(&device_mon);
                            Some(AttemptOutcome::Exited(status.to_string()))
                        }
                        Ok(None) => None,
                        Err(e) => {
                            processes.remove(&device_mon);
                            Some(AttemptOutcome::WaitError(e.to_string()))
                        }
                    },
                    None => Some(AttemptOutcome::UserStopped),
                }
            };

            let outcome = match outcome {
                Some(o) => o,
                None => continue,
            };

            match outcome {
                AttemptOutcome::WaitError(e) => {
                    let _ = window_mon.emit("scrcpy-log", format!("[SYSTEM] Error waiting for scrcpy: {}", e));
                    emit_window_pos_and_stop(&window_mon, &device_mon, requested_pos, first_window_pos, last_window_pos);
                    break;
                }
                AttemptOutcome::UserStopped => {
                    // stop_scrcpy grabs one last, guaranteed-fresh sample right
                    // before killing the process -- prefer it over our own
                    // periodic sample, which can be up to ~3s stale if the
                    // window was moved and closed in the same breath.
                    let fresh_pos = app_handle_mon
                        .state::<ScrcpyState>()
                        .final_capture_hint
                        .lock()
                        .unwrap()
                        .remove(&device_mon);
                    let pos_to_persist = fresh_pos.or(last_window_pos);
                    emit_window_pos_and_stop(&window_mon, &device_mon, requested_pos, first_window_pos, pos_to_persist);
                    break;
                }
                AttemptOutcome::Exited(status) => {
                    let _ = window_mon.emit("scrcpy-log", format!("[SYSTEM] Scrcpy process exited with status: {}", status));

                    // Give stream reader tasks a moment to flush remaining stderr
                    // lines (the audio error pattern may still be in flight).
                    tokio::time::sleep(Duration::from_millis(250)).await;

                    let audio_failed = current_audio_flag.load(Ordering::SeqCst);
                    let decoder_failed = current_decoder_error_flag.load(Ordering::SeqCst);

                    if forced_hardware_decoder && decoder_failed && !forced_decoder_retry_used {
                        // An explicit GPU-only decoder may fail on a machine
                        // lacking suitable drivers. Retry ONCE with scrcpy's
                        // default auto (which has its own software fallback).
                        // Do not retry on unrelated USB/ADB or codec errors.
                        forced_decoder_retry_used = true;
                        current_decoder_decision.flag = None;
                        let _ = window_mon.emit("scrcpy-log",
                            "[DECODER] GPU decoder failed. Retrying once with automatic/software fallback.".to_string());
                        let _ = window_mon.emit("scrcpy-video-decoder", json!({
                            "device": device_mon.as_str(), "mode": "retrying",
                        }));
                        let retry_args = build_scrcpy_args_with_decoder(
                            &config_mon, video_dir_mon.clone(),
                            current_audio_override.as_deref(), &current_decoder_decision,
                        );
                        match spawn_scrcpy_streams(
                            &window_mon, &device_mon, &exe_path_mon, &adb_exe_path_mon,
                            server_path_mon.as_deref(), &retry_args,
                            config_mon.vsync.unwrap_or(true),
                        ).await {
                            Ok((new_child, new_audio_flag, new_decoder_flag)) => {
                                current_scrcpy_pid = new_child.id();
                                first_window_pos = None;
                                last_window_pos = None;
                                app_handle_mon.state::<ScrcpyState>()
                                    .processes.lock().unwrap()
                                    .insert(device_mon.clone(), new_child);
                                current_audio_flag = new_audio_flag;
                                current_decoder_error_flag = new_decoder_flag;
                                continue;
                            }
                            Err(reason) => {
                                let _ = window_mon.emit("scrcpy-log",
                                    format!("[DECODER] Automatic fallback also failed: {reason}"));
                                emit_window_pos_and_stop(&window_mon, &device_mon,
                                    requested_pos, first_window_pos, last_window_pos);
                                break;
                            }
                        }
                    } else if should_auto_fallback && audio_failed && chain_index < AUDIO_FALLBACK_CHAIN.len() {
                        let next_codec = AUDIO_FALLBACK_CHAIN[chain_index];
                        let _ = window_mon.emit(
                            "scrcpy-log",
                            format!("[SYSTEM] Default audio codec failed, retrying with {}...", next_codec.to_uppercase()),
                        );

                        let new_args = build_scrcpy_args_with_decoder(&config_mon,
                            video_dir_mon.clone(), Some(next_codec), &current_decoder_decision);

                        match spawn_scrcpy_streams(
                            &window_mon,
                            &device_mon,
                            &exe_path_mon,
                            &adb_exe_path_mon,
                            server_path_mon.as_deref(),
                            &new_args,
                            config_mon.vsync.unwrap_or(true),
                        ).await {
                            Ok((new_child, new_flag, new_decoder_flag)) => {
                                current_scrcpy_pid = new_child.id();
                                // New window: re-learn its decoration offset by
                                // resampling this attempt from scratch. Clear the
                                // last sample too, so we never persist the old
                                // window's position with the new (unlearned)
                                // offset if this attempt closes before its first
                                // sample.
                                first_window_pos = None;
                                last_window_pos = None;
                                let state_mon = app_handle_mon.state::<ScrcpyState>();
                                state_mon.processes.lock().unwrap().insert(device_mon.clone(), new_child);
                                current_audio_flag = new_flag;
                                current_decoder_error_flag = new_decoder_flag;
                                current_audio_override = Some(next_codec.to_string());
                                chain_index += 1;
                                continue;
                            }
                            Err(e) => {
                                let _ = window_mon.emit("scrcpy-log", format!("[SYSTEM] Failed to spawn retry: {}", e));
                                emit_window_pos_and_stop(&window_mon, &device_mon, requested_pos, first_window_pos, last_window_pos);
                                break;
                            }
                        }
                    } else if should_auto_fallback && audio_failed {
                        let _ = window_mon.emit(
                            "scrcpy-log",
                            "[SYSTEM] No compatible audio codec found. Consider disabling audio forwarding.".to_string(),
                        );
                        emit_window_pos_and_stop(&window_mon, &device_mon, requested_pos, first_window_pos, last_window_pos);
                        break;
                    } else {
                        emit_window_pos_and_stop(&window_mon, &device_mon, requested_pos, first_window_pos, last_window_pos);
                        break;
                    }
                }
            }
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config(session_mode: &str) -> ScrcpyConfig {
        ScrcpyConfig {
            device: "device1".to_string(),
            session_mode: session_mode.to_string(),
            bitrate: None,
            fps: None,
            stay_awake: None,
            turn_off: None,
            audio_enabled: None,
            audio_codec: None,
            always_on_top: None,
            fullscreen: None,
            borderless: None,
            device_frame: None,
            record: None,
            record_path: None,
            scrcpy_path: None,
            otg_pure: None,
            camera_facing: None,
            camera_id: None,
            codec: None,
            camera_ar: None,
            camera_high_speed: None,
            vd_width: None,
            vd_height: None,
            vd_dpi: None,
            rotation: None,
            res: None,
            hid_keyboard: None,
            hid_mouse: None,
            render_driver: None,
            hardware_decoder: None,
            print_fps: None,
            flex_display: None,
            camera_torch: None,
            camera_zoom: None,
            background_color: None,
            keep_active: None,
            vsync: None,
            window_x: None,
            window_y: None,
            ignore_video_encoder_constraints: None,
        }
    }

    #[test]
    fn test_device_frame_only_for_windows_windowed_mirror() {
        let mut config = base_config("mirror");
        config.device_frame = Some(true);
        let args = build_scrcpy_args(&config, None, None);
        assert_eq!(
            args.iter().filter(|a| a.as_str() == "--window-borderless").count(),
            if cfg!(target_os = "windows") { 1 } else { 0 },
        );

        config.fullscreen = Some(true);
        let fullscreen_args = build_scrcpy_args(&config, None, None);
        assert!(!fullscreen_args.contains(&"--window-borderless".to_string()));

        config.fullscreen = None;
        config.otg_pure = Some(true);
        let otg_args = build_scrcpy_args(&config, None, None);
        assert!(!otg_args.contains(&"--window-borderless".to_string()));

        config.otg_pure = None;
        config.session_mode = "camera".to_owned();
        let camera_args = build_scrcpy_args(&config, None, None);
        assert!(!camera_args.contains(&"--window-borderless".to_string()));
    }

    #[test]
    fn test_build_scrcpy_args_mirror_defaults() {
        let config = base_config("mirror");
        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"-s".to_string()));
        assert!(args.contains(&"device1".to_string()));
        assert!(args.contains(&"--video-codec=h264".to_string()));
    }

    #[test]
    fn decoder_flags_are_appended_only_from_validated_core_capabilities() {
        use crate::video_decoder::{analyze_support, choose_decoder};
        let mut config = base_config("mirror");
        config.hardware_decoder = Some("d3d11va".into());
        config.print_fps = Some(true);
        let help = "    --hwdec=mode\n      modes: auto, disabled, d3d11va\n    --print-fps\n";
        let old_core = analyze_support("scrcpy 4.1", help, "windows");
        let old_choice = choose_decoder(config.hardware_decoder.as_deref(), true, Some(&old_core), true);
        let old_args = build_scrcpy_args_with_decoder(&config, None, None, &old_choice);
        assert!(!old_args.iter().any(|s| s.starts_with("--hwdec=")));
        assert!(old_args.iter().any(|s| s == "--print-fps"));

        let new_core = analyze_support("scrcpy 5.0.1", help, "windows");
        let hw_choice = choose_decoder(config.hardware_decoder.as_deref(), true, Some(&new_core), true);
        let gpu_args = build_scrcpy_args_with_decoder(&config, None, None, &hw_choice);
        assert_eq!(gpu_args.iter().filter(|v| v.starts_with("--hwdec=")).count(), 1);
        assert!(gpu_args.contains(&"--hwdec=d3d11va".to_string()));
        assert_eq!(gpu_args.iter().filter(|v| *v == "--print-fps").count(), 1);
        assert!(gpu_args.contains(&"--video-codec=h264".to_string()));

        let fallback = choose_decoder(Some("auto"), true, Some(&new_core), true);
        let fallback_args = build_scrcpy_args_with_decoder(&config, None, None, &fallback);
        assert!(!fallback_args.iter().any(|s| s.starts_with("--hwdec=")));

        let invalid = choose_decoder(Some("--help"), false, Some(&new_core), true);
        assert!(build_scrcpy_args_with_decoder(&config, None, None, &invalid)
            .iter().all(|s| s != "--hwdec=--help"));
    }

    #[test]
    fn test_build_scrcpy_args_camera_mode() {
        let mut config = base_config("camera");
        config.fps = Some(30);
        config.camera_facing = Some("front".to_string());

        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--video-source=camera".to_string()));
        assert!(args.contains(&"--camera-facing=front".to_string()));
        assert!(args.contains(&"--camera-fps".to_string()));
        assert!(args.contains(&"30".to_string()));
    }

    #[test]
    fn test_build_scrcpy_args_bitrate_and_fps() {
        let mut config = base_config("mirror");
        config.bitrate = Some(8);
        config.fps = Some(60);

        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--video-bit-rate".to_string()));
        assert!(args.contains(&"8M".to_string()));
        assert!(args.contains(&"--max-fps".to_string()));
        assert!(args.contains(&"60".to_string()));
    }

    #[test]
    fn test_audio_codec_auto_adds_no_flag() {
        let mut config = base_config("mirror");
        config.audio_codec = Some("auto".to_string());

        let args = build_scrcpy_args(&config, None, None);
        assert!(!args.iter().any(|a| a.starts_with("--audio-codec")));
    }

    #[test]
    fn test_audio_codec_manual_adds_flag() {
        let mut config = base_config("mirror");
        config.audio_codec = Some("aac".to_string());

        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--audio-codec=aac".to_string()));
    }

    #[test]
    fn test_audio_codec_override_takes_precedence() {
        let mut config = base_config("mirror");
        config.audio_codec = Some("auto".to_string());

        let args = build_scrcpy_args(&config, None, Some("flac"));
        assert!(args.contains(&"--audio-codec=flac".to_string()));
    }

    #[test]
    fn test_audio_codec_skipped_when_audio_disabled() {
        let mut config = base_config("mirror");
        config.audio_enabled = Some(false);
        config.audio_codec = Some("aac".to_string());

        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--no-audio".to_string()));
        assert!(!args.iter().any(|a| a.starts_with("--audio-codec")));
    }

    #[test]
    fn test_is_audio_codec_error_matches_known_patterns() {
        assert!(is_audio_codec_error(
            "ERROR: Could not create default audio encoder for opus"
        ));
        assert!(is_audio_codec_error("Failed to initialize audio/opus"));
        assert!(is_audio_codec_error("audio codec not supported on device"));
        assert!(is_audio_codec_error("audio encoder failed to start"));
    }

    #[test]
    fn test_is_audio_codec_error_ignores_unrelated() {
        assert!(!is_audio_codec_error("ADB disconnected"));
        assert!(!is_audio_codec_error("device offline"));
        assert!(!is_audio_codec_error("video encoder error"));
        assert!(!is_audio_codec_error("permission issue"));
        assert!(!is_audio_codec_error("INFO Audio codec selected: opus"));
    }

    #[test]
    fn test_build_scrcpy_args_window_position() {
        let mut config = base_config("mirror");
        config.window_x = Some(100);
        config.window_y = Some(200);
        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--window-x=100".to_string()));
        assert!(args.contains(&"--window-y=200".to_string()));
    }

    #[test]
    fn test_build_scrcpy_args_window_position_negative_coords() {
        // Multi-monitor: a display left of / above the primary yields negative
        // coordinates, which scrcpy accepts.
        let mut config = base_config("mirror");
        config.window_x = Some(-1920);
        config.window_y = Some(0);
        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--window-x=-1920".to_string()));
        assert!(args.contains(&"--window-y=0".to_string()));
    }

    #[test]
    fn test_build_scrcpy_args_window_position_skipped_in_fullscreen() {
        let mut config = base_config("mirror");
        config.window_x = Some(100);
        config.window_y = Some(200);
        config.fullscreen = Some(true);
        let args = build_scrcpy_args(&config, None, None);
        assert!(!args.iter().any(|a| a.starts_with("--window-x")));
        assert!(!args.iter().any(|a| a.starts_with("--window-y")));
    }

    #[test]
    fn test_build_scrcpy_args_window_position_only_mirror() {
        // Camera (and desktop) sessions must not receive the mirror window's
        // remembered position, it is scoped to mirror mode.
        let mut config = base_config("camera");
        config.window_x = Some(100);
        config.window_y = Some(200);
        let args = build_scrcpy_args(&config, None, None);
        assert!(!args.iter().any(|a| a.starts_with("--window-x")));
        assert!(!args.iter().any(|a| a.starts_with("--window-y")));
    }

    #[test]
    fn test_parse_shell_var_int() {
        let text = "WINDOW=12345\nX=959\nY=24\nWIDTH=428\nHEIGHT=928\n";
        assert_eq!(parse_shell_var_int(text, "X"), Some(959));
        assert_eq!(parse_shell_var_int(text, "Y"), Some(24));
        assert_eq!(parse_shell_var_int(text, "MISSING"), None);
    }

    #[test]
    fn test_resolve_window_pos_first_launch_uses_raw() {
        // First launch ever (nothing requested): persist the raw last sample.
        assert_eq!(
            resolve_window_pos_to_persist(None, Some((100, 72)), (300, 222)),
            (300, 222)
        );
        assert_eq!(
            resolve_window_pos_to_persist(None, None, (300, 222)),
            (300, 222)
        );
    }

    #[test]
    fn test_resolve_window_pos_zero_offset_is_noop() {
        // Windows/Linux: the tool already reports the client origin, so the
        // first sample matches what we requested -> offset 0 -> last unchanged.
        let requested = Some((100, 100));
        let first = Some((100, 100));
        assert_eq!(
            resolve_window_pos_to_persist(requested, first, (640, 480)),
            (640, 480)
        );
    }

    #[test]
    fn test_resolve_window_pos_cancels_titlebar_offset() {
        // macOS-like: the tool reports the frame origin (title-bar top), 28 px
        // above the client origin scrcpy positions. Requested (100,100) but
        // first observed (100,72); the -28 px offset must be cancelled so the
        // persisted value maps back to the client origin.
        let requested = Some((100, 100));
        let first = Some((100, 72));
        assert_eq!(
            resolve_window_pos_to_persist(requested, first, (300, 222)),
            (300, 250)
        );
    }

    #[test]
    fn test_resolve_window_pos_cancels_full_frame_offset() {
        // Frame offset on both axes (border + title bar).
        let requested = Some((100, 100));
        let first = Some((92, 69));
        assert_eq!(
            resolve_window_pos_to_persist(requested, first, (292, 219)),
            (300, 250)
        );
    }

    #[test]
    fn test_resolve_window_pos_ignores_implausible_offset() {
        // The window was moved before the first sample (or placed by the WM):
        // the offset dwarfs any window decoration, so fall back to the raw last.
        let requested = Some((100, 100));
        let first = Some((900, 700));
        assert_eq!(
            resolve_window_pos_to_persist(requested, first, (950, 760)),
            (950, 760)
        );
    }

    #[test]
    fn test_parse_adb_device_line_usb_serial() {
        assert_eq!(
            parse_adb_device_line("ZY22MGW35T device"),
            Some(("ZY22MGW35T".to_string(), "device".to_string()))
        );
    }

    #[test]
    fn test_parse_adb_device_line_tcpip_serial() {
        assert_eq!(
            parse_adb_device_line("192.168.1.50:5555 device"),
            Some(("192.168.1.50:5555".to_string(), "device".to_string()))
        );
    }

    #[test]
    fn test_parse_adb_device_line_mdns_serial() {
        assert_eq!(
            parse_adb_device_line("adb-ZY22MGW35T-Z3uXXq._adb-tls-connect._tcp device"),
            Some((
                "adb-ZY22MGW35T-Z3uXXq._adb-tls-connect._tcp".to_string(),
                "device".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_adb_device_line_mdns_conflict_serial_with_space() {
        // mDNS conflict resolution appends " (2)" inside the serial itself, so
        // the line has a space that is NOT the serial/state separator.
        assert_eq!(
            parse_adb_device_line("adb-ZY22MGW35T-Z3uXXq (2)._adb-tls-connect._tcp device"),
            Some((
                "adb-ZY22MGW35T-Z3uXXq (2)._adb-tls-connect._tcp".to_string(),
                "device".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_adb_device_line_ignores_trailing_l_fields() {
        // `adb devices -l` appends product/model/transport_id fields after the
        // state; they must not affect the parsed serial or state.
        assert_eq!(
            parse_adb_device_line(
                "ZY22MGW35T device usb:1-1 product:foo model:bar device:baz transport_id:1"
            ),
            Some(("ZY22MGW35T".to_string(), "device".to_string()))
        );
    }

    #[test]
    fn test_extract_device_model_replaces_underscores_with_spaces() {
        assert_eq!(
            extract_device_model(
                "ZY22MGW35T device usb:1-1 product:foo model:SM_S911B device:baz transport_id:1"
            ),
            Some("SM S911B".to_string())
        );
    }

    #[test]
    fn test_extract_device_model_missing_field_returns_none() {
        assert_eq!(extract_device_model("ZY22MGW35T device"), None);
    }

    #[test]
    fn test_parse_adb_device_line_offline_and_unauthorized() {
        assert_eq!(
            parse_adb_device_line("ZY22MGW35T offline"),
            Some(("ZY22MGW35T".to_string(), "offline".to_string()))
        );
        assert_eq!(
            parse_adb_device_line("192.168.1.50:5555 unauthorized"),
            Some(("192.168.1.50:5555".to_string(), "unauthorized".to_string()))
        );
    }

    #[test]
    fn test_parse_adb_device_line_unparsable_returns_none() {
        assert_eq!(parse_adb_device_line(""), None);
        assert_eq!(parse_adb_device_line("List of devices attached"), None);
    }

    #[test]
    fn test_get_devices_filtering_keeps_only_device_state() {
        // Mirrors the filter chain in get_devices: only the exact "device"
        // state is kept, regardless of serial shape, including mDNS wireless
        // debugging serials (._tcp/._udp) that must no longer be excluded.
        let output = "List of devices attached\n\
             ZY22MGW35T device usb:1-1 product:foo model:bar transport_id:1\n\
             192.168.1.50:5555 device product:foo model:bar transport_id:2\n\
             adb-ZY22MGW35T-Z3uXXq._adb-tls-connect._tcp device product:foo transport_id:3\n\
             adb-ZY22MGW35T-Z3uXXq (2)._adb-tls-connect._tcp device product:foo transport_id:4\n\
             emulator-5554 offline\n\
             0123456789ABCDEF unauthorized\n";

        let devices: Vec<String> = output
            .lines()
            .skip(1)
            .filter_map(parse_adb_device_line)
            .filter(|(_, state)| state == "device")
            .map(|(serial, _)| serial)
            .collect();

        assert_eq!(
            devices,
            vec![
                "ZY22MGW35T".to_string(),
                "192.168.1.50:5555".to_string(),
                "adb-ZY22MGW35T-Z3uXXq._adb-tls-connect._tcp".to_string(),
                "adb-ZY22MGW35T-Z3uXXq (2)._adb-tls-connect._tcp".to_string(),
            ]
        );
    }
}

#[tauri::command]
pub async fn stop_scrcpy(window: Window, state: State<'_, ScrcpyState>, device: String) -> Result<(), String> {
    require_main_window(&window)?;
    stop_scrcpy_impl(state, device).await
}

/// Internal control is called only after device_frame::control has verified
/// the current toolbar window belongs to this device. Expose no unguarded
/// Tauri IPC entry point for arbitrary webviews to stop other sessions.
pub async fn stop_scrcpy_impl(state: State<'_, ScrcpyState>, device: String) -> Result<(), String> {
    let child = {
        let mut processes = state.processes.lock().unwrap();
        processes.remove(&device)
    };

    if let Some(mut c) = child {
        if let Some(pid) = c.id() {
             // Grab the window's position now, one last time, before it's
             // killed: this is the only moment it's guaranteed to still be
             // exactly where the user left it. The run_scrcpy monitor loop
             // only refreshes its own sample every ~3s, so stopping right
             // after a move could otherwise persist a stale, pre-move spot.
             if let Some(pos) = try_capture_window_pos(pid) {
                 state.final_capture_hint.lock().unwrap().insert(device.clone(), pos);
             }

             #[cfg(target_os = "windows")]
             {
                let _ = StdCommand::new("taskkill")
                    .args(&["/PID", &pid.to_string()])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
             }

             #[cfg(not(target_os = "windows"))]
             {
                 // Try graceful termination first via SIGTERM
                 let _ = StdCommand::new("kill")
                    .arg(pid.to_string())
                    .output();
             }

            // Give it a moment to finalize, but don't block too long
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        } else {
            let _ = c.kill().await;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn download_scrcpy(
    window: Window,
    state: State<'_, ScrcpyState>,
) -> Result<(), String> {
    require_main_window(&window)?;
    if !state.processes.lock().unwrap().is_empty() {
        return Err("Stop all running scrcpy sessions before updating the engine.".into());
    }
    crate::core_manager::install_latest(&window).await
}


/// Narrow acknowledgement channel from the local transparent phone-frame
/// page. Only the currently active phone-frame-* view with the matching nonce
/// can mark its Canvas command as executed.
#[tauri::command]
pub fn device_frame_ack(
    window: Window,
    token: u64,
    success: bool,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        crate::device_frame::acknowledge_ink(&window, token, success)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, token, success);
        Err("Annotation acknowledgements are Windows-only".into())
    }
}

/// One-shot transparent Canvas export from the actual phone-frame-* WebView.
/// The caller's native Tauri window label and a pending nonce must both
/// match; a main window or another device cannot impersonate this response.
#[tauri::command]
pub fn device_frame_capture_ack(
    window: Window,
    token: u64,
    png: String,
    orientation: String,
    strokes: usize,
    error: Option<String>,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        crate::device_frame::acknowledge_ink_capture(&window, token, png, orientation, strokes, error)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, token, png, orientation, strokes, error);
        Err("Annotated screenshots require a Windows phone frame".into())
    }
}

#[tauri::command]
pub async fn device_frame_control(
    window: Window,
    app_handle: tauri::AppHandle,
    action: String,
) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        crate::device_frame::control(&app_handle, &window, &action).await
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, app_handle, action);
        Err("Device frame controls are available only on Windows".into())
    }
}


#[tauri::command]
pub async fn get_videos_dir(app_handle: tauri::AppHandle) -> Result<String, String> {
    use tauri::Manager;
    app_handle.path().video_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

/// A fixed filename under Downloads is the only permitted report destination.
/// Never allow directory separators, Windows drive prefixes, alternate data
/// streams, hidden filenames or symlinks supplied by an IPC caller.
fn validated_report_filename(name: &str) -> Result<&str, String> {
    if name.is_empty() || name.len() > 96 || name.starts_with('.') ||
        !name.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
    {
        return Err("Invalid report filename. Use only letters, digits, '-', '_' and '.'.".into());
    }
    if ![".json", ".txt", ".log"].iter().any(|extension| name.ends_with(extension)) {
        return Err("Report must be .json, .txt or .log".into());
    }
    Ok(name)
}

#[tauri::command]
pub async fn save_report(window: Window, app_handle: tauri::AppHandle, content: String, name: String) -> Result<String, String> {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use tauri::Manager;

    if window.label() != "main" {
        return Err("Only the main window can export reports.".into());
    }
    let file_name = validated_report_filename(&name)?;
    if content.len() > 2 * 1024 * 1024 {
        return Err("Report exceeds the 2 MiB export limit.".into());
    }
    let downloads = app_handle.path().download_dir()
        .map_err(|e| e.to_string())?;
    fs::create_dir_all(&downloads).map_err(|e| format!("Cannot create Downloads directory: {e}"))?;
    let path = downloads.join(file_name);
    // create_new refuses overwrites and existing symlinks/reparse points.
    // Do not replace arbitrary files on repeated exports.
    let mut output = OpenOptions::new().write(true).create_new(true).open(&path)
        .map_err(|e| format!("Cannot create new report file: {e}"))?;
    if let Err(error) = output.write_all(content.as_bytes()) {
        drop(output);
        let _ = fs::remove_file(&path);
        return Err(format!("Cannot write report: {error}"));
    }
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod report_terminal_security_tests {
    use super::{validated_report_filename, parse_terminal_input, capped_console_output};

    #[test]
    fn report_exports_only_safe_flat_filenames() {
        for good in ["mirroleap-logs-42.json", "scrcpy-gui-logs-123.json", "report.txt", "a_b-1.log"] {
            assert_eq!(validated_report_filename(good).unwrap(), good);
        }
        let too_long = "abcdefghijklmnopqrstuv".repeat(6);
        for bad in ["", "../evil.json", "folder/file.log", "folder\\file.log",
            "C:\\temp\\target.json", ".hidden.json", "test.json:stream",
            "file.exe", "a\0b.json", "bad name.json", too_long.as_str(),
        ] {
            assert!(validated_report_filename(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn terminal_refuses_malformed_or_unbounded_inputs() {
        let too_long = "a".repeat(2049);
        for bad in ["", "  ", "adb shell \"unterminated", "adb devices\nexit", "adb\0devices",
            too_long.as_str()] {
            assert!(parse_terminal_input(bad).is_err(), "accepted {bad:?}");
        }
        assert_eq!(parse_terminal_input("adb shell getprop ro.product.model").unwrap()[0], "adb");
        assert_eq!(parse_terminal_input("devices -l").unwrap(),
            vec!["devices".to_string(), "-l".to_string()]);
    }

    #[test]
    fn terminal_output_has_visible_size_cap() {
        assert_eq!(capped_console_output("正常输出".as_bytes()), "正常输出");
        let big = vec![b'x'; 90_000];
        let truncated = capped_console_output(&big);
        assert!(truncated.len() < 70_000);
        assert!(truncated.contains("output truncated"));
    }
}

#[tauri::command]
pub async fn get_scrcpy_bin_dir() -> Result<String, String> {
    let current_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap());
        
    let extract_path = current_dir.join("scrcpy-bin");
    
    if extract_path.exists() {
        Ok(extract_path.to_string_lossy().to_string())
    } else {
        Ok(current_dir.to_string_lossy().to_string())
    }
}

pub fn get_local_scrcpy_version(custom_path: Option<String>) -> Option<String> {
    let exe_path = get_binary_path("scrcpy", custom_path);
    let mut cmd = std::process::Command::new(&exe_path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.arg("--version").output().ok()?;
    if output.status.success() {
        let out_str = String::from_utf8_lossy(&output.stdout);
        for line in out_str.lines() {
            if line.contains("scrcpy") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(idx) = parts.iter().position(|&x| x == "scrcpy") {
                    if let Some(version) = parts.get(idx + 1) {
                        return Some(version.trim_start_matches('v').to_string());
                    }
                }
            }
        }
    }
    None
}

pub async fn get_latest_scrcpy_version() -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("ScrcpyGui-Updater")
        .build()
        .map_err(|e| e.to_string())?;

    let api_url = "https://api.github.com/repos/Genymobile/scrcpy/releases/latest";
    let api_resp = client.get(api_url).send().await;

    let mut tag_name = String::new();
    let mut used_fallback = false;

    if let Ok(resp) = api_resp {
        if resp.status().is_success() {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(tag) = json["tag_name"].as_str() {
                    tag_name = tag.trim_start_matches('v').to_string();
                }
            }
        } else if resp.status() == reqwest::StatusCode::FORBIDDEN {
            used_fallback = true;
        }
    } else {
        used_fallback = true;
    }

    if used_fallback || tag_name.is_empty() {
        let redirect_res = client.get("https://github.com/Genymobile/scrcpy/releases/latest")
            .send().await
            .map_err(|e| format!("Fallback failed: {}", e))?;
        
        let final_url = redirect_res.url().as_str();
        if let Some(tag) = final_url.split('/').next_back() {
            if tag.starts_with('v') || tag.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                tag_name = tag.trim_start_matches('v').to_string();
            }
        }
    }

    if tag_name.is_empty() {
        return Err("Could not determine latest version".to_string());
    }

    Ok(tag_name)
}

fn compare_versions(local: &str, remote: &str) -> bool {
    let local_parts: Vec<u32> = local.split('.')
        .filter_map(|s| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok())
        .collect();
    let remote_parts: Vec<u32> = remote.split('.')
        .filter_map(|s| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok())
        .collect();

    for i in 0..std::cmp::max(local_parts.len(), remote_parts.len()) {
        let local_val = local_parts.get(i).cloned().unwrap_or(0);
        let remote_val = remote_parts.get(i).cloned().unwrap_or(0);
        if remote_val > local_val {
            return true;
        } else if local_val > remote_val {
            return false;
        }
    }
    false
}

#[tauri::command]
pub async fn check_scrcpy_update(custom_path: Option<String>) -> serde_json::Value {
    let local_version = match get_local_scrcpy_version(custom_path.clone()) {
        Some(v) => v,
        None => return json!({ "update_available": false, "local_version": null, "latest_version": null, "message": "Scrcpy not installed or not working" })
    };

    let latest_version = match get_latest_scrcpy_version().await {
        Ok(v) => v,
        Err(e) => return json!({ "update_available": false, "local_version": local_version, "latest_version": null, "message": format!("Could not fetch latest release: {}", e) })
    };

    let update_available = compare_versions(&local_version, &latest_version);

    json!({
        "update_available": update_available,
        "local_version": local_version,
        "latest_version": latest_version
    })
}



