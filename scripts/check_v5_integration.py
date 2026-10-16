#!/usr/bin/env python3
"""MirroLeap v5.0 unified-release guard.

The five feature PRs were developed from the same main. Merge conflicts in
App.tsx and version manifests can silently discard a feature while standalone
CI for each PR still passes. Check that all five have actual UI entry points,
backend IPC, supported permissions and the one canonical release version.
"""
from pathlib import Path
import json
import re

root = Path(__file__).resolve().parents[1]
read = lambda path: (root / path).read_text(encoding="utf-8")
package = json.loads(read("package.json"))
lock = json.loads(read("package-lock.json"))
tauri = json.loads(read("src-tauri/tauri.conf.json"))
capabilities = json.loads(read("src-tauri/capabilities/default.json"))
crate = read("src-tauri/Cargo.toml")
cargo_lock = read("src-tauri/Cargo.lock")
app = read("src/App.tsx")
native = read("src-tauri/src/lib.rs")
hooks = read("src/hooks/useScrcpy.ts")
ci = read(".github/workflows/desktop-ci.yml")

version = package["version"]
assert re.fullmatch(r"5\.\d+\.\d+", version), "Expected a v5 release"
assert version == lock["version"] == lock["packages"][""]["version"]
assert tauri["version"] == package["version"]
assert tauri["identifier"] == "com.kb.scrcpy-gui-v3", "Existing Windows user settings must survive upgrades"
assert re.search(rf'(?m)^version = "{re.escape(version)}"$', crate)
assert re.search(rf'(?m)^name = "mirroleap"\nversion = "{re.escape(version)}"$', cargo_lock)
assert "tauri-plugin-clipboard-manager" in cargo_lock
assert "tauri-plugin-clipboard-manager" in crate
assert "@tauri-apps/plugin-clipboard-manager" in package["dependencies"]
assert capabilities["windows"] == ["main"]
assert "clipboard-manager:allow-write-image" in capabilities["permissions"]
assert not any("clipboard" in permission and "read" in permission for permission in capabilities["permissions"])

ui = {
    "src/components/ScreenshotTools.tsx": "<ScreenshotTools",
    "src/components/AppUpdateChecker.tsx": "<AppUpdateChecker",
    "src/components/DecoderPerformance.tsx": "<DecoderPerformance",
    "src/components/MirroPresentPanel.tsx": "<MirroPresentPanel",
}
for file, element in ui.items():
    assert (root / file).exists(), file
    assert element in app, f"{element} must be actually mounted in App.tsx"
assert "<Sidebar" in app
assert "videoDecoderSupport" in hooks and "observedDecoders" in hooks
assert "recenterMirrorWindow" in app
for ipc in (
    "commands::preview_saved_screenshot",
    "commands::get_video_decoder_support",
    "commands::presentation_get_mode",
    "commands::presentation_set_mode",
    "commands::presentation_refresh_snapshot",
    "app_update::check_mirroleap_update",
):
    assert ipc in native, f"Native IPC handler missing: {ipc}"

for check in (
    "check_preview_clipboard.py", "check_wireless_center.py",
    "check_app_update.py", "check_video_decoder.py", "check_mirropresent.py",
):
    assert (root / "scripts" / check).exists(), f"Missing check {check}"
    assert "scripts/" + check in ci, f"Desktop CI must execute {check}"
assert '<canvas id="presentation"' in read("public/frame.html")
assert 'src="/present-overlay.js"' in read("public/frame.html")
assert 'data:image/png;base64,' in read("public/present-overlay.js")
assert "Run-MirroLeap.cmd" in read("README.md")
for expected in (
    f"MirroLeap_{version}_x64_Portable.zip",
    f"MirroLeap_{version}_x64-setup.exe",
    f"MirroLeap_{version}_x64_en-US.msi",
):
    assert expected in read("README.md"), expected
print(f"MirroLeap v{version}: five integrated UI features, Tauri IPC, native dependencies, security scopes, "
      "portable release metadata and CI regression gates verified")
