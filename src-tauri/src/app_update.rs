//! MirroLeap desktop release discovery and explicitly clicked signed updates.
//! This is independent of the scrcpy engine updater. Only fixed official
//! GitHub release URLs may enter the Tauri updater, which verifies the signed
//! NSIS payload against the pinned public key before installation.

use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tauri::{AppHandle, Window, Emitter, Manager};
#[cfg(target_os = "windows")]
use tauri_plugin_updater::UpdaterExt;

const RELEASE_API: &str = "https://api.github.com/repos/slobys/MirroLeap/releases/latest";
const PREVIEW_RELEASES_API: &str = "https://api.github.com/repos/slobys/MirroLeap/releases?per_page=10";
const ALL_RELEASES_API: &str = "https://api.github.com/repos/slobys/MirroLeap/releases?per_page=20";
const RELEASE_URL_BASE: &str = "https://github.com/slobys/MirroLeap/releases/tag/";
const ASSET_URL_BASE: &str = "https://github.com/slobys/MirroLeap/releases/download/";
const MAX_METADATA_BYTES: usize = 512 * 1024;
const MAX_CHANGELOG_CHARS: usize = 4000;

/// Preview packages have a separate Windows app identity and MUST NOT use
/// stable MirroLeap's automatic installer/update channel.
fn isolated_hq_preview()->bool {option_env!("MIRROLEAP_HQ_PREVIEW").is_some()}

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
    pub prerelease: bool,
    /// stable or hq_preview. Only HQ binaries can select the HQ channel.
    pub channel: String,
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
        prerelease: release.get("prerelease").and_then(Value::as_bool).unwrap_or(false),
        channel:"stable".into(),
    })
}

fn has_official_updater(release: &Value, tag: &str, version: &str) -> bool {
    let Some(assets) = release.get("assets").and_then(Value::as_array) else { return false; };
    ["latest.json", &format!("MirroLeap_{version}_x64-setup.exe.sig")]
        .iter().all(|name| {
            let url = format!("{ASSET_URL_BASE}{tag}/{name}");
            assets.iter().take(40).any(|asset| {
                asset.get("name").and_then(Value::as_str) == Some(*name) &&
                asset.get("browser_download_url").and_then(Value::as_str) == Some(url.as_str()) &&
                asset.get("size").and_then(Value::as_u64).is_some_and(|n| n>0 && n<500000)
            })
        })
}

/// One user-facing channel: choose the numerically newest *complete* official
/// release whether it is a prerelease or stable. A merely recent GitHub tag
/// without its signed installer artifacts must not be offered as installable.
fn interpret_latest_release(current: &str, data: &Value) -> Result<AppUpdateInfo, String> {
    let releases = data.as_array().ok_or("invalid_release")?;
    let chosen = releases.iter().take(20).filter_map(|release| {
        if release.get("draft").and_then(Value::as_bool) != Some(false) ||
            release.get("prerelease").and_then(Value::as_bool).is_none()
        { return None; }
        let tag = release.get("tag_name")?.as_str()?;
        let version = tag.strip_prefix('v')?;
        let numbers = stable_version(version)?;
        if !has_official_updater(release, tag, version) ||
            !official_assets(release, tag, version).iter().any(|a|a.kind=="exe")
        { return None; }
        Some((numbers, release))
    }).max_by_key(|(version,_)| *version).map(|(_,release)| release)
        .ok_or("release_unavailable")?;
    let is_preview = chosen.get("prerelease").and_then(Value::as_bool).unwrap_or(false);
    let mut adapted = chosen.clone();
    adapted["prerelease"] = Value::Bool(false);
    let mut info = interpret_release(current, &adapted)?;
    info.prerelease = is_preview;
    Ok(info)
}

fn signed_manifest_url(version: &str) -> Result<reqwest::Url, String> {
    if version.starts_with('v') || stable_version(version).is_none() {
        return Err("invalid_release_version".into());
    }
    reqwest::Url::parse(&format!(
        "{ASSET_URL_BASE}v{version}/latest.json"
    )).map_err(|e| format!("Invalid official update endpoint: {e}"))
}

/// Strictly separate the HQ Preview channel from the stable vX.Y.Z
/// channel. Version numbers compare normally, but tags additionally embed
/// an Actions run number so repeated experimental builds do not collide.
const HQ_PREVIEW_TAG_PREFIX:&str="hq-preview-v";
const HQ_PREVIEW_SETUP_PREFIX:&str="MirroLeap.HQ.Preview_";
fn parse_hq_preview_tag(tag:&str)->Option<(&str,[u32;3],u64)> {
    let (version,run)=tag.strip_prefix(HQ_PREVIEW_TAG_PREFIX)?.rsplit_once("-run")?;
    let numbers=stable_version(version)?;
    if run.is_empty() || run.starts_with('0') ||
       !run.bytes().all(|c|c.is_ascii_digit()) {return None;}
    let run_number=run.parse::<u64>().ok().filter(|n|*n>0)?;
    Some((version,numbers,run_number))
}

fn hq_release_asset(release:&Value,tag:&str,kind:&str,name:&str)
    ->Option<ReleaseDownload> {
    let url=format!("{ASSET_URL_BASE}{tag}/{name}");
    let entry=release.get("assets")?.as_array()?.iter().take(40).find(|entry| {
        entry.get("name").and_then(Value::as_str)==Some(name) &&
        entry.get("browser_download_url").and_then(Value::as_str)==Some(url.as_str())
    })?;
    let size=entry.get("size")?.as_u64()
        .filter(|n|*n>0 && *n<500*1024*1024)?;
    let sha256=entry.get("digest").and_then(Value::as_str)
        .and_then(|s|s.strip_prefix("sha256:"))
        .filter(|s|s.len()==64 && s.bytes().all(|c|c.is_ascii_hexdigit()))
        .map(|s|s.to_ascii_lowercase());
    Some(ReleaseDownload{
        kind:kind.to_string(),name:name.to_string(),url,size,sha256,
    })
}
fn signed_hq_preview_release(release:&Value)->Option<([u32;3],u64,String)> {
    if release.get("prerelease").and_then(Value::as_bool)!=Some(true) ||
       release.get("draft").and_then(Value::as_bool)!=Some(false) {
        return None;
    }
    let tag=release.get("tag_name")?.as_str()?;
    let (version,numbers,run)=parse_hq_preview_tag(tag)?;
    let setup=format!("{HQ_PREVIEW_SETUP_PREFIX}{version}_x64-setup.exe");
    let signature=format!("{setup}.sig");
    // Both the signed executable and its version-specific manifest must exist
    // before ANY update notification. An unsigned HQ test build is not eligible.
    for name in [setup.as_str(),signature.as_str(),"latest.json","SHA256SUMS.txt"] {
        hq_release_asset(release,tag,"internal",name)?;
    }
    Some((numbers,run,tag.to_string()))
}

fn interpret_hq_preview_releases(current:&str,data:&Value)
    ->Result<AppUpdateInfo,String> {
    let installed=stable_version(current).ok_or("invalid_local_version")?;
    let list=data.as_array().ok_or("invalid_release")?;
    let (latest,_,tag,release)=list.iter().take(30)
        .filter_map(|r|{
            let (version,run,tag)=signed_hq_preview_release(r)?;
            Some((version,run,tag,r))
        })
        .max_by_key(|(version,run,_,_)|(*version,*run))
        .ok_or("release_unavailable")?;
    let (version,_,_)=parse_hq_preview_tag(&tag).ok_or("invalid_release")?;
    let filenames=[
        ("exe",format!("{HQ_PREVIEW_SETUP_PREFIX}{version}_x64-setup.exe")),
        ("msi",format!("{HQ_PREVIEW_SETUP_PREFIX}{version}_x64_en-US.msi")),
        ("checksums","SHA256SUMS.txt".to_string()),
    ];
    let downloads=filenames.iter().filter_map(|(kind,name)|
        hq_release_asset(release,&tag,kind,name)).collect::<Vec<_>>();
    let published_at=release.get("published_at").and_then(Value::as_str)
        .filter(|s|s.len()<=40 && s.bytes().all(|c|c.is_ascii_digit() ||
            matches!(c,b'-'|b':'|b'.'|b'T'|b'Z'|b'+')))
        .map(String::from);
    let status=if latest>installed {"update_available"}
        else if latest==installed {"up_to_date"} else {"ahead"};
    Ok(AppUpdateInfo{
        current_version:current.to_string(),latest_version:version.to_string(),
        status:status.to_string(),release_url:format!("{RELEASE_URL_BASE}{tag}"),
        published_at,downloads,prerelease:true,channel:"hq_preview".to_string(),
        changelog:release.get("body").and_then(Value::as_str).unwrap_or("")
            .chars().take(MAX_CHANGELOG_CHARS).collect(),
    })
}
fn signed_hq_preview_manifest_url(release_url:&str,version:&str)
    ->Result<reqwest::Url,String> {
    let tag=release_url.strip_prefix(RELEASE_URL_BASE).ok_or("invalid_hq_url")?;
    let (tag_version,_,_)=parse_hq_preview_tag(tag).ok_or("invalid_hq_tag")?;
    if tag_version!=version {return Err("hq_release_version_mismatch".into());}
    reqwest::Url::parse(&format!("{ASSET_URL_BASE}{tag}/latest.json"))
        .map_err(|_|"invalid_hq_manifest".into())
}

/// Explicit opt-in preview channel. The stable check above remains unchanged
/// and never trusts prereleases. Only safe numeric tags and this repository's
/// canonical release assets can appear in the preview list.
fn interpret_preview_releases(current: &str, releases: &Value) -> Result<AppUpdateInfo, String> {
    let releases = releases.as_array().ok_or("invalid_release")?;
    let release = releases.iter().take(10).filter(|release| {
        release.get("draft").and_then(Value::as_bool) == Some(false) &&
        release.get("prerelease").and_then(Value::as_bool) == Some(true)
    }).filter_map(|release| {
        let tag = release.get("tag_name")?.as_str()?;
        if !tag.starts_with('v') { return None; }
        let version = stable_version(tag)?;
        Some((version, release))
    }).max_by_key(|(version, _)| *version)
        .map(|(_, release)| release).ok_or("release_unavailable")?;
    let mut parsed = release.clone();
    parsed["prerelease"] = Value::Bool(false);
    interpret_release(current, &parsed)
}

async fn fetch_release_json(endpoint: &'static str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .user_agent("MirroLeap-Desktop-UpdateCheck")
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(12))
        .build().map_err(|_| "network_error")?;

    let mut response = client.get(endpoint)
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
    serde_json::from_slice(&payload).map_err(|_| "invalid_release".into())
}

#[tauri::command]
pub async fn check_mirroleap_update(
    window: Window,
    app_handle: AppHandle,
) -> Result<AppUpdateInfo, String> {
    if window.label() != "main" { return Err("main_window_only".into()); }
    let data = fetch_release_json(ALL_RELEASES_API).await?;
    let local = app_handle.package_info().version.to_string();
    if isolated_hq_preview() {
        interpret_hq_preview_releases(&local,&data)
    } else {
        interpret_latest_release(&local, &data)
    }
}

/// Native browser handoff, intentionally restricted to exact official release
/// assets instead of arbitrary URLs. WebView2 <a target=_blank> alone cannot
/// reliably open external URLs on Windows.
#[tauri::command]
pub async fn open_official_release_link(
    window: Window, app_handle: AppHandle, version: String, kind: String,
) -> Result<(), String> {
    if window.label() != "main" { return Err("main_window_only".into()); }
    let path=if isolated_hq_preview() {
        if kind=="list" {
            "https://github.com/slobys/MirroLeap/releases".to_string()
        } else {
            if stable_version(&version).is_none() {return Err("Invalid HQ Preview version".into());}
            // Re-query the signed-only HQ channel. No arbitrary browser URLs
            // or older release aliases are trusted from the WebView.
            let local=app_handle.package_info().version.to_string();
            let info=interpret_hq_preview_releases(
                &local,&fetch_release_json(ALL_RELEASES_API).await?
            )?;
            if info.latest_version!=version {
                return Err("HQ Preview release changed: recheck for updates".into());
            }
            let tag=info.release_url.strip_prefix(RELEASE_URL_BASE)
                .ok_or("Invalid HQ Preview release URL")?;
            if parse_hq_preview_tag(tag).is_none() {return Err("Invalid HQ Preview tag".into());}
            match &kind[..] {
                "page"=>info.release_url.clone(),
                "exe"|"msi"|"checksums"=>info.downloads.iter()
                    .find(|a|a.kind==kind).map(|a|a.url.clone())
                    .ok_or("HQ Preview asset unavailable")?,
                _=>return Err("Unsupported HQ Preview release asset".into()),
            }
        }
    } else {
        official_release_browser_url(&version, &kind)?
    };
    use tauri_plugin_shell::ShellExt;
    #[allow(deprecated)]
    app_handle.shell().open(path, None)
        .map_err(|e| format!("Could not open official release in browser: {e}"))
}

fn official_release_browser_url(version: &str, kind: &str) -> Result<String, String> {
    if version.starts_with('v') || stable_version(&version).is_none() {
        return Err("Untrusted release version".into());
    }
    let path = match kind {
        "list" => "https://github.com/slobys/MirroLeap/releases".to_string(),
        "page" => format!("{RELEASE_URL_BASE}v{version}"),
        "portable" => format!("{ASSET_URL_BASE}v{version}/MirroLeap_{version}_x64_Portable.zip"),
        "exe" => format!("{ASSET_URL_BASE}v{version}/MirroLeap_{version}_x64-setup.exe"),
        "msi" => format!("{ASSET_URL_BASE}v{version}/MirroLeap_{version}_x64_en-US.msi"),
        "checksums" => format!("{ASSET_URL_BASE}v{version}/SHA256SUMS.txt"),
        _ => return Err("Untrusted release asset type".into()),
    };
    Ok(path)
}


fn checksum_entry(manifest: &str, filename: &str) -> Option<String> {
    let mut found = None;
    for line in manifest.lines() {
        let mut words = line.split_whitespace();
        let (Some(hash), Some(name)) = (words.next(), words.next()) else { continue };
        if words.next().is_some() || name.trim_start_matches('*') != filename ||
            hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let value = hash.to_ascii_lowercase();
        if found.replace(value.clone()).is_some() { return None; }
    }
    found
}

fn release_download_kind(kind: &str) -> bool {
    matches!(kind, "exe" | "msi" | "portable" | "checksums")
}

fn download_client() -> Result<reqwest::Client, String> {
    // GitHub Release downloads redirect to GitHub-controlled asset hosts.
    // Never follow an unexpected or non-TLS redirect into an untrusted host.
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        let host = attempt.url().host_str().unwrap_or("");
        if attempt.previous().len() < 6 && attempt.url().scheme() == "https" &&
            matches!(host, "github.com" | "release-assets.githubusercontent.com" |
                "objects.githubusercontent.com") {
            attempt.follow()
        } else {
            attempt.stop()
        }
    });
    reqwest::Client::builder()
        .user_agent("MirroLeap-Official-Manual-Download")
        .redirect(policy)
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(300))
        .build().map_err(|e| format!("Cannot create safe downloader: {e}"))
}

async fn expected_release_hash(client: &reqwest::Client,
    info: &AppUpdateInfo, asset: &ReleaseDownload) -> Result<String, String> {
    if let Some(hash) = &asset.sha256 { return Ok(hash.clone()); }
    // Older GitHub assets may not expose an API digest. In that case require
    // the release's exact checksum manifest; never accept unchecked binaries.
    let checksums = info.downloads.iter().find(|entry| entry.kind == "checksums")
        .ok_or("This release has no verifiable SHA-256 checksum")?;
    let response = client.get(&checksums.url).send().await
        .map_err(|e| format!("Cannot fetch official SHA256SUMS: {e}"))?;
    if !response.status().is_success() { return Err("Official checksum download failed".into()); }
    if response.content_length().is_some_and(|n| n > 32_768) {
        return Err("Official checksum manifest is too large".into());
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await
        .map_err(|e| format!("Cannot read checksum list: {e}"))? {
        if bytes.len().saturating_add(chunk.len()) > 32_768 {
            return Err("Checksum manifest is too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "Invalid checksum manifest encoding")?;
    checksum_entry(text, &asset.name).ok_or("Release file is missing its unique SHA-256 entry".into())
}

static MANUAL_DOWNLOAD_BUSY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

struct ManualDownloadGuard;
impl ManualDownloadGuard {
    fn acquire() -> Result<Self, String> {
        if MANUAL_DOWNLOAD_BUSY.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return Err("Another official download is already running".into());
        }
        Ok(Self)
    }
}
impl Drop for ManualDownloadGuard {
    fn drop(&mut self) {
        MANUAL_DOWNLOAD_BUSY.store(false, std::sync::atomic::Ordering::Release);
    }
}

/// User-click-only download. Windows WebView2 may refuse external executable
/// downloads even when an anchor has a valid href. Save directly to Downloads,
/// compare SHA-256 with the official release metadata/manifest and NEVER run it.
#[tauri::command]
pub async fn download_official_release_asset(
    window: Window, app_handle: AppHandle, version: String, kind: String,
) -> Result<String, String> {
    if window.label() != "main" { return Err("main_window_only".into()); }
    if !release_download_kind(&kind) || stable_version(&version).is_none() ||
        version.starts_with('v') { return Err("Untrusted release download request".into()); }
    let _busy = ManualDownloadGuard::acquire()?;
    let local = app_handle.package_info().version.to_string();
    let releases=fetch_release_json(ALL_RELEASES_API).await?;
    let info=if isolated_hq_preview() {
        interpret_hq_preview_releases(&local,&releases)?
    } else {
        interpret_latest_release(&local,&releases)?
    };
    if info.status != "update_available" || info.latest_version != version {
        return Err("Official release changed; check for updates again".into());
    }
    let asset = info.downloads.iter().find(|entry| entry.kind == kind)
        .ok_or("Official release file is unavailable")?;
    let client = download_client()?;
    let expected = expected_release_hash(&client, &info, asset).await?;
    let downloads = app_handle.path().download_dir()
        .map_err(|e| format!("Cannot locate Downloads folder: {e}"))?;
    tokio::fs::create_dir_all(&downloads).await
        .map_err(|e| format!("Cannot prepare Downloads folder: {e}"))?;
    let response = client.get(&asset.url).send().await
        .map_err(|e| format!("Cannot download official asset: {e}"))?;
    if !response.status().is_success() { return Err("Official download URL failed".into()); }
    if response.content_length().is_some_and(|n| n != asset.size) {
        return Err("GitHub asset length does not match release metadata".into());
    }
    let mut response = response;
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncWriteExt;
    let mut sha = Sha256::new();
    let mut received = 0_u64;
    // Each candidate uses a unique temporary file. Never overwrite a user's
    // previous downloads or expose a partial installer as a completed EXE.
    for i in 0..100 {
        let filename = if i == 0 { asset.name.clone() } else {
            let (stem, extension) = asset.name.rsplit_once('.')
                .ok_or("Invalid official filename")?;
            format!("{stem} ({i}).{extension}")
        };
        let target = downloads.join(&filename);
        if target.exists() { continue; }
        let part = downloads.join(format!(".{filename}.mirroleap-part"));
        let mut file = match tokio::fs::OpenOptions::new().write(true)
            .create_new(true).open(&part).await {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("Cannot create download: {error}")),
            };
        let download_result = async {
            while let Some(chunk) = response.chunk().await
                .map_err(|e| format!("Official download interrupted: {e}"))? {
                received = received.saturating_add(chunk.len() as u64);
                if received > asset.size {
                    return Err("Downloaded file exceeds official release size".to_string());
                }
                sha.update(&chunk);
                file.write_all(&chunk).await.map_err(|e| format!("Download write failed: {e}"))?;
                let percent = (100 * received / asset.size).min(100);
                let _ = app_handle.emit("mirroleap-manual-download-progress",
                    serde_json::json!({"kind":kind,"percent":percent}));
            }
            if received != asset.size || format!("{:x}", sha.finalize()) != expected {
                return Err("Official release SHA-256 mismatch; file discarded".into());
            }
            file.flush().await.map_err(|e| format!("Cannot flush download: {e}"))?;
            file.sync_all().await.map_err(|e| format!("Cannot finish download: {e}"))?;
            drop(file);
            tokio::fs::rename(&part, &target).await
                .map_err(|e| format!("Cannot finalize download: {e}"))?;
            Ok::<String,String>(target.to_string_lossy().into_owned())
        }.await;
        if download_result.is_err() { let _ = tokio::fs::remove_file(&part).await; }
        return download_result;
    }
    Err("Too many files already exist in Downloads for this release".into())
}
#[tauri::command]
pub async fn install_signed_update(
    window: Window, app_handle: AppHandle, version: String,
) -> Result<(), String> {
    if window.label() != "main" { return Err("main_window_only".into()); }
    #[cfg(not(target_os = "windows"))]
    { let _ = (app_handle, version); return Err("Signed one-click updates currently require Windows NSIS".into()); }
    #[cfg(target_os = "windows")]
    {
        if crate::commands::app_install_kind()? != "installed" {
            return Err("Only a verified NSIS EXE installation can use automatic updates".into());
        }
        if !app_handle.state::<crate::ScrcpyState>().processes.lock()
            .map_err(|e|e.to_string())?.is_empty() {
            return Err("Stop phone mirroring before installing an application update".into());
        }
        let local_version = app_handle.package_info().version.to_string();
        if stable_version(&version).ok_or("invalid_update_version")? <=
            stable_version(&local_version).ok_or("invalid_local_version")? {
            return Err("Update version must be newer than the current application".into());
        }
        // Re-query GitHub immediately before installation. Never trust a
        // version supplied by WebView2 alone or a GitHub attachment URL.
        let releases=fetch_release_json(ALL_RELEASES_API).await?;
        // An HQ Preview is not allowed to update to an ordinary stable build,
        // even if the stable release version is numerically newer.
        let official=if isolated_hq_preview() {
            interpret_hq_preview_releases(&local_version,&releases)?
        } else {
            interpret_latest_release(&local_version,&releases)?
        };
        if official.latest_version != version || official.status != "update_available" {
            return Err("Official release changed: check for updates again".into());
        }
        let endpoint=if isolated_hq_preview() {
            signed_hq_preview_manifest_url(&official.release_url,&version)?
        } else {
            signed_manifest_url(&version)?
        };
        let update = app_handle.updater_builder()
            .endpoints(vec![endpoint]).map_err(|e|e.to_string())?
            .build().map_err(|e|e.to_string())?
            .check().await.map_err(|e|format!("Cannot verify official update manifest: {e}"))?
            .ok_or("No newer signed update was provided by the official release")?;
        if update.version != version {
            return Err("Signed manifest version differs from official GitHub Release".into());
        }
        let events = app_handle.clone();
        let mut downloaded = 0_u64;
        let mut last_percent = -1_i32;
        update.download_and_install(
            move |bytes, size| {
                downloaded += bytes as u64;
                let percent = size.filter(|total| *total>0)
                    .map(|total| (100 * downloaded / total).min(100) as i32);
                if let Some(p) = percent {
                    if p == last_percent { return; }
                    last_percent = p;
                }
                let _ = events.emit("mirroleap-app-update-progress",
                    serde_json::json!({"received":downloaded, "total":size, "percent":percent}));
            },
            || {},
        ).await.map_err(|e|format!("Signed update download or installation failed: {e}"))?;
        Ok(())
    }
}

#[tauri::command]
pub async fn check_mirroleap_preview_update(
    window: Window,
    app_handle: AppHandle,
) -> Result<AppUpdateInfo, String> {
    if window.label() != "main" { return Err("main_window_only".into()); }
    let local = app_handle.package_info().version.to_string();
    if isolated_hq_preview() {
        interpret_hq_preview_releases(&local,&fetch_release_json(ALL_RELEASES_API).await?)
    } else {
        interpret_preview_releases(&local,&fetch_release_json(PREVIEW_RELEASES_API).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn signed_release(version: &str, prerelease: bool, draft: bool) -> Value {
        let tag = format!("v{version}");
        let file = format!("MirroLeap_{version}_x64-setup.exe");
        let signature = format!("{file}.sig");
        let asset = |name: &str, size: u64| json!({
            "name":name,
            "browser_download_url":format!("{ASSET_URL_BASE}{tag}/{name}"),
            "size":size,
        });
        json!({
            "tag_name":tag,"draft":draft,"prerelease":prerelease,
            "published_at":"2026-10-17T08:00:00Z","body":"Official release",
            "assets":[asset(&file,4900000),asset(&signature,420),asset("latest.json",766)]
        })
    }

    #[test]
    fn unified_update_check_prefers_newest_signed_release_including_prereleases() {
        let stable = signed_release("5.0.2",false,false);
        let preview = signed_release("5.0.7",true,false);
        let mut malicious = signed_release("5.99.0",true,false);
        malicious["assets"][1]["browser_download_url"]=json!("https://evil.invalid/forged.sig");
        let draft = signed_release("6.0.0",true,true);
        let items=json!([stable,malicious,preview,draft]);
        let selected=interpret_latest_release("5.0.6",&items).unwrap();
        assert_eq!(selected.latest_version,"5.0.7");
        assert_eq!(selected.status,"update_available");
        assert!(selected.prerelease);
        assert_eq!(selected.downloads.len(),1);
        assert_eq!(selected.downloads[0].kind,"exe");
        assert_eq!(interpret_latest_release("5.0.7",&items).unwrap().status,"up_to_date");
        assert_eq!(interpret_latest_release("5.0.8",&items).unwrap().status,"ahead");
    }

    /// Regression contract against the immutable v5.2.3 installed client:
    /// it asks check_mirroleap_update for complete vX.Y.Z releases, accepts
    /// signed prereleases, and only then opens the once-per-startup prompt.
    /// A separate "hq-preview-v*" tag NEVER matches this normal channel.
    #[test]
    fn v523_can_discover_and_install_standard_signed_v528_without_rebranding() {
        let older=signed_release("5.2.3",true,false);
        let normal=signed_release("5.2.8",true,false);
        let mut hq=signed_release("5.2.9",true,false);
        hq["tag_name"]=json!("hq-preview-v5.2.9-run2");
        let releases=json!([hq,normal,older]);
        let found=interpret_latest_release("5.2.3",&releases).unwrap();
        assert_eq!(found.current_version,"5.2.3");
        assert_eq!(found.latest_version,"5.2.8");
        assert_eq!(found.status,"update_available");
        assert_eq!(found.channel,"stable");
        assert_eq!(found.release_url,
            "https://github.com/slobys/MirroLeap/releases/tag/v5.2.8");
        assert!(found.downloads.iter().any(|file|
            file.name=="MirroLeap_5.2.8_x64-setup.exe"));
        assert_eq!(signed_manifest_url("5.2.8").unwrap().as_str(),
            "https://github.com/slobys/MirroLeap/releases/download/v5.2.8/latest.json");
        assert_eq!(interpret_latest_release("5.2.8",&releases)
            .unwrap().status,"up_to_date");
    }

    #[test]
    fn signed_endpoint_is_pinned_and_rejects_untrusted_versions() {
        assert_eq!(signed_manifest_url("5.0.7").unwrap().as_str(),
            "https://github.com/slobys/MirroLeap/releases/download/v5.0.7/latest.json");
        for bad in ["", "../5.0.7","5.0.7?download=1","v5.0.7-beta","5.0.7/x","5.0.07"] {
            assert!(signed_manifest_url(bad).is_err(),"{bad}");
        }
        assert!(interpret_latest_release("5.0.2",
            &json!([signed_release("5.0.7",true,true)])).is_err());
    }

    #[test]
    fn release_browser_handoff_is_restricted_to_official_assets() {
        assert_eq!(official_release_browser_url("5.0.8","page").unwrap(),
            "https://github.com/slobys/MirroLeap/releases/tag/v5.0.8");
        assert_eq!(official_release_browser_url("5.0.8","portable").unwrap(),
            "https://github.com/slobys/MirroLeap/releases/download/v5.0.8/MirroLeap_5.0.8_x64_Portable.zip");
        assert_eq!(official_release_browser_url("5.0.8","list").unwrap(),
            "https://github.com/slobys/MirroLeap/releases");
        for bad_kind in ["", "https://evil.invalid", "../shell", "file", "script"] {
            assert!(official_release_browser_url("5.0.8",bad_kind).is_err());
        }
        for bad_version in ["", "v5.0.8", "5.0.8/../../evil", "5.0.8?download=1"] {
            assert!(official_release_browser_url(bad_version,"page").is_err());
        }
    }

    #[test]
    fn official_manual_download_must_have_exact_unique_checksum_and_allowed_kind() {
        let name="MirroLeap_5.0.8_x64-setup.exe";
        let good="b".repeat(64);
        let text=format!("{good}  {name}\n");
        assert_eq!(checksum_entry(&text,name),Some(good.clone()));
        assert_eq!(checksum_entry(&format!("{text}{good} *{name}\n"),name),None,
            "Duplicated checksum entries must fail closed");
        assert_eq!(checksum_entry(&format!("{good}  ../{name}\n"),name),None);
        assert_eq!(checksum_entry(&format!("not-a-hash  {name}\n"),name),None);
        for kind in ["exe","portable","msi","checksums"] {
            assert!(release_download_kind(kind));
        }
        for bad in ["","page","../../evil","exe.exe","https://evil.invalid"] {
            assert!(!release_download_kind(bad));
        }
    }

    fn hq_fixture(version:&str,run:u64)->Value {
        let tag=format!("hq-preview-v{version}-run{run}");
        let exe=format!("MirroLeap.HQ.Preview_{version}_x64-setup.exe");
        let msi=format!("MirroLeap.HQ.Preview_{version}_x64_en-US.msi");
        let asset=|name:&str,size:u64|json!({
            "name":name,
            "browser_download_url":format!("{ASSET_URL_BASE}{tag}/{name}"),
            "size":size,
        });
        json!({
            "tag_name":tag,"draft":false,"prerelease":true,
            "published_at":"2026-10-21T08:00:00Z","body":"HQ Preview only",
            "assets":[
                asset(&exe,20_000_000),asset(&format!("{exe}.sig"),400),
                asset(&msi,24_000_000),asset("latest.json",900),
                asset("SHA256SUMS.txt",225)
            ]
        })
    }

    #[test]
    fn hq_preview_tags_must_be_canonical_and_never_refer_to_stable_releases() {
        assert!(parse_hq_preview_tag("hq-preview-v5.2.6-run9").is_some());
        for bad in [
            "", "v5.2.6","hq-preview-v5.2.6","hq-preview-v5.2.6-run0",
            "hq-preview-v5.2.6-run01","hq-preview-v5.2.06-run9",
            "hq-preview-v5.2.6-run9/../../stable",
            "hq-preview-v5.2.6-run9?download=1",
            "hq-preview-v5.2.6-run-1"
        ] { assert!(parse_hq_preview_tag(bad).is_none(),"{bad}"); }
        assert!(signed_hq_preview_manifest_url(
            "https://github.com/slobys/MirroLeap/releases/tag/v5.2.6","5.2.6").is_err());
        assert_eq!(signed_hq_preview_manifest_url(
            "https://github.com/slobys/MirroLeap/releases/tag/hq-preview-v5.2.6-run9",
            "5.2.6").unwrap().as_str(),
            "https://github.com/slobys/MirroLeap/releases/download/hq-preview-v5.2.6-run9/latest.json");
        assert!(signed_hq_preview_manifest_url(
            "https://github.com/slobys/MirroLeap/releases/tag/hq-preview-v5.2.6-run9",
            "5.2.7").is_err());
    }

    #[test]
    fn only_newest_completely_signed_hq_release_is_eligible_for_updates() {
        let good=hq_fixture("5.2.6",9);
        let next=hq_fixture("5.2.6",10);
        let mut forged=hq_fixture("5.2.8",11);
        forged["assets"][1]["browser_download_url"]=
            json!("https://attacker.invalid/forged.sig");
        let mut unsigned=hq_fixture("5.2.9",12);
        unsigned["assets"][3]["size"]=json!(0);
        let mut draft=hq_fixture("5.3.0",13);
        draft["draft"]=json!(true);
        let stable=signed_release("9.0.0",false,false);
        let info=interpret_hq_preview_releases("5.2.5",
            &json!([forged,stable,draft,good,next,unsigned])).unwrap();
        assert_eq!(info.latest_version,"5.2.6");
        assert_eq!(info.channel,"hq_preview");
        assert_eq!(info.status,"update_available");
        assert_eq!(info.release_url,
            "https://github.com/slobys/MirroLeap/releases/tag/hq-preview-v5.2.6-run10");
        assert!(info.downloads.iter().any(|d|d.kind=="exe" && d.name.starts_with("MirroLeap.HQ.")));
        assert!(info.downloads.iter().all(|d|!d.url.contains("/download/v")));
        assert_eq!(interpret_hq_preview_releases("5.2.6",&json!([next]))
            .unwrap().status,"up_to_date");
        let mut old_unsigned=hq_fixture("5.2.5",5);
        old_unsigned["assets"][1]["size"]=json!(0);
        assert!(interpret_hq_preview_releases("5.2.4",
            &json!([old_unsigned])).is_err(),
            "Unsigned earlier previews must never be offered as signed updates");
    }

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
    fn preview_channel_selects_only_canonical_non_draft_prereleases() {
        let mut preview = sample_release();
        preview["tag_name"] = json!("v5.0.4");
        preview["prerelease"] = json!(true);
        let mut invalid = preview.clone();
        invalid["tag_name"] = json!("v5.99.0/../../evil");
        let mut draft = preview.clone();
        draft["draft"] = json!(true);
        draft["tag_name"] = json!("v6.0.0");
        let mut old = preview.clone();
        old["tag_name"] = json!("v5.0.3");
        let selected = interpret_preview_releases("5.0.4", &json!([
            invalid, draft, old, preview
        ])).unwrap();
        assert_eq!(selected.latest_version, "5.0.4");
        assert_eq!(selected.status, "up_to_date");
        assert_eq!(selected.release_url,
            "https://github.com/slobys/MirroLeap/releases/tag/v5.0.4");
        assert_eq!(interpret_preview_releases("5.0.2", &json!([
            {"tag_name":"v5.0.5", "prerelease":true, "draft":false}
        ])).unwrap().status, "update_available");
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
