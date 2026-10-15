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

# Verify the optional native tool rail and rounded screen clipping survive
# future refactors. This is a static safety check; Windows UI still needs an
# actual physical mouse and monitor test.
controls_page = (root / "public/phone-controls.html").read_text(encoding="utf-8")
backend = (root / "src-tauri/src/device_frame.rs").read_text(encoding="utf-8")
capability = (root / "src-tauri/capabilities/default.json").read_text(encoding="utf-8")
assert all(f'id="{key}"' in controls_page for key in ("drag", "minimize", "close"))
assert all(f"act('{key}')" in controls_page for key in ("drag", "minimize", "close"))
assert 'phone-controls-*' in capability
# Do NOT reintroduce Win32 region clipping: it is a hard-pixel mask,
# incompatible with WebView2 antialiasing and creates wallpaper-colored seams.
assert not re.search(r"\\bSetWindowRgn\\s*\\(", backend), (
    "Win32 SetWindowRgn reintroduced: it leaks cyan wallpaper behind rounded SVG edges"
)
assert not re.search(r"\\bCreateRoundRectRgn\\s*\\(", backend), (
    "GDI hard rounded mask must not be used under SVG antialiasing"
)
seal = top.find('.//svg:g[@id="screen-seam-seal"]', ns)
assert seal is not None, "Screen corner matte / pixel seam seal is missing"
matte = next((n for n in seal if n.attrib.get("mask") == "url(#transparentScreen)"), None)
assert matte is not None and matte.attrib.get("fill") == "#06080b", (
    "The four client corner wedges must have an opaque dark matte"
)
gasket = seal.find('svg:rect[@id="video-edge-gasket"]', ns)
assert gasket is not None, "SVG must cover subpixel seams on inner video perimeter"
for attr, expected in {
    "x": "18", "y": "18", "width": "964", "height": "2114", "rx": "57",
    "stroke": "#06080b", "stroke-width": "9", "fill": "none",
}.items():
    assert gasket.attrib.get(attr) == expected, f"Screen gasket {attr} drifted"
# Gasket extends at least 3 physical pixels over a common 720px scrcpy
# video; this conceals the observed 1-2px high-DPI bleed without a thick bezel.
assert (9 / 2) * 720 / 964 > 3.0, "Insufficient screen-edge overdraw"
assert "set_ignore_cursor_events(true)" in backend, "Main overlay must stay click-through"
assert "GetAsyncKeyState" in backend, "Dragging must end when left mouse button is released"
assert "is_frame_edge_hover" in backend, "Controls must auto-appear only at the bezel edge"
assert "TOOLBAR_HEIGHT" in rust and "round_clip_radius" in rust
assert 'stroke="#d3d5d6"' not in svg.read_text(encoding="utf-8"), (
    "Bright white perimeter outline must not be reintroduced"
)

print("Phone frame: SVG corner matte, dark seam gasket and click-through controls verified")
