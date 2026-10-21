#!/usr/bin/env python3
"""Prevent the SVG's real transparent screen aperture from drifting off scrcpy.

Only standard library; runs in CI. The overlay is a *separate native window*,
not an editable screenshot. If its SVG and Rust geometry diverge, the metal
frame will visibly cover live video or leave an ugly black gap.
"""
from pathlib import Path
import re
import xml.etree.ElementTree as ET

root = Path(__file__).resolve().parents[1]
svg = root / "public/device-frame/iphone-black-portrait.svg"
html = root / "public/frame.html"
layout = root / "src-tauri/src/frame_layout.rs"

ns = {"svg": "http://www.w3.org/2000/svg"}
tree = ET.parse(svg)
top = tree.getroot()
assert top.attrib["viewBox"] == "0 0 1000 2150", "SVG artboard size changed"

mask = top.find('.//svg:mask[@id="transparentScreen"]', ns)
assert mask is not None, "SVG no longer has a transparent screen mask"
hole = next(
    (child for child in mask if child.attrib.get("fill") == "#000000"),
    None,
)
assert hole is not None, "SVG no longer has a clear aperture"
for key, expected in {
    "x": "18", "y": "18", "width": "964", "height": "2114"
}.items():
    assert hole.attrib.get(key) == expected, f"SVG hole {key} changed: {hole.attrib}"

rust = layout.read_text(encoding="utf-8")
for field, value in [
    ("ARTBOARD_WIDTH", "1000.0"),
    ("ARTBOARD_HEIGHT", "2150.0"),
    ("INNER_X", "18.0"),
    ("INNER_Y", "18.0"),
]:
    assert re.search(
        rf"pub const {field}:\s*f64\s*=\s*{re.escape(value)}\s*;",
        rust
    ), f"Rust geometry {field} differs from SVG"

page = html.read_text(encoding="utf-8")
assert 'src="/device-frame/iphone-black-portrait.svg"' in page
assert "object-fit: fill" in page
assert "background: transparent" in page
assert "pointer-events: none" in page
assert not any(node.attrib.get("id") in ("island", "notch", "speaker")
               for node in top.iter()), "No simulated iPhone camera cutout allowed"
assert not any(
    element.tag.endswith("text") for element in top.iter()
), "The frame must not draw a fake clock, status bar or UI"

# Verify the native hover rail and SVG-based rounded corner seal survive
# future refactors. This is a static safety check; Windows UI still needs an
# actual physical mouse and monitor test.
controls_page = (root / "public/phone-controls.html").read_text(encoding="utf-8")
backend = (root / "src-tauri/src/device_frame.rs").read_text(encoding="utf-8")
capability = (root / "src-tauri/capabilities/default.json").read_text(encoding="utf-8")
controls_capability = (root / "src-tauri/capabilities/controls.json").read_text(encoding="utf-8")
assert all(f'id="{key}"' in controls_page for key in ("drag", "minimize", "close"))
assert all(f"act('{key}')" in controls_page for key in ("drag", "minimize", "close"))
assert 'phone-controls-*' in controls_capability
assert 'phone-controls-*' not in capability, "Phone controls must not inherit main UI permissions"
# The v4.3.3 SVG corner matte was a SHARP SQUARE RECTANGLE, which itself
# protruded beyond the handset's rounded outer silhouette. Do not bring it back.
seal = top.find('.//svg:g[@id="screen-seam-seal"]', ns)
assert seal is not None, "Rounded seam gasket is missing"
assert not any(
    n.attrib.get("mask") == "url(#transparentScreen)" for n in seal
), "Square masked corner matte would protrude beyond the phone silhouette"
assert all(not (
    n.tag.endswith("rect") and n.attrib.get("fill") in ("#06080b", "#000000")
) for n in seal), "Opaque rectangular matte detected"

gasket = seal.find('svg:rect[@id="video-edge-gasket"]', ns)
assert gasket is not None, "Antialiased gasket is missing"
for attr, expected in {
    "x": "18", "y": "18", "width": "964", "height": "2114", "rx": "72",
    "stroke": "#06080b", "stroke-width": "6", "fill": "none"
}.items():
    assert gasket.attrib.get(attr) == expected, f"Screen gasket {attr} drifted"
# Visual quality regression: the old skin stacked SEVEN separate dark
# rectangles and multiple metallic outlines, which read as a doubled border
# on dark Windows desktops. The streamlined SVG has exactly ONE thin metal
# contour and ONE continuous glass backing. Both orientations share the same
# portrait-space geometry (landscape rotates it, no extra fake layer).
for name in ("iphone-black-portrait.svg", "iphone-black-landscape.svg"):
    asset = ET.parse(root / "public/device-frame" / name).getroot()
    assert asset.attrib.get("shape-rendering") == "geometricPrecision", (
        f"{name}: preserve the smooth SVG quarter-ellipse at nonintegral Windows DPI"
    )
    bodies = asset.findall('.//svg:g[@mask="url(#transparentScreen)"]', ns)
    assert len(bodies) == 1, f"{name}: expected one masked phone body"
    layers = list(bodies[0])
    assert [layer.attrib.get("id") for layer in layers] == [
        "phone-titanium-outline", "phone-glass-bezel",
    ], f"{name}: removed multi-ring body was reintroduced"
    assert all(layer.tag.endswith("rect") for layer in layers)
    metal, glass = layers
    assert (metal.attrib["x"], metal.attrib["y"], metal.attrib["width"],
        metal.attrib["height"], metal.attrib["rx"]) == ("1","1","998","2148","76")
    assert metal.attrib["fill"] == "url(#titanium)"
    assert glass.attrib["fill"] == "#0a0c10"
    assert float(glass.attrib["x"]) < 4.5, "metal needs a hairline, not a thick border"
    assert float(glass.attrib["rx"]) > 70
    assert "stroke" not in metal.attrib and "stroke" not in glass.attrib
    gasket_for_asset = asset.find('.//svg:g[@id="screen-seam-seal"]/svg:rect[@id="video-edge-gasket"]',ns)
    assert gasket_for_asset is not None
    assert gasket_for_asset.attrib["stroke-width"] == "6"
    assert gasket_for_asset.attrib["fill"] == "none"
    assert asset.find('.//svg:linearGradient[@id="edge"]',ns) is None, (
        "A second glossy metallic stripe would restore the unwanted extra ring"
    )

# The SDL VIDEO mask is safely oversized behind the glass aperture.
# The separate NATIVE OUTER BODY mask is one pixel OUTSIDE the SVG's visible
# titanium contour, so hard GDI raster steps cannot amputate SVG alpha.
assert "CreateRoundRectRgn" in backend and "SetWindowRgn" in backend
assert "native_clip_radius" in backend and "reset_native_corner_clip" in backend
assert "SCREEN_CORNER_RADIUS: f64 = 72.0" in rust
assert "NATIVE_CORNER_OVERSCAN_PX: i32 = 4" in rust
# SDL's Alt+F can drop or replace the actual Win32 HRGN while the window
# returns to its PREVIOUS width/height. Stale size-only caches leave four
# phone-content corners protruding beyond the antialiased metal bezel.
assert "last_client_size" not in backend, "Never cache the clip by only PID and dimensions"
assert "GetWindowRgn(hwnd, probe)" in backend and "PtInRegion(" in backend, (
    "The native clip must be checked, not merely inferred from its cached size"
)
assert "last_clip_signature != signature" in backend
assert "native_corner_clip_intact(hwnd, screen)" in backend
assert "clip_window_rect(hwnd, screen)" in backend, (
    "Recompute the window-relative client inset after SDL style changes"
)
assert "is_fullscreen_client(screen, monitor)" in backend
assert "ResetForFullscreen" in backend and "ApplyRounded" in backend
assert "corner_clip_action(" in rust
assert "fn fullscreen_exit_recreates_native_rounded_clip_even_at_identical_size()" in rust
assert "if recovering { 130 } else { 550 }" in backend
assert "clip_recovery_until = Some(Instant::now() + Duration::from_secs(3))" in backend
assert "CreateRoundRectRgn" in backend and "SetWindowRgn" in backend

# A native video HRGN alone DID NOT stop the black rectangular corner
# fragments after fullscreen. Clip the transparent WebView2 case HWND too,
# and constrain the SDL video HRGN to the physical phone silhouette.
assert "BEZEL_OUTER_RADIUS: f64 = 76.0" in rust
assert "BEZEL_CLIP_OVERSCAN_PX: i32 = 1" in rust
assert "BEZEL_CLIP_INSET_PX" not in rust
assert "let overscan = crate::frame_layout::BEZEL_CLIP_OVERSCAN_PX;" in backend
assert "left: -overscan, top: -overscan" in backend
assert "frame.x-window_rect.left-overscan" in backend
assert "native_corner_mask_respects_svg_antialias_at_common_windows_dpi" in rust
assert "bezel_outer_clip_radii(" in rust and "native_outer_bezel_clip_intact(" in backend
assert "apply_native_outer_bezel_clip(shell)" in backend
assert "last_outer_mask_geometry" in backend
assert "GetWindowRgn(hwnd,region)" in backend
assert "CombineRgn(Some(region),Some(video_region),Some(physical),RGN_AND)" in backend
assert "Restored phone body HWND silhouette after fullscreen" in backend
assert "fn bezel_outer_region_rejects_square_window_corners_after_fullscreen()" in rust

# SDL's Windows fullscreen implementation resets DWM-managed border and
# corner preferences independently of the user-code Win32 HRGN. A healthy
# GDI region cannot prove that its DWM non-client pixels are rounded.
dwm = (root / "src-tauri/src/win_chrome.rs").read_text(encoding="utf-8")
cargo = (root / "src-tauri/Cargo.toml").read_text(encoding="utf-8")
lib = (root / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
assert 'mod win_chrome;' in lib
assert '"Win32_Graphics_Dwm"' in cargo
assert "DwmSetWindowAttribute(hwnd, DWMWA_NCRENDERING_POLICY" in dwm
assert "DWMNCRP_DISABLED" in dwm
assert "DWMWA_WINDOW_CORNER_PREFERENCE" in dwm and "DWMWCP_DONOTROUND" in dwm
assert "DWMWA_BORDER_COLOR" in dwm and "DWMWA_COLOR_NONE" in dwm
assert "DwmGetWindowAttribute(" in dwm
assert "CS_DROPSHADOW" in dwm, "Include window-class shadow in diagnostic report"
assert "DWMWA_EXTENDED_FRAME_BOUNDS" in dwm
assert "last_dwm_report" in backend
assert "crate::win_chrome::neutralize(hwnd)" in backend
assert "crate::win_chrome::neutralize(shell)" in backend
assert "[FRAME][DWM] SDL fullscreen transition" in backend
assert "if !fullscreen && dwm_checked_at.is_none_or" in backend
assert "DwmSetWindowAttribute" not in (root / "src-tauri/src/commands.rs").read_text(encoding="utf-8"), (
    "Never mutate DWM for the MirroLeap main app, only framed video/shell HWNDs"
)

assert "round_clip_radius" not in backend, (
    "Using exact SVG radius for native mask causes wallpaper-colored seams"
)
# A thin gasket (3.5 SVG units per side) is enough to smooth underscanned GDI
# edges without introducing a thick, square, black overlay.
assert (6 / 2) * 720 / 964 > 2.0, "Insufficient gasket overdraw"
assert (6 / 2) * 720 / 964 < 3.0, "Gasket would visibly thicken the bezel"

assert "set_ignore_cursor_events(true)" in backend, "Main overlay must stay click-through"
assert "GetAsyncKeyState" in backend, "Dragging must end when left mouse button is released"
assert "is_frame_edge_hover" in backend, "Controls must auto-appear only at the bezel edge"
assert "TOOLBAR_HEIGHT" in rust and "native_clip_radius" in rust
assert 'stroke="#d3d5d6"' not in svg.read_text(encoding="utf-8"), (
    "Bright white perimeter outline must not be reintroduced"
)

# Native WebView2 transparent first paint must be validated BEFORE reveal.
# PageLoadEvent::Finished alone was insufficient: the initial opaque HWND
# could cover the SDL video while the frozen magnifier stayed visible.
first_paint = (root / "public/frame-ready.js").read_text(encoding="utf-8")
commands = (root / "src-tauri/src/commands.rs").read_text(encoding="utf-8")
assert '<script src="/frame-ready.js"></script>' in page
assert "frame.decode()" in first_paint and "paintedOrHidden()" in first_paint
assert "requestAnimationFrame(() => requestAnimationFrame(finish))" in first_paint
assert "setTimeout(finish, 240)" in first_paint, "A hidden WebView may throttle rAF"
assert "document.documentElement,document.body,annotation,presentation" in first_paint
assert "device_frame_page_ready" in first_paint and "device_frame_page_ready" in commands
assert "commands::device_frame_page_ready" in lib
assert "acknowledge_frame_first_paint" in backend
assert "caller.hwnd()" in backend and "current.hwnd()" in backend
assert "frame_first_paints().lock()" in backend
assert "PageLoadEvent::Started" in backend and "PageLoadEvent::Finished" in backend
assert "if !frame_presentation_ready(&device)" in backend
assert backend.index("if !frame_presentation_ready(&device)") < backend.index("if overlay.show().is_ok()")
assert ".background_color(tauri::window::Color(0,0,0,0))" in backend
assert "overlay.set_decorations(false)" in backend

print("Phone frame: no square matte, overscanned native clip and narrow anti-aliased gasket verified")
