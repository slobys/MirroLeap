#!/usr/bin/env python3
"""Cross-layer guard for explicit, signed MirroLeap Windows updates.

The native GitHub metadata request remains read-only; a separate Tauri
Ed25519 verifier downloads/installs ONLY on an explicit user click. This is
not an alternative digital signature verifier or real Windows acceptance.
"""
from pathlib import Path
import base64
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
assert "tauri_plugin_updater::Builder::new().build()" in lib
assert 'app_update::can_install_signed_update' in lib
assert 'pub fn can_install_signed_update(window: Window)' in backend
assert 'if window.label() != "main"' in backend
assert 'Run-MirroLeap.cmd' in backend and 'ProgramFiles' in backend and 'LOCALAPPDATA' in backend
assert caps["windows"] == ["main"] and "updater:allow-check" in caps["permissions"]
assert "updater:allow-download-and-install" in caps["permissions"]
assert "updater:allow-download" not in caps["permissions"], "Install and check scopes only"
updater = cfg["plugins"]["updater"]
assert cfg["bundle"]["createUpdaterArtifacts"] is True
assert updater["endpoints"] == [
    "https://github.com/slobys/MirroLeap/releases/latest/download/latest.json"
]
assert updater["windows"]["installMode"] == "passive"
assert updater.get("dangerousInsecureTransportProtocol") is not True
pub = base64.b64decode(updater["pubkey"], validate=True).decode("utf-8")
assert pub.startswith("untrusted comment: minisign public key:"), "Expected embedded Ed25519 public key"
assert "check as checkSignedUpdate" in frontend
assert "await checkSignedUpdate()" in frontend and "await update.downloadAndInstall(" in frontend
assert "update.version.replace(/^v/, '') !== info.latestVersion" in frontend
assert "if (installLock.current || lock.current" in frontend, "Disallow repeated install clicks"
assert "can_install_signed_update" in frontend, "Portable safeguard must be native"
assert "onClick={() => void install()}" in frontend, "No automatic install effect"
signing_script = (root / "scripts/windows-sign-and-checksum.ps1").read_text("utf-8")
ci = (root / ".github/workflows/desktop-ci.yml").read_text("utf-8")
assert "TAURI_SIGNING_PRIVATE_KEY_PASSWORD" in ci
assert "TAURI_SIGNING_PRIVATE_KEY" in ci
assert "npm run tauri signer sign" in signing_script
assert signing_script.index("signtool") < signing_script.index("npm run tauri signer sign")
assert "scripts/windows-updater-manifest.ps1" in ci
assert "scripts/check_signed_updater_release.py" in ci
assert "sha256sum --check SHA256SUMS.txt" in ci
for prohibited in ("std::process::Command", "tokio::process::Command",
                   "std::fs::write", "tokio::fs::write",
                   "tauri_plugin_shell", "auto_install", "run_installer"):
    assert prohibited not in backend, f"Update checker must be read-only: {prohibited}"
assert (root / "src-tauri/src/core_manager.rs").exists(), "scrcpy update remains independent"
print("Signed updater: main-only permissions, Ed25519 trust root, manual install, portable guard and release signing verified")
