#!/usr/bin/env python3
"""Bounded static integration checks for MirroCreator HQ frame IPC prototype.

Does not download scrcpy, execute Android commands, or enable experimental HQ.
On Windows CI, Rust itself additionally compiles and tests the mapping owner.
"""
from __future__ import annotations

import importlib.util
import re
from pathlib import Path

root = Path(__file__).resolve().parents[1]


def read(name: str) -> str:
    return (root / name).read_text(encoding="utf-8")


def numeric_macro(header: str, name: str) -> int:
    match = re.search(
        r"^#define\s+" + re.escape(name) + r"\s+(\d+)u(?:\s|$)", header, re.M
    )
    assert match is not None, f"Missing numeric macro {name}"
    return int(match.group(1))


header = read("hq/include/mirroleap_hq_protocol.h")
rust = read("src-tauri/src/hq_protocol.rs")
bridge = read("src-tauri/src/hq_bridge_windows.rs")
producer = read("hq/scrcpy/mirroleap_hq_sink.c")
sink_header = read("hq/scrcpy/mirroleap_hq_sink.h")
patch_script = read("hq/apply_scrcpy_v4.py")
app = read("src-tauri/src/commands.rs")
lib = read("src-tauri/src/lib.rs")

assert numeric_macro(header, "MLHQ_VERSION") == 1
assert numeric_macro(header, "MLHQ_HEADER_BYTES") == 64
assert numeric_macro(header, "MLHQ_SLOT_BYTES") == 80
assert numeric_macro(header, "MLHQ_SLOT_COUNT") == 2
assert numeric_macro(header, "MLHQ_MAX_WIDTH") == 3840
assert numeric_macro(header, "MLHQ_MAX_HEIGHT") == 3840
assert "MLHQ_MAX_PIXELS" in header
assert "MAX_PIXELS: usize = 3840 * 2160" in rust
for prefix, fragment in [
    ("MLHQ_G_LATEST_SLOT", "20"),
    ("MLHQ_S_SEQUENCE", "0"),
    ("MLHQ_S_FORMAT", "4"),
    ("MLHQ_S_WIDTH", "8"),
    ("MLHQ_S_HEIGHT", "12"),
    ("MLHQ_S_PTS", "16"),
    ("MLHQ_S_STRIDE0", "24"),
    ("MLHQ_S_STRIDE1", "28"),
    ("MLHQ_S_STRIDE2", "32"),
    ("MLHQ_S_OFFSET0", "36"),
    ("MLHQ_S_OFFSET1", "40"),
    ("MLHQ_S_OFFSET2", "44"),
    ("MLHQ_S_LENGTH0", "48"),
    ("MLHQ_S_LENGTH1", "52"),
    ("MLHQ_S_LENGTH2", "56"),
    ("MLHQ_S_PAYLOAD", "60"),
    ("MLHQ_S_ROTATION", "64"),
]:
    assert numeric_macro(header, prefix) == int(fragment), prefix

assert "MAX_WIDTH: u32 = 3840" in rust
assert "MAX_HEIGHT: u32 = 3840" in rust
assert "SLOT_HEADER_BYTES: usize = 80" in rust
assert "SLOT_CAPACITY" in rust and "MAPPING_BYTES" in rust
assert "validate_frame_header" in rust and "read_latest_snapshot" in rust
assert "HQ frame planes must be packed and canonical" in rust
assert "HQ frame is not committed" in rust
assert "MIRROLEAP_HQ_TOKEN" in producer and "MLHQ_MAP_PREFIX" in header
assert "InterlockedExchangePointer" in producer
assert "MemoryBarrier()" in producer and "InterlockedIncrement(sequence)" in producer
assert "AV_PIX_FMT_YUV420P" in producer and "AV_PIX_FMT_NV12" in producer
assert "av_frame_clone(frame)" in producer and "SetEvent(sink->ready)" in producer
assert "return true; // HQ failures MUST NOT terminate normal scrcpy playback" in producer
assert "struct sc_frame_sink frame_sink" in sink_header
assert "sc_frame_source_add_sink(&s->video_decoder.frame_source," in patch_script
assert "PINNED_COMMIT" in patch_script and "2322868e9e256eb5fce0b3d659ab2a409f29bae1" in patch_script

# The two named maps must have the exact same prefix. A nonce is generated
# by Windows and never derived from the Android device ID or hostname.
assert "Local\\\\MirroLeapHQ_" in bridge and 'MLHQ_MAP_PREFIX' in header
assert "BCryptGenRandom" in bridge and "GetLastError()" in bridge
assert "Try" not in sink_header or "frame_sink" in sink_header
assert "try_latest" in bridge and "Ordering::Acquire" in bridge
assert "first!=second" in bridge and "validate_frame_header" in bridge
assert "mod hq_protocol" in lib and "mod hq_bridge_windows" in lib
# Stage 2 CPU reference crop proves source-resolution math before any GPU
# presentation is exposed. It must NOT become a hidden duplicate capture stream.
geometry = read("src-tauri/src/hq_geometry.rs")
raster = read("src-tauri/src/hq_reference_render.rs")
assert "mod hq_geometry" in lib and "mod hq_reference_render" in lib
assert "map_lens_to_decoded_frame" in geometry
assert "source_width" in geometry and "source_center_x" in geometry
assert "render_decoded_reference" in raster
assert "PixelFormat::Yuv420p" in raster and "PixelFormat::Nv12" in raster
assert "BGRA_BACKGROUND" in raster and "bilinear_plane" in raster
assert "MAX_REFERENCE_DIAMETER: u32 = 720" in raster
assert "render_decoded_reference" not in app, "reference path must stay disabled in production"


# The client binary is still official by default; this source patch must not
# auto-enable any second video stream or user-visible HQ status.
assert "MIRROLEAP_HQ_TOKEN" not in app
assert "start_scrcpy" not in producer
assert "http://" not in producer and "https://" not in producer
assert "screencap" not in producer
assert "HQ_FRAME_ACTIVE" not in read("src/components/MirroPresentPanel.tsx")

spec = importlib.util.spec_from_file_location("mirroleap_hq_patch", root / "hq/apply_scrcpy_v4.py")
assert spec and spec.loader
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
sample = "begin\nANCHOR\nend"
assert mod.add_once(sample, "ANCHOR", "ANCHOR + PATCH", "fixture") == "begin\nANCHOR + PATCH\nend"
assert mod.add_once("begin\nANCHOR + PATCH\nend", "ANCHOR", "ANCHOR + PATCH", "fixture") == "begin\nANCHOR + PATCH\nend"
try:
    mod.add_once(sample, "MISSING", "PATCH", "fixture")
except ValueError:
    pass
else:
    raise AssertionError("wrong scrcpy source must be rejected")

print("HQ v1 frame IPC: pinned source, opt-in second sink, Windows mapping owner, "
      "YUV420P/NV12 layout, non-blocking frame handoff and fail-closed checks passed "
      "(NO high-definition video renderer shipped)")
