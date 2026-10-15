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
use tokio::io::{BufReader, AsyncBufReadExt};
use tokio::time::{timeout, Duration};
use serde::Deserialize;

const AUDIO_FALLBACK_CHAIN: &[&str] = &["aac", "flac", "raw"];

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
    let silent = silent.unwrap_or(false);
    let adb_path = get_binary_path("adb", custom_path);
    let requested = ip.trim();
    if requested.is_empty() { return Ok(json!({ "success": false, "message": "Enter a device IP address" })); }
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
    ip: String,
    custom_path: Option<String>,
) -> serde_json::Value {
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
        for invalid in ["", "10.0.0.1:0", "10.0.0.1:99999", "example.com:5555", "256.0.0.1:5", "0.0.0.0:5555"] {
            assert!(parse_diagnostic_endpoint(invalid).is_none(), "{invalid}");
        }
    }
}

#[tauri::command]
pub async fn adb_pair(window: Window, ip: String, code: String, custom_path: Option<String>) -> Result<serde_json::Value, String> {
    let adb_path = get_binary_path("adb", custom_path);
    let _ = window.emit("scrcpy-log", format!("[SYSTEM] Pairing with {}...", ip));

    let output = create_command(&adb_path)
        .arg("pair")
        .arg(&ip)
        .arg(&code)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let out_text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let err_text = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !out_text.is_empty() { let _ = window.emit("scrcpy-log", format!("[ADB] {}", out_text)); }
    if !err_text.is_empty() { let _ = window.emit("scrcpy-log", format!("[ADB ERROR] {}", err_text)); }

    let success = output.status.success() && (out_text.contains("Successfully paired") || err_text.contains("Successfully paired"));

    Ok(json!({ "success": success, "message": if out_text.is_empty() { err_text } else { out_text } }))
}

#[tauri::command]
pub async fn adb_shell(device: String, command: String, custom_path: Option<String>) -> serde_json::Value {
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

#[tauri::command]
pub async fn run_terminal_command(device: Option<String>, cmd: String, custom_path: Option<String>) -> serde_json::Value {
    let mut parts = split_args(&cmd).unwrap_or_else(|_| cmd.split_whitespace().map(|s| s.to_string()).collect());
    if parts.is_empty() { return json!({ "success": false, "message": "No command provided" }); }

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

    let output = create_command(&exe_path)
        .args(&args)
        .output()
        .await;

    match output {
        Ok(o) => {
             json!({ 
                 "success": o.status.success(), 
                 "binary": binary_name,
                 "stdout": String::from_utf8_lossy(&o.stdout).to_string(),
                 "stderr": String::from_utf8_lossy(&o.stderr).to_string()
             })
        },
        Err(e) => json!({ "success": false, "message": e.to_string() })
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
pub async fn push_file(device: String, file_path: String, custom_path: Option<String>) -> serde_json::Value {
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
pub async fn install_apk(device: String, file_path: String, custom_path: Option<String>) -> serde_json::Value {
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

async fn spawn_scrcpy_streams(
    window: &Window,
    exe_path: &str,
    adb_exe_path: &str,
    server_path: Option<&str>,
    args: &[String],
    vsync: bool,
) -> Result<(tokio::process::Child, Arc<AtomicBool>), String> {
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

    let window_stdout = window.clone();
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut buffer = Vec::new();
        let mut interval = tokio::time::interval(Duration::from_millis(100));

        loop {
            tokio::select! {
                line_res = lines.next_line() => {
                    match line_res {
                        Ok(Some(line)) => buffer.push(line),
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

    Ok((child, audio_error_flag))
}

enum AttemptOutcome {
    Exited(String),
    WaitError(String),
    UserStopped,
}

#[tauri::command]
pub async fn run_scrcpy(window: Window, state: State<'_, ScrcpyState>, config: ScrcpyConfig, app_handle: tauri::AppHandle) -> Result<(), String> {

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

    // First attempt: honour the user's audio codec choice. In "auto" mode this
    // launches without --audio-codec, matching the previous default behaviour.
    let initial_args = build_scrcpy_args(&config, video_dir.clone(), None);

    let (child, audio_error_flag) = spawn_scrcpy_streams(
        &window,
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

                    if should_auto_fallback && audio_failed && chain_index < AUDIO_FALLBACK_CHAIN.len() {
                        let next_codec = AUDIO_FALLBACK_CHAIN[chain_index];
                        let _ = window_mon.emit(
                            "scrcpy-log",
                            format!("[SYSTEM] Default audio codec failed, retrying with {}...", next_codec.to_uppercase()),
                        );

                        let new_args = build_scrcpy_args(&config_mon, video_dir_mon.clone(), Some(next_codec));

                        match spawn_scrcpy_streams(
                            &window_mon,
                            &exe_path_mon,
                            &adb_exe_path_mon,
                            server_path_mon.as_deref(),
                            &new_args,
                            config_mon.vsync.unwrap_or(true),
                        ).await {
                            Ok((new_child, new_flag)) => {
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
pub async fn stop_scrcpy(state: State<'_, ScrcpyState>, device: String) -> Result<(), String> {
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

#[tauri::command]
pub async fn save_report(app_handle: tauri::AppHandle, content: String, name: String) -> Result<String, String> {
    use std::fs;
    use tauri::Manager;

    let downloads = app_handle.path().download_dir()
        .map_err(|e| e.to_string())?;
    
    let path = downloads.join(&name);
    fs::write(&path, content).map_err(|e| e.to_string())?;
    
    Ok(path.to_string_lossy().to_string())
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



