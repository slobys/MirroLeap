//! ADB's `push -p` emits progress on either output stream, often with
//! carriage returns rather than newline-delimited messages. Report only
//! percentages actually emitted by ADB, never guessed transfer rates.
use tauri::{Emitter, Window};
use tokio::io::{AsyncRead, AsyncReadExt};
use serde_json::json;

pub(crate) fn adb_push_percent(output: &str) -> Option<u8> {
    let bytes = output.as_bytes();
    let mut found = None;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'[' { continue; }
        let end = bytes.len().min(index + 10);
        let Some(close) = bytes[index + 1..end].iter().position(|&v| v == b']') else {
            continue;
        };
        let Ok(content) = std::str::from_utf8(&bytes[index + 1..index + 1 + close]) else {
            continue;
        };
        let Some(digits) = content.trim().strip_suffix('%') else { continue; };
        let digits = digits.trim();
        if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(percent) = digits.parse::<u8>() {
            if percent <= 100 { found = Some(percent); }
        }
    }
    found
}

/// Drain each pipe, avoiding adb deadlocks when both stdout/stderr are written.
/// Events are scoped to the actual transfer ID and do not reveal local paths.
pub(crate) async fn watch_adb_push_output<R: AsyncRead + Unpin>(
    mut pipe: R, window: Window, device: String, transfer_id: Option<String>,
) {
    let mut buffer = [0u8; 2048];
    let mut tail = String::new();
    let mut last_percent: Option<u8> = None;
    loop {
        let length = match pipe.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(size) => size,
        };
        tail.push_str(&String::from_utf8_lossy(&buffer[..length]));
        if let (Some(id), Some(percent)) = (transfer_id.as_deref(), adb_push_percent(&tail)) {
            if last_percent.is_none_or(|last| percent > last) {
                last_percent = Some(percent);
                let _ = window.emit("mirroleap-transfer-progress", json!({
                    "transferId": id, "device": device, "percent": percent,
                }));
            }
        }
        // Retain just enough text to recognize a "[ 42%]" token split
        // across adjacent reads. Never buffer file names or unbounded logs.
        tail = tail.chars().rev().take(24).collect::<String>().chars().rev().collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_only_adb_reported_bounded_progress() {
        assert_eq!(adb_push_percent("file: [  1%] /sdcard/Download/a"), Some(1));
        assert_eq!(adb_push_percent("x\r[ 73%] transferring\r[100%] done"), Some(100));
        assert_eq!(adb_push_percent("[1000%]"), None);
        assert_eq!(adb_push_percent("[101%]"), None);
        assert_eq!(adb_push_percent("[1x%]"), None);
        assert_eq!(adb_push_percent("moving 62% [no progress]"), None);
        assert_eq!(adb_push_percent(""), None);
    }
    #[test]
    fn carry_supports_split_frames_without_guessing() {
        assert_eq!(adb_push_percent("partial [ 4"), None);
        assert_eq!(adb_push_percent("partial [ 48%]"), Some(48));
        assert_eq!(adb_push_percent("filename [80%] \r[ 90%]"), Some(90));
    }
}
