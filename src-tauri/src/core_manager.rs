//! Transactional updater for the *scrcpy engine*, independent of the GUI installer.
//! Never remove a working engine until the new archive is fully downloaded,
//! extracted and verified. Keep the previous engine as a rollback copy.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use sha2::{Digest, Sha256};
use tauri::{Emitter, Window};

static UPDATING: AtomicBool = AtomicBool::new(false);
const MAX_ARCHIVE_SIZE: u64 = 250 * 1024 * 1024;

struct UpdateLock;
impl UpdateLock {
    fn acquire() -> Result<Self, String> {
        if UPDATING.swap(true, Ordering::SeqCst) {
            return Err("Another scrcpy engine update is already running.".into());
        }
        Ok(Self)
    }
}
impl Drop for UpdateLock {
    fn drop(&mut self) {
        UPDATING.store(false, Ordering::SeqCst);
    }
}

struct ScratchDir(PathBuf);
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn unique_suffix() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}

fn target_folder() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Cannot find application directory: {e}"))?;
    let parent = exe.parent().ok_or("Cannot find application directory")?;
    Ok(parent.join("scrcpy-bin"))
}

fn supported_asset() -> Result<(&'static str, &'static str), String> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok(("win64", ".zip"))
    } else if cfg!(all(target_os = "windows", target_arch = "x86")) {
        Ok(("win32", ".zip"))
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok(("linux-x86_64", ".tar.gz"))
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok(("macos-aarch64", ".tar.gz"))
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Ok(("macos-x86_64", ".tar.gz"))
    } else {
        Err("Automatic scrcpy update is not supported on this platform.".into())
    }
}

#[derive(Debug)]
struct VerifiedReleaseAsset {
    url: String,
    sha256_hex: String,
    size: u64,
}

/// Require GitHub's asset digest, not just a downloaded archive with a valid
/// filename. This verifies byte-for-byte consistency with the HTTPS release
/// metadata BEFORE unpacking and before touching any working engine.
fn find_verified_asset(
    release: &serde_json::Value, arch: &str, suffix: &str,
) -> Result<VerifiedReleaseAsset, String> {
    let prefix = format!("scrcpy-{arch}-v");
    let assets = release["assets"].as_array().ok_or("Missing official release assets")?;
    for asset in assets {
        let name = asset["name"].as_str().unwrap_or_default();
        if !name.starts_with(&prefix) || !name.ends_with(suffix) {
            continue;
        }
        let url = asset["browser_download_url"].as_str()
            .ok_or("Official scrcpy release has no download URL")?;
        if !url.starts_with("https://github.com/Genymobile/scrcpy/releases/download/") {
            return Err("Unexpected scrcpy download host".into());
        }
        let size = asset["size"].as_u64()
            .filter(|size| *size > 0 && *size <= MAX_ARCHIVE_SIZE)
            .ok_or("Invalid official scrcpy archive size")?;
        let digest = asset["digest"].as_str().unwrap_or_default();
        let digest = digest.strip_prefix("sha256:")
            .filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("Official scrcpy release lacks a valid SHA-256 digest; update aborted")?;
        return Ok(VerifiedReleaseAsset {
            url: url.to_string(),
            sha256_hex: digest.to_ascii_lowercase(),
            size,
        });
    }
    Err("No official scrcpy package found for this platform".into())
}

async fn release_asset(
    client: &reqwest::Client, arch: &str, suffix: &str,
) -> Result<VerifiedReleaseAsset, String> {
    let release: serde_json::Value = client
        .get("https://api.github.com/repos/Genymobile/scrcpy/releases/latest")
        .send().await
        .map_err(|e| format!("Cannot read official scrcpy release metadata: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Cannot verify official release digest (retry later): {e}"))?
        .json().await
        .map_err(|e| format!("Invalid official release response: {e}"))?;
    // A rate-limited/malformed API response must fail closed. The old HTML
    // redirect fallback could not supply a signed/verified digest.
    find_verified_asset(&release, arch, suffix)
}

fn expected_file(dir: &Path, name: &str) -> Result<(), String> {
    let path = dir.join(name);
    let metadata = fs::symlink_metadata(&path).map_err(|_| format!("Update package is missing {name}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("Invalid {name} in update package"));
    }
    Ok(())
}

fn verify_engine(dir: &Path) -> Result<(), String> {
    let scrcpy = if cfg!(target_os = "windows") { "scrcpy.exe" } else { "scrcpy" };
    let adb = if cfg!(target_os = "windows") { "adb.exe" } else { "adb" };
    for name in [scrcpy, adb, "scrcpy-server"] {
        expected_file(dir, name)?;
    }
    if cfg!(target_os = "windows") {
        for name in ["AdbWinApi.dll", "AdbWinUsbApi.dll"] {
            expected_file(dir, name)?;
        }
    }

    // A package with the right filenames but a broken binary must not replace
    // a working installation.
    let output = std::process::Command::new(dir.join(scrcpy))
        .arg("--version")
        .output()
        .map_err(|e| format!("Downloaded scrcpy could not start: {e}"))?;
    if !output.status.success() {
        return Err("Downloaded scrcpy failed its --version check".into());
    }
    Ok(())
}

fn unpack_verified(archive_path: &Path, working_dir: &Path, suffix: &str) -> Result<PathBuf, String> {
    let extract_to = working_dir.join("extracted");
    fs::create_dir(&extract_to).map_err(|e| format!("Cannot prepare extraction: {e}"))?;
    let input = fs::File::open(archive_path).map_err(|e| e.to_string())?;
    if suffix == ".zip" {
        let mut archive = zip::ZipArchive::new(input).map_err(|e| format!("Invalid ZIP: {e}"))?;
        // ZipArchive::extract sanitizes enclosed names; reject any unsafe entry
        // before extraction as an additional check.
        for i in 0..archive.len() {
            if archive.by_index(i).map_err(|e| e.to_string())?.enclosed_name().is_none() {
                return Err("Unsafe path inside scrcpy ZIP archive".into());
            }
        }
        archive.extract(&extract_to).map_err(|e| format!("Cannot extract ZIP: {e}"))?;
    } else {
        let decoder = flate2::read::GzDecoder::new(input);
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(&extract_to).map_err(|e| format!("Cannot extract tar.gz: {e}"))?;
    }

    let mut candidate = extract_to.clone();
    if !candidate.join(if cfg!(target_os = "windows") { "scrcpy.exe" } else { "scrcpy" }).is_file() {
        let children: Vec<_> = fs::read_dir(&extract_to)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        if children.len() != 1 || !children[0].file_type().map_err(|e| e.to_string())?.is_dir() {
            return Err("Unexpected scrcpy archive layout".into());
        }
        candidate = children[0].path();
    }
    verify_engine(&candidate)?;
    Ok(candidate)
}

/// Rename is atomic on the same filesystem. On any failure while installing,
/// try to move the old directory back. Return the backup path for easy rollback.
fn replace_with_backup(current: &Path, staged: &Path, backup: &Path) -> Result<Option<PathBuf>, String> {
    let had_previous = current.exists();
    if backup.exists() {
        return Err("Backup destination already exists; refusing to overwrite it".into());
    }
    if had_previous {
        fs::rename(current, backup)
            .map_err(|e| format!("Could not move previous engine (close scrcpy windows first): {e}"))?;
    }
    if let Err(e) = fs::rename(staged, current) {
        if had_previous {
            if let Err(rollback) = fs::rename(backup, current) {
                return Err(format!("Update failed: {e}; automatic rollback failed: {rollback}. Previous engine remains at {}", backup.display()));
            }
        }
        return Err(format!("Could not activate new engine: {e}. Previous engine restored."));
    }
    Ok(had_previous.then(|| backup.to_path_buf()))
}

async fn do_update(window: &Window) -> Result<PathBuf, String> {
    let (arch, suffix) = supported_asset()?;
    let destination = target_folder()?;
    let parent = destination.parent().ok_or("Invalid scrcpy installation folder")?;
    let temp = parent.join(format!(".scrcpy-staging-{}", unique_suffix()));
    fs::create_dir(&temp).map_err(|e| format!("Cannot write to application folder: {e}"))?;
    let _cleanup = ScratchDir(temp.clone());

    let client = reqwest::Client::builder()
        .user_agent("MirroLeap-EngineUpdater/4.2")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let _ = window.emit("scrcpy-status", serde_json::json!({"type": "downloading", "message": "Finding official scrcpy release"}));
    let asset = release_asset(&client, arch, suffix).await?;
    let mut response = client.get(&asset.url).send().await
        .map_err(|e| format!("Download failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Download failed: {e}"))?;
    let total = response.content_length().unwrap_or(0);
    if total > MAX_ARCHIVE_SIZE {
        return Err("scrcpy archive exceeds the download size limit".into());
    }
    let archive_path = temp.join(if suffix == ".zip" { "scrcpy.zip" } else { "scrcpy.tar.gz" });
    let mut file = fs::File::create(&archive_path).map_err(|e| format!("Cannot save update: {e}"))?;
    let mut downloaded: u64 = 0;
    let mut digest = Sha256::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > MAX_ARCHIVE_SIZE {
            return Err("scrcpy archive exceeds the download size limit".into());
        }
        digest.update(&chunk);
        file.write_all(&chunk).map_err(|e| format!("Cannot write update: {e}"))?;
        if total > 0 {
            let pct = (downloaded.saturating_mul(100) / total).min(100);
            let _ = window.emit("download-progress", serde_json::json!({"percent": pct}));
        }
    }
    file.sync_all().map_err(|e| format!("Cannot finish update download: {e}"))?;
    drop(file);
    if downloaded != asset.size {
        return Err("scrcpy download size differs from the official release; previous engine preserved".into());
    }
    let actual_hash = format!("{:x}", digest.finalize());
    if actual_hash != asset.sha256_hex {
        return Err("scrcpy SHA-256 integrity check failed; previous engine preserved".into());
    }
    let _ = window.emit("scrcpy-log",
        "[UPDATE] Official scrcpy SHA-256 and release size verified before extraction.");
    let _ = window.emit("scrcpy-status", serde_json::json!({"type": "downloading", "message": "Verifying scrcpy package"}));
    let archive_for_worker = archive_path.clone();
    let temp_for_worker = temp.clone();
    let suffix_string = suffix.to_string();
    let staged = tokio::task::spawn_blocking(move || unpack_verified(&archive_for_worker, &temp_for_worker, &suffix_string))
        .await.map_err(|e| format!("Extraction task failed: {e}"))??;

    let backup = parent.join(format!("scrcpy-bin-backup-{}", unique_suffix()));
    let backup_path = replace_with_backup(&destination, &staged, &backup)?;
    if let Some(path) = backup_path {
        let _ = window.emit("scrcpy-log", format!("[UPDATE] Previous scrcpy saved at {}", path.display()));
    }
    Ok(destination)
}

pub async fn install_latest(window: &Window) -> Result<(), String> {
    let _lock = UpdateLock::acquire()?;
    let result = do_update(window).await;
    match result {
        Ok(path) => {
            let _ = window.emit("scrcpy-log", "[UPDATE] New scrcpy verified and installed successfully.");
            let _ = window.emit("scrcpy-status", serde_json::json!({
                "type": "download-complete", "success": true, "message": path.to_string_lossy()
            }));
            Ok(())
        }
        Err(message) => {
            let _ = window.emit("scrcpy-log", format!("[UPDATE ERROR] {message}"));
            let _ = window.emit("scrcpy-status", serde_json::json!({
                "type": "download-error", "success": false, "message": message
            }));
            Err(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn official_scrcpy_asset_must_have_valid_sha256_size_and_host() {
        let sha = "ab".repeat(32);
        let release = serde_json::json!({"assets": [{
            "name": "scrcpy-win64-v5.0.1.zip",
            "browser_download_url": "https://github.com/Genymobile/scrcpy/releases/download/v5.0.1/scrcpy-win64-v5.0.1.zip",
            "digest": format!("sha256:{sha}"),
            "size": 11264634
        }]});
        let verified = find_verified_asset(&release, "win64", ".zip").unwrap();
        assert_eq!(verified.sha256_hex, sha);
        assert_eq!(verified.size, 11264634);
        assert!(verified.url.starts_with("https://github.com/Genymobile/"));

        let mut broken = release.clone();
        broken["assets"][0]["digest"] = serde_json::Value::Null;
        assert!(find_verified_asset(&broken, "win64", ".zip").is_err());
        broken["assets"][0]["digest"] = serde_json::json!("sha256:invalid");
        assert!(find_verified_asset(&broken, "win64", ".zip").is_err());
        broken = release.clone();
        broken["assets"][0]["size"] = serde_json::json!(MAX_ARCHIVE_SIZE + 1);
        assert!(find_verified_asset(&broken, "win64", ".zip").is_err());
        broken = release.clone();
        broken["assets"][0]["browser_download_url"] = serde_json::json!("https://example.com/file.zip");
        assert!(find_verified_asset(&broken, "win64", ".zip").is_err());
        assert!(find_verified_asset(&release, "macos-aarch64", ".tar.gz").is_err());
    }

    #[test]
    fn streaming_sha256_matches_official_digest() {
        let mut hasher = Sha256::new();
        hasher.update(b"a");
        hasher.update(b"bc");
        assert_eq!(format!("{:x}", hasher.finalize()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!("scrcpy-updater-test-{}", unique_suffix()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn original_engine_survives_failed_install() {
        let root = scratch();
        let current = root.join("scrcpy-bin");
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join("old.txt"), "working").unwrap();
        let result = replace_with_backup(&current, &root.join("missing"), &root.join("backup"));
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(current.join("old.txt")).unwrap(), "working");
        assert!(!root.join("backup").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn successful_swap_retains_rollback_copy() {
        let root = scratch();
        let current = root.join("scrcpy-bin");
        let staged = root.join("staged");
        let backup = root.join("backup");
        fs::create_dir_all(&current).unwrap();
        fs::create_dir_all(&staged).unwrap();
        fs::write(current.join("old.txt"), "previous").unwrap();
        fs::write(staged.join("new.txt"), "updated").unwrap();
        let result = replace_with_backup(&current, &staged, &backup).unwrap();
        assert_eq!(result, Some(backup.clone()));
        assert!(current.join("new.txt").exists());
        assert!(backup.join("old.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
