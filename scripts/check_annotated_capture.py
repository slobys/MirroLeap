#!/usr/bin/env python3
"""Cross-layer guard for original screenshot + transparent paint export."""
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1]
frame = (root / "public/frame.html").read_text(encoding="utf-8")
native = (root / "src-tauri/src/device_frame.rs").read_text(encoding="utf-8")
commands = (root / "src-tauri/src/commands.rs").read_text(encoding="utf-8")
compositor = (root / "src-tauri/src/capture_compositor.rs").read_text(encoding="utf-8")
interface = (root / "src/components/ScreenshotTools.tsx").read_text(encoding="utf-8")
handler = (root / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
app = (root / "src/App.tsx").read_text(encoding="utf-8")
config = json.loads((root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))

assert config["identifier"] == "com.kb.scrcpy-gui-v3"
assert "captureSnapshot() {" in frame
assert "canvas.toDataURL('image/png')" in frame
assert "strokes.length" in frame
assert "capture_ink_snapshot(" in native
assert "acknowledge_ink_capture(" in native
assert 'caller.label().starts_with("phone-frame-")' in native
assert "expected == caller.label()" in native
assert "CAPTURE_ACK_NONCE.fetch_add" in native
assert "Duration::from_secs(5)" in native
assert "device_frame_capture_ack" in native and "device_frame_capture_ack" in handler
assert "save_annotated_screenshot" in commands
assert "save_annotated_screenshot" in handler and "save_annotated_screenshot" in interface
assert "capture_device_png(&device, custom_path).await?" in commands
assert "tokio::task::spawn_blocking" in commands
assert '"MirroLeap-Annotated"' in commands
assert "composite_annotated_png(" in compositor
assert "sample_overlay(" in compositor
assert "ColorType::Rgba" in compositor
assert 'ScreenshotTools device={activeDevice}' in app
assert (root / "src/test/annotationCapture.test.mjs").exists()
print("Canvas snapshot, scoped native IPC, PNG compositing and UI export path verified")
