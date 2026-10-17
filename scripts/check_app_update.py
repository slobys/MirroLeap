#!/usr/bin/env python3
"""Safety gate: single official Release lookup and pinned-key NSIS updater.

No automatic install on check, arbitrary URLs, or portable/MSI replacement.
"""
from pathlib import Path
import json
import re

root = Path(__file__).resolve().parents[1]
backend = (root / "src-tauri/src/app_update.rs").read_text(encoding="utf-8")
frontend = (root / "src/components/AppUpdateChecker.tsx").read_text(encoding="utf-8")
validator = (root / "src/utils/appUpdate.ts").read_text(encoding="utf-8")
app = (root / "src/App.tsx").read_text(encoding="utf-8")
lib = (root / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
caps = json.loads((root / "src-tauri/capabilities/default.json").read_text(encoding="utf-8"))
cfg = json.loads((root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
pkg = json.loads((root / "package.json").read_text(encoding="utf-8"))
crate = (root / "src-tauri/Cargo.toml").read_text(encoding="utf-8")

assert caps["windows"] == ["main"], "Only the main window may check external releases"
assert cfg["identifier"] == "com.kb.scrcpy-gui-v3", "Preserve existing Windows app data"
assert pkg["version"] == cfg["version"]
assert re.search(r'(?m)^version = "' + re.escape(pkg["version"]) + r'"', crate)
assert 'app_update::check_mirroleap_update' in lib
assert 'app_update::check_mirroleap_preview_update' in lib
assert 'window.label() != "main"' in backend
assert 'https://api.github.com/repos/slobys/MirroLeap/releases?per_page=20' in backend
assert 'https://api.github.com/repos/slobys/MirroLeap/releases?per_page=10' in backend
assert 'reqwest::redirect::Policy::none()' in backend
assert 'Duration::from_secs(12)' in backend
assert 'MAX_METADATA_BYTES: usize = 512 * 1024' in backend
assert '"update_available"' in backend and '"up_to_date"' in backend and '"ahead"' in backend
assert 'interpret_latest_release(current: &str' in backend and 'has_official_updater(' in backend
assert 'asset.get("browser_download_url").and_then(Value::as_str) == Some(expected_url.as_str())' in backend
assert 'MirroLeap_{version}_x64_Portable.zip' in backend
assert 'SHA256SUMS.txt' in backend
assert 'AppUpdateChecker version={appVersion}' in app
assert "await invoke<AppUpdateInfo>('check_mirroleap_update')" in frontend
assert "check('preview')" not in frontend and "channel === 'preview'" not in frontend
assert "if (result.status === 'update_available')" in frontend
assert "installKind === 'installed'" in frontend
assert 'trustedReleaseDownload(info, asset)' in frontend
assert "invoke<void>('install_signed_update', {version:info.latestVersion})" in frontend
assert 'pub async fn install_signed_update(' in backend
assert 'updater_builder()' in backend and '.endpoints(vec![endpoint])' in backend
assert 'update.download_and_install(' in backend
assert 'signed_manifest_url(' in backend
assert 'app_install_kind()' in backend and 'Stop phone mirroring' in backend
assert 'if update.version != version' in backend, "Signed version mismatch must fail closed"

assert 'trustedReleaseUrl(info)' in frontend and 'Object.prototype.hasOwnProperty.call' in validator
assert "dangerouslySetInnerHTML" not in frontend
assert "setInterval(" not in frontend and "useEffect(() => {" in frontend
assert "startupUpdateRequested.current" in app
assert "setStartupRelease(latest)" in app
assert "StartupUpdateNotice" in app
assert "open_official_release_link" in backend
assert "official_release_browser_url(" in backend
assert 'kind.as_str()' not in backend
assert 'app_handle.shell().open(path, None)' in backend
assert 'open_official_community_link' in (root / 'src-tauri/src/commands.rs').read_text(encoding='utf-8')
for prohibited in ("std::process::Command", "tokio::process::Command",
                   "std::fs::write", "tokio::fs::write",
                   "auto_install", "run_installer"):
    assert prohibited not in backend, f"Update checker must not execute arbitrary programs: {prohibited}"
assert (root / "src-tauri/src/core_manager.rs").exists(), "scrcpy update remains independent"
print("Unified release discovery and strictly scoped signed-updater installer security checks passed")
