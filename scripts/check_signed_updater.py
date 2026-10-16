#!/usr/bin/env python3
"""Static signed Windows updater + floating phone-control release guard.

The signature is verified by Tauri at runtime; this checks the wiring and
ensures no main-window or arbitrary keyboard access has leaked to overlays.
"""
import base64
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1]
text = lambda file: (root / file).read_text(encoding="utf-8")
tauri = json.loads(text("src-tauri/tauri.conf.json"))
cap = json.loads(text("src-tauri/capabilities/default.json"))
updater = tauri["plugins"]["updater"]
assert tauri["identifier"] == "com.kb.scrcpy-gui-v3"
assert tauri["bundle"]["createUpdaterArtifacts"] is True
assert updater["endpoints"] == [
    "https://github.com/slobys/MirroLeap/releases/latest/download/latest.json"
]
assert updater["windows"]["installMode"] == "passive"
assert not updater.get("dangerousInsecureTransportProtocol")
public = base64.b64decode(updater["pubkey"], validate=True).decode("utf-8")
assert public.startswith("untrusted comment: minisign public key:")
assert cap["windows"] == ["main"] and "updater:allow-check" in cap["permissions"]
assert "updater:allow-download-and-install" in cap["permissions"]

crate = text("src-tauri/Cargo.toml")
assert "tauri-plugin-updater" in crate
assert 'default-features = false, features = ["native-tls"]' in crate, (
    "The direct signed EXE update must not depend on obsolete ZIP 4.x extraction"
)
lib = text("src-tauri/src/lib.rs")
assert "tauri_plugin_updater::Builder::new().build()" in lib
assert "app_update::can_install_signed_update" in lib

backend = text("src-tauri/src/app_update.rs")
assert 'if window.label() != "main"' in backend
assert "Run-MirroLeap.cmd" in backend
assert "LOCALAPPDATA" in backend and "ProgramFiles" in backend

frontend = text("src/components/AppUpdateChecker.tsx")
assert "check as checkSignedUpdate" in frontend
assert "await checkSignedUpdate()" in frontend
assert "await update.downloadAndInstall(" in frontend
assert "info.latestVersion" in frontend and "update.version.replace" in frontend
assert "onClick={() => void install()}" in frontend
assert "setInterval(" not in frontend
assert "can_install_signed_update" in frontend
assert "dangerouslySetInnerHTML" not in frontend

toolbar = text("public/phone-controls.html")
native = text("src-tauri/src/device_frame.rs")
assert '<div id="shortcut-tools"' in toolbar
assert 'id="shortcut-toggle"' in toolbar
assert '<ShortcutsPanel' not in text("src/App.tsx")
assert "device_for_controls(app, caller)?" in native
assert "send_scrcpy_shortcut(hwnd, key)" in native
assert "GetForegroundWindow() } != video" in native
assert "SHORTCUT_PANELS" in native and 'action == "shortcuts_toggle"' in native
for action in ["shortcut_fullscreen", "shortcut_home", "shortcut_back",
               "shortcut_recents", "shortcut_power", "shortcut_rotate",
               "shortcut_paste", "shortcut_screen_off"]:
    assert action in native and action in toolbar
assert "SendInput(&keys" in native
assert "ink_toggle" in toolbar

ci = text(".github/workflows/desktop-ci.yml")
signing = text("scripts/windows-sign-and-checksum.ps1")
assert "TAURI_SIGNING_PRIVATE_KEY" in ci
assert "TAURI_SIGNING_PRIVATE_KEY_PASSWORD" in ci
assert "npm run tauri signer sign" in signing
assert signing.index("signtool") < signing.index("npm run tauri signer sign")
assert "windows-updater-manifest.ps1" in ci
assert "check_signed_updater_release.py" in ci
assert "windows-phone-toolbar-layout.mjs" in ci
assert "release-checksums/latest.json" in ci
assert 'release-assets' in ci and 'verified-assets/latest.json' in ci
assert "'nsis/MirroLeap_${version}" not in ci, (
    "PowerShell single quotes prevent expanding installer version placeholders"
)
assert not list((root / "src-tauri").glob("*.key")), "Never ship a private signer key"
print("Signed in-app updater and live, device-scoped floating phone shortcuts guards passed")
