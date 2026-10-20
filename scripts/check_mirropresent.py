#!/usr/bin/env python3
"""MirroPresent v5.0 safety & integration contract.

Static guard, not a substitute for Windows/AOSP device presentation tests.
Checks important pointer-input and per-device access invariants so an
unverified enhancement cannot silently break existing scrcpy touch control.
"""
from pathlib import Path
import json
import re

root = Path(__file__).resolve().parents[1]
html = (root / "public/frame.html").read_text(encoding="utf-8")
present = (root / "public/present-overlay.js").read_text(encoding="utf-8")
native = (root / "src-tauri/src/device_frame.rs").read_text(encoding="utf-8")
commands = (root / "src-tauri/src/commands.rs").read_text(encoding="utf-8")
lib = (root / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
main = (root / "src/App.tsx").read_text(encoding="utf-8")
panel = (root / "src/components/MirroPresentPanel.tsx").read_text(encoding="utf-8")
cfg = json.loads((root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
pkg = json.loads((root / "package.json").read_text(encoding="utf-8"))

assert pkg["version"] == cfg["version"] and cfg["identifier"] == "com.kb.scrcpy-gui-v3"
assert "present-overlay.js" in html and '<canvas id="presentation"' in html
assert "body.presenting #presentation { pointer-events: auto; }" in html
assert 'body.inking #annotation { pointer-events: auto; }' in html
assert "window.scrcpyPresent = Object.freeze" in present
assert "new Set(['off', 'spotlight', 'magnifier', 'live'])" in present
assert "MutationObserver" in present and "snapshot = null" in present
assert "pointermove" in present and "destination-out" in present
assert "Magnification control over the SDL video" in present
assert "await new Promise" in present
assert "window.scrcpyInk = Object.freeze" in html, "Drawing preserved"
assert "presentation_set_mode" in lib and "presentation_get_mode" in lib
assert "presentation_refresh_snapshot" in lib
assert "require_main_window(&window)?;" in commands
assert "validate_capture_device(&device)?;" in commands
assert '"magnifier" | "live"' in commands, "Both still and continuous lens require verified mode"
assert "capture_device_png(device, custom_path.clone()).await?" in commands
assert "retryable_presenter_orientation_error" in commands
assert "Duration::from_secs(24)" in commands
assert "set_presentation_snapshot(app, device, &png).await" in commands
assert "refresh_presenter_image(&app_handle, &device, custom_path).await" in commands
assert "clip-path: inset(0 round" in html, "Presenter canvas must stay inside the rounded video aperture"
assert "canvas.addEventListener('wheel'" in present and "zoomFactor" in present
assert "present_is_magnifier" in native and "present_toggle" in native
assert "known_present_mode(" in native and "PresentationTransitionGuard::begin" in native
assert "overlay.set_ignore_cursor_events(true)" in native
assert "overlay.set_ignore_cursor_events(false)" in native
assert "promote_drawing_windows(app, device, &overlay)" in native
assert 'set_ignore_cursor_events(mode == "live")' in native
assert 'native_live_lens::update' in native
assert 'WindowFromPoint' in native and 'GetAncestor(hovered,GA_ROOT)' in native
assert 'start_creator_click_sampler' in native
assert 'MagSetWindowSource' in (root / "src-tauri/src/native_live_lens.rs").read_text()
assert "mode === 'off' || mode === 'live'" in present

assert "update_presentation_mode(app, device, \"off\")" in native
assert "set_presentation_snapshot(" in native
assert "Screenshot is not" not in native
assert "MAX_PRESENTER_BYTES: usize = 8 * 1024 * 1024" in native
assert 'device_frame_ack' in native and "INK_ACK_NONCE.fetch_add" in native
assert "Duration::from_secs(8)" in native
assert "<MirroPresentPanel device={activeDevice}" not in main, "Avoid duplicated presenter tools in Settings"
assert 'id="present-toggle"' in (root / "public/phone-controls.html").read_text(), (
    "MirroPresent must remain available on the floating phone rail"
)
assert "Creator masks only cover the visible overlay" in panel
assert "Live lens uses the Windows desktop compositor over SDL video" in panel
assert "presentations" not in cfg.get("identifier", "")
assert (root / "src/test/mirroPresent.test.mjs").exists()
assert (root / "src/components/__tests__/MirroPresentPanel.test.tsx").exists()
assert re.search(r"(?m)^version = \""+re.escape(pkg["version"])+r"\"$", (root / "src-tauri/Cargo.toml").read_text())
print("MirroPresent main-only device scope, orientation-safe snapshots, native pointer restore and UI wiring passed")
