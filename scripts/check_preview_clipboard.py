#!/usr/bin/env python3
"""Guard the second-stage MirroCapture preview+copy feature boundaries."""
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1]
caps = {
    f.stem: json.loads(f.read_text(encoding="utf-8"))
    for f in (root / "src-tauri/capabilities").glob("*.json")
}
native = (root / "src-tauri/src/commands.rs").read_text(encoding="utf-8")
frontend = (root / "src/components/ScreenshotTools.tsx").read_text(encoding="utf-8")
lib = (root / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
crate = (root / "src-tauri/Cargo.toml").read_text(encoding="utf-8")

assert "clipboard-manager:allow-write-image" in caps["default"]["permissions"]
for label in ("controls", "splash", "frame-ack"):
    assert not any("clipboard" in s for s in caps[label]["permissions"]), label
assert not any("clipboard-manager:allow-read" in p for p in caps["default"]["permissions"])
assert "tauri-plugin-clipboard-manager" in crate
assert 'features = ["image-png"]' in crate
assert "tauri_plugin_clipboard_manager::init()" in lib
assert "commands::preview_saved_screenshot" in lib
assert "require_main_window(&window)?" in native
assert "trusted_preview_path(&folder, &path)?" in native
assert "file.parent() != Some(home.as_path())" in native
assert "allowed_capture_preview_name" in native
assert "PREVIEW_LIMIT: u64 = 12 * 1024 * 1024" in native
assert "std::fs::metadata(&source)" in native
assert "valid_android_screenshot_png(&screenshot)" in native
assert "Image.fromBytes(bytes)" in frontend
assert "await writeImage(image)" in frontend
assert "await image.close()" in frontend
assert "generation.current" in frontend
assert "setPreviewUrl(null)" in frontend
print("MirroCapture read-only bounded PNG preview, main-only clipboard write and device-reset guards verified")
