#!/usr/bin/env python3
"""Guard the MirroLeap name, compact default and advanced feature access."""
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1]
app = (root / "src/App.tsx").read_text(encoding="utf-8")
home = (root / "src/components/CompactDashboard.tsx").read_text(encoding="utf-8")
config = json.loads((root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
package = json.loads((root / "package.json").read_text(encoding="utf-8"))
src = (root / "src/hooks/useScrcpy.ts").read_text(encoding="utf-8")

assert package["name"] == "mirroleap"
assert config["productName"] == config["app"]["windows"][0]["title"] == "MirroLeap"
assert config["version"] == package["version"]
assert config["app"]["windows"][0]["width"] <= 1000
assert config["app"]["windows"][0]["height"] <= 720
assert config["identifier"] == "com.kb.scrcpy-gui-v3", "Preserve existing app identity"
assert "scrcpy_config" in src and "scrcpy_window_positions" in src, (
    "Do not erase or move existing user configuration storage"
)
assert "useState<'home' | 'settings'>('home')" in app
assert "<CompactDashboard" in app and "onOpenSettings={openSettings}" in app
for component in (
    "<Sidebar", "<ControlPanel", "<SessionBehavior", "<ShortcutsPanel",
    "<Header", "<LogPanel", "<Footer",
):
    assert component in app, f"Advanced setting must remain accessible: {component}"
for tab in ("devices", "mirror", "behavior", "appearance", "tools"):
    assert f"settingsTab === '{tab}'" in app
for prop in ("onSelectDevice", "onStart", "onStop", "onResolutionChange",
             "onDownloadCore", "onConfigChange", "onOpenSettings"):
    assert prop in home
assert "Mirro<span>Leap</span>" in home
assert (root / "public/mirroleap-icon.svg").is_file()
assert (root / "src-tauri/icons/icon.ico").is_file()
assert (root / "src-tauri/icons/icon.icns").is_file()
print("MirroLeap compact home, full settings access, branded app and legacy storage verified")
