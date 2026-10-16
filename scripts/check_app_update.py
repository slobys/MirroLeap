#!/usr/bin/env python3
"""Cross-layer safety gate for the read-only MirroLeap app updater.

This is not a digital signature validator or a Windows user acceptance test.
It enforces the intended absence of silent installer actions and arbitrary
release URLs until a separately signed Tauri updater is designed.
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
assert 'window.label() != "main"' in backend
assert 'https://api.github.com/repos/slobys/MirroLeap/releases/latest' in backend
assert 'reqwest::redirect::Policy::none()' in backend
assert 'Duration::from_secs(12)' in backend
assert 'MAX_METADATA_BYTES: usize = 512 * 1024' in backend
assert '"update_available"' in backend and '"up_to_date"' in backend and '"ahead"' in backend
assert 'release.get("prerelease").and_then(Value::as_bool) != Some(false)' in backend
assert 'asset.get("browser_download_url").and_then(Value::as_str) == Some(expected_url.as_str())' in backend
assert 'MirroLeap_{version}_x64_Portable.zip' in backend
assert 'SHA256SUMS.txt' in backend
assert 'AppUpdateChecker version={appVersion}' in app
assert "await invoke<AppUpdateInfo>('check_mirroleap_update')" in frontend
assert 'trustedReleaseDownload(info, asset)' in frontend
assert 'trustedReleaseUrl(info)' in frontend and 'Object.prototype.hasOwnProperty.call' in validator
assert "dangerouslySetInnerHTML" not in frontend
assert "setInterval(" not in frontend and "useEffect(() => {" in frontend
for prohibited in ("std::process::Command", "tokio::process::Command",
                   "std::fs::write", "tokio::fs::write",
                   "tauri_plugin_shell", "auto_install", "run_installer"):
    assert prohibited not in backend, f"Update checker must be read-only: {prohibited}"
assert (root / "src-tauri/src/core_manager.rs").exists(), "scrcpy update remains independent"
print("Read-only update checker host, release validation, user-click IPC and no-install contract passed")
