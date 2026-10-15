//! Transactional updater for the *scrcpy engine*, independent of the GUI installer.
//! Never remove a working engine until the new archive is fully downloaded,
//! extracted and verified. Keep the previous engine as a rollback copy.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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

async fn release_asset(client: &reqwest::Client, arch: &str, suffix: &str) -> Result<String, String> {
    let api = "https://api.github.com/repos/Genymobile/scrcpy/releases/latest";
    if let Ok(response) = client.get(api).send().await {
        if response.status().is_success() {
            if let Ok(value) = response.json::<serde_json::Value>().await {
                if let Some(assets) = value["assets"].as_array() {
                    let prefix = format!("scrcpy-{arch}-v");
                    for asset in assets {
                        let name = asset["name"].as_str().unwrap_or("");
                        if name.starts_with(&prefix) && name.ends_with(suffix) {
                            if let Some(url) = asset["browser_download_url"].as_str() {
                                if url.starts_with("https://github.com/Genymobile/scrcpy/releases/download/") {
                                    return Ok(url.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // GitHub API may be rate limited. The redirect still gives the release tag.
    let response = client
        .get("https://github.com/Genymobile/scrcpy/releases/latest")
        .send()
        .await
        .map_err(|e| format!("Cannot discover scrcpy release: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Cannot discover scrcpy release: {e}"))?;
    let tag = response.url().path_segments()
        .and_then(|mut s| s.next_back())
        .ok_or("No scrcpy release tag found")?;
    if !tag.starts_with('v') || !tag[1..].chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err("Unexpected scrcpy release tag".into());
    }
    let file = format!("scrcpy-{arch}-{tag}{suffix}");
    Ok(format!("https://github.com/Genymobile/scrcpy/releases/download/{tag}/{file}"))
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
    let url = release_asset(&client, arch, suffix).await?;
    let mut response = client.get(&url).send().await
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
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        downloaded += chunk.len() as u64;
        if downloaded > MAX_ARCHIVE_SIZE {
            return Err("scrcpy archive exceeds the download size limit".into());
        }
        file.write_all(&chunk).map_err(|e| format!("Cannot write update: {e}"))?;
        if total > 0 {
            let pct = (downloaded.saturating_mul(100) / total).min(100);
            let _ = window.emit("download-progress", serde_json::json!({"percent": pct}));
        }
    }
    file.sync_all().map_err(|e| format!("Cannot finish update download: {e}"))?;
    drop(file);
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
