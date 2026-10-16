#!/usr/bin/env python3
"""Guard the landscape skin, pen-reset, grouped toolbar and restore lifecycle.

These static invariants supplement unit tests. A real Windows desktop remains
necessary to test focus and DWM transparency on different DPI configurations.
"""
from pathlib import Path
import re
import xml.etree.ElementTree as ET

root = Path(__file__).resolve().parents[1]
art = root / "public/device-frame"
portrait = ET.parse(art / "iphone-black-portrait.svg").getroot()
landscape = ET.parse(art / "iphone-black-landscape.svg").getroot()
ns = {"svg": "http://www.w3.org/2000/svg"}
assert portrait.get("viewBox") == "0 0 1000 2150"
assert landscape.get("viewBox") == "0 0 2150 1000"
assert landscape.find(".//svg:g[@id='landscape-rotation']", ns) is not None
rotated = landscape.find(".//svg:g[@id='landscape-rotation']", ns)
assert rotated is not None and rotated.get("transform") == "translate(2150 0) rotate(90)"
for root_svg in (portrait, landscape):
    mask = root_svg.find(".//svg:mask[@id='transparentScreen']", ns)
    assert mask is not None
    aperture = [e for e in mask if e.attrib.get("fill") == "#000000"]
    assert len(aperture) == 1
    for key, expected in {"x": "18", "y": "18", "width": "964", "height": "2114",
                          "rx": "72"}.items():
        assert aperture[0].get(key) == expected, f"SVG aperture {key} differs"
# The geometry is rotated with the entire portrait group, rather than
# stretched to landscape dimensions as an unrotated portrait phone.
assert "mask=\"url(#transparentScreen)\"" in (art / "iphone-black-landscape.svg").read_text()

layout = (root / "src-tauri/src/frame_layout.rs").read_text(encoding="utf-8")
native = (root / "src-tauri/src/device_frame.rs").read_text(encoding="utf-8")
frame = (root / "public/frame.html").read_text(encoding="utf-8")
toolbar = (root / "public/phone-controls.html").read_text(encoding="utf-8")
ci = (root / ".github/workflows/desktop-ci.yml").read_text(encoding="utf-8")

assert "LANDSCAPE_INNER_WIDTH: f64 = INNER_HEIGHT" in layout
assert "LANDSCAPE_INNER_HEIGHT: f64 = INNER_WIDTH" in layout
assert "is_landscape(screen.width, screen.height)" in native
assert "screen.width >= screen.height" not in layout, "Landscape must show frame"
assert "native_clip_radius(video_width" in layout and "is_landscape(video_width, video_height)" in layout

assert "html[data-orientation=\"landscape\"] #annotation" in frame
assert "window.scrcpyFrame = Object.freeze" in frame
assert "setOrientation(orientation)" in frame
assert "iphone-black-' + value + '.svg" in frame
assert "strokes.length = 0" in frame, "Clear portrait-relative ink on rotation"
assert "window.scrcpyFrame?.setOrientation" in native

# Controls order is a UX guarantee; one-click restore is visible even when
# pen tools are collapsed, and close must stay immediately after minimize.
expected_order = ["drag", "minimize", "close", "ink-toggle", "quick-toggle", "restore", "ink-tools", "quick-tools"]
offsets = [toolbar.index('id="' + element + '"') for element in expected_order]
assert offsets == sorted(offsets), f"Toolbar order regressed: {expected_order}"
assert 'class="session-actions"' in toolbar and 'class="annotation-actions"' in toolbar
assert "act('ink_reset'" in toolbar
assert "action == \"ink_reset\"" in native
assert "window.scrcpyInk.clear(); window.scrcpyInk.setEnabled(false)" in native
assert "set_ignore_cursor_events(true)" in native, "One-click reset must recover touch"

# Minimized scrcpy must not cause a permanent stop in the geometry watcher.
assert "let needs_restore = !shell_visible || !actual_visible" in native
assert "IsWindowVisible(h).as_bool()" in native
assert "let actually_visible = toolbar.hwnd().ok()" in native
assert "Repositioning toolbar on next tick" in native
assert "let wanted = close_to_edge ||" in native
assert "pub const TOOLBAR_WIDTH: i32 = 62;" in layout
assert "pub const TOOLBAR_HEIGHT: i32 = 322;" in layout
assert "pub const ANNOTATION_TOOLBAR_WIDTH: i32 = 268;" in layout
assert "pub const ANNOTATION_STACK_WIDTH: i32 = 168;" in layout
assert "pub const ANNOTATION_TOOLBAR_HEIGHT: i32 = 538;" in layout
assert "pub fn toolbar_placement(" in layout and "ToolbarLayout::StackRight" in layout
assert "visible_top = frame.y.max(desktop.y)" in layout
assert "visible_bottom = frame.y.saturating_add(frame.height).min(desktop_bottom)" in layout
assert "pub const TOOLBAR_SAFE_MARGIN: i32 = 12;" in layout
assert "ToolbarLayout::ShortRight" in layout and "ToolbarLayout::TightInkRight" in layout
assert "monitor_work_area(hwnd)" in native and "GetMonitorInfoW" in native
assert "MONITOR_DEFAULTTONEAREST" in native and "info.rcWork" in native
assert "placement.zoom_percent" in native and "document.documentElement.style.zoom" in toolbar
assert "GetWindow(hwnd, GW_HWNDPREV)" in native, "Toolbar must keep SDL-relative stacking"
assert "pub fn companion_needs_reposition(" in layout, "Stable native companions must avoid per-tick SetWindowPos"
assert native.count("companion_needs_reposition(") >= 2, "Both frame and toolbar must use the movement gate"
assert "Duration::from_secs(2)" in native, "Native companion z-order needs periodic health check"
assert "shell_position_checked_at" in native and "toolbar_position_checked_at" in native
assert "recent_restore" in native and "video_was_unavailable" in native
assert "window.scrcpyControls?.setLayout" in native

# Release integrity and optional authentic code signing do not disable any
# Windows security mechanism. A trusted CA certificate must be provided.
sign = (root / "scripts/windows-sign-and-checksum.ps1").read_text(encoding="utf-8")
assert "Get-FileHash" in sign and "SHA256SUMS.txt" in sign
assert "signtool" in sign.lower() and "SCRCPY_SIGNING_PFX_PASSWORD" in sign
assert "SCRCPY_SIGNING_PFX_BASE64" in ci
assert "sha256sum --check SHA256SUMS.txt" in ci
assert "dist/device-frame/iphone-black-landscape.svg" in ci

print("Landscape SVG geometry, native restore, grouped pen actions and release checks passed")
