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

print("Phone frame: SVG aperture, Rust pixel geometry and HTML transparency agree")
