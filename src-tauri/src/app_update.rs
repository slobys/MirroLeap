//! Read-only update checks for the MirroLeap DESKTOP application.
//!
//! Distinct from core_manager.rs, which can update the scrcpy engine.
//! No installer is downloaded, written, executed, or silently launched here:
//! the app has no trusted Windows Authenticode/update-signature configuration.
//! GitHub release metadata is untrusted input even over HTTPS. Validate the
//! tag, release status and every asset URL before displaying external links.

use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tauri::{AppHandle, Window};

const RELEASE_API: &str = "https://api.github.com/repos/slobys/MirroLeap/releases/latest";
const RELEASE_URL_BASE: &str = "https://github.com/slobys/MirroLeap/releases/tag/";
const ASSET_URL_BASE: &str = "https://github.com/slobys/MirroLeap/releases/download/";
const MAX_METADATA_BYTES: usize = 512 * 1024;
const MAX_CHANGELOG_CHARS: usize = 4000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseDownload {
    pub kind: String,
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    /// "update_available" / "up_to_date" / "ahead" (preview/testing build).
    pub status: String,
    pub release_url: String,
    pub published_at: Option<String>,
    pub changelog: String,
    pub downloads: Vec<ReleaseDownload>,
}

fn stable_version(input: &str) -> Option<[u32; 3]> {
    let value = input.strip_prefix('v').unwrap_or(input);
    if value.is_empty() || value.len() > 42 { return None; }
    let mut version = [0_u32; 3];
    let mut values = value.split('.');
    for item in &mut version {
        let part = values.next()?;
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) ||
            !part.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        *item = part.parse().ok()?;
    }
    if values.next().is_some() { return None; }
    Some(version)
}

fn official_assets(release: &Value, tag: &str, version: &str) -> Vec<ReleaseDownload> {
    let allowed = [
        ("portable", format!("MirroLeap_{version}_x64_Portable.zip")),
        ("exe", format!("MirroLeap_{version}_x64-setup.exe")),
        ("msi", format!("MirroLeap_{version}_x64_en-US.msi")),
        ("checksums", "SHA256SUMS.txt".to_string()),
    ];
    let Some(assets) = release.get("assets").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut collected = Vec::new();
    for (kind, name) in allowed {
        let expected_url = format!("{ASSET_URL_BASE}{tag}/{name}");
        let item = assets.iter().take(40).find(|asset| {
            asset.get("name").and_then(Value::as_str) == Some(name.as_str()) &&
            asset.get("browser_download_url").and_then(Value::as_str) == Some(expected_url.as_str())
        });
        let Some(asset) = item else { continue };
        let Some(size) = asset.get("size").and_then(Value::as_u64)
            .filter(|size| *size > 0 && *size <= 500 * 1024 * 1024)
        else { continue };
        let hash = asset.get("digest").and_then(Value::as_str)
            .and_then(|value| value.strip_prefix("sha256:"))
            .filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(|hex| hex.to_ascii_lowercase());
        collected.push(ReleaseDownload {
            kind: kind.to_string(), name, url: expected_url, size, sha256: hash,
        });
    }
    collected
}

fn interpret_release(current: &str, release: &Value) -> Result<AppUpdateInfo, String> {
    let local = stable_version(current).ok_or("invalid_local_version")?;
    // /releases/latest normally excludes drafts/prereleases. Still reject
    // explicitly flagged or malformed entries; never recommend test builds.
    if release.get("draft").and_then(Value::as_bool) != Some(false) ||
        release.get("prerelease").and_then(Value::as_bool) != Some(false)
    {
        return Err("invalid_release".into());
    }
    let tag = release.get("tag_name").and_then(Value::as_str)
        .ok_or("invalid_release")?;
    if !tag.starts_with('v') { return Err("invalid_release".into()); }
    let latest = stable_version(tag).ok_or("invalid_release")?;
    let version = tag.strip_prefix('v').ok_or("invalid_release")?;
    let status = if latest > local { "update_available" }
        else if latest == local { "up_to_date" }
        else { "ahead" };
    let changelog = release.get("body").and_then(Value::as_str)
        .unwrap_or("")
        .chars().take(MAX_CHANGELOG_CHARS).collect::<String>();
    let published_at = release.get("published_at").and_then(Value::as_str)
        .filter(|value| value.len() <= 40 && value.bytes().all(|b|
            b.is_ascii_digit() || matches!(b, b'-' | b':' | b'.' | b'T' | b'Z' | b'+')
        )).map(String::from);

    Ok(AppUpdateInfo {
        current_version: current.to_string(),
        latest_version: version.to_string(),
        status: status.into(),
        release_url: format!("{RELEASE_URL_BASE}{tag}"),
        published_at,
        changelog,
        downloads: official_assets(release, tag, version),
    })
}

#[tauri::command]
pub async fn check_mirroleap_update(
    window: Window,
    app_handle: AppHandle,
) -> Result<AppUpdateInfo, String> {
    if window.label() != "main" { return Err("main_window_only".into()); }
    let client = reqwest::Client::builder()
        .user_agent("MirroLeap-Desktop-UpdateCheck")
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(12))
        .build().map_err(|_| "network_error")?;

    let mut response = client.get(RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .send().await.map_err(|e| {
            if e.is_timeout() { "timeout" } else { "network_error" }
        })?;
    match response.status() {
        reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::TOO_MANY_REQUESTS =>
            return Err("rate_limited".into()),
        code if !code.is_success() => return Err("release_unavailable".into()),
        _ => {},
    }
    if response.content_length().is_some_and(|size| size > MAX_METADATA_BYTES as u64) {
        return Err("metadata_too_large".into());
    }
    let mut payload = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e|
        if e.is_timeout() { "timeout" } else { "network_error" }
    )? {
        if payload.len().saturating_add(chunk.len()) > MAX_METADATA_BYTES {
            return Err("metadata_too_large".into());
        }
        payload.extend_from_slice(&chunk);
    }
    let data: Value = serde_json::from_slice(&payload).map_err(|_| "invalid_release")?;
    let local = app_handle.package_info().version.to_string();
    interpret_release(&local, &data)
}

/// Do not turn a portable/extract-and-run ZIP into an opaque machine-wide
/// installer upgrade. Signed one-click NSIS/MSI updates are for installed
/// Windows applications; portable users keep the explicit ZIP workflow.
#[tauri::command]
pub fn can_install_signed_update(window: Window) -> Result<bool, String> {
    if window.label() != "main" { return Err("main_window_only".into()); }
    #[cfg(target_os = "windows")]
    {
        let executable = std::env::current_exe()
            .map_err(|_| "cannot_identify_install_type".to_string())?;
        let parent = executable.parent().ok_or("cannot_identify_install_type")?;
        let portable = parent.join("Run-MirroLeap.cmd").exists() ||
            parent.join("README-PORTABLE.txt").exists();
        // An EXE copied out of a ZIP may have lost its portable launcher.
        // Require an installed-system directory instead of treating every
        // file without Run-MirroLeap.cmd as an installed application.
        let installation_roots = ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"];
        let looks_installed = installation_roots.iter()
            .filter_map(|key| std::env::var_os(key))
            .map(std::path::PathBuf::from)
            .any(|root| parent.starts_with(&root));
        Ok(!portable && looks_installed)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_release() -> Value {
        json!({
            "tag_name": "v4.9.0", "draft": false, "prerelease": false,
            "published_at": "2026-10-15T12:00:00Z",
            "body": "Bug fixes and features",
            "assets": [
                {
                    "name": "MirroLeap_4.9.0_x64_Portable.zip",
                    "browser_download_url": "https://github.com/slobys/MirroLeap/releases/download/v4.9.0/MirroLeap_4.9.0_x64_Portable.zip",
                    "size": 5320000, "digest": format!("sha256:{}", "a".repeat(64))
                },
                {
                    "name": "MirroLeap_4.9.0_x64-setup.exe",
                    "browser_download_url": "https://evil.invalid/installer.exe",
                    "size": 5000000
                },
                {
                    "name": "SHA256SUMS.txt",
                    "browser_download_url": "https://github.com/slobys/MirroLeap/releases/download/v4.9.0/SHA256SUMS.txt",
                    "size": 340
                }
            ]
        })
    }

    #[test]
    fn stable_version_is_strict_and_numeric() {
        assert_eq!(stable_version("4.7.2"), Some([4, 7, 2]));
        assert_eq!(stable_version("v4.12.0"), Some([4, 12, 0]));
        for bad in ["", "v4.7", "v4.7.2-beta", "4.7.2+dev",
            "v4.7.02", "v4.7.2/../../", "v4.7.2.1", "v99999999999999.1.0"] {
            assert!(stable_version(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn compares_releases_without_confusing_development_builds() {
        let release = sample_release();
        assert_eq!(interpret_release("4.8.9", &release).unwrap().status, "update_available");
        assert_eq!(interpret_release("4.9.0", &release).unwrap().status, "up_to_date");
        assert_eq!(interpret_release("4.9.1", &release).unwrap().status, "ahead");
        assert_eq!(interpret_release("4.12.0", &release).unwrap().status, "ahead");
    }

    #[test]
    fn rejects_prereleases_and_unsafe_release_names() {
        let mut release = sample_release();
        release["prerelease"] = json!(true);
        assert!(interpret_release("4.8.0", &release).is_err());
        release["prerelease"] = json!(false);
        release["draft"] = json!(true);
        assert!(interpret_release("4.8.0", &release).is_err());
        release["draft"] = json!(false);
        for tag in ["latest", "v4.9.0-beta.1", "v4.9.0/../evil", "v4.9", "v4.09.0"] {
            release["tag_name"] = json!(tag);
            assert!(interpret_release("4.8.0", &release).is_err(), "{tag}");
        }
    }

    #[test]
    fn only_exposes_download_links_from_this_repository_with_exact_asset_names() {
        let info = interpret_release("4.8.1", &sample_release()).unwrap();
        assert_eq!(info.downloads.len(), 2);
        assert_eq!(info.downloads[0].kind, "portable");
        assert_eq!(info.downloads[0].sha256.as_deref(), Some("a".repeat(64).as_str()));
        assert_eq!(info.downloads[1].kind, "checksums");
        assert!(!info.downloads.iter().any(|asset| asset.kind == "exe"));
        assert!(info.downloads.iter().all(|asset|
            asset.url.starts_with("https://github.com/slobys/MirroLeap/releases/download/")
        ));
        assert!(info.release_url.ends_with("/tag/v4.9.0"));
    }

    #[test]
    fn does_not_trust_other_asset_sizes_or_unbounded_notes() {
        let mut release = sample_release();
        release["body"] = json!("x".repeat(6000));
        release["assets"][0]["size"] = json!(0);
        let info = interpret_release("4.8.0", &release).unwrap();
        assert_eq!(info.changelog.chars().count(), MAX_CHANGELOG_CHARS);
        assert!(!info.downloads.iter().any(|asset| asset.kind == "portable"));
    }
}
