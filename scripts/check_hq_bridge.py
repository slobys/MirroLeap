#!/usr/bin/env python3
"""Static guardrails for opt-in MirroCreator HQ decoded-frame preview.

Never downloads scrcpy, runs Android commands or enables a user session.
On Windows CI, Rust compiles/tests the live native IPC and GDI reference lens.
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
assert "try_latest_with_publish_id" in bridge
assert "publish_before" in bridge and "publish_after" in bridge
assert "validate_header_prefix" in rust
assert "global_publish_id_advances_even_when_two_slots_have_equal_local_sequence" in bridge
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
pipeline = read("src-tauri/src/hq_pipeline.rs")
assert "mod hq_pipeline" in lib
assert "render_decoded_hq_preview" in pipeline
assert "map_lens_to_decoded_frame" in pipeline
assert "render_decoded_reference" in pipeline
assert "synthetic_c_packed_nv12_mapping_flows_into_bgra_pixels" in pipeline
assert "render_decoded_hq_preview" not in app, "experimental pixels must never run implicitly"
# CPU/GDI preview only becomes reachable after EXPLICIT user opt-in, verified
# patched scrcpy version, and creation of the per-device mapping owner. GPU
# production readiness still requires D3D11 and Android-device validation.
circle=read("src-tauri/src/hq_cpu_lens_windows.rs")
assert "mod hq_cpu_lens_windows" in lib
assert "UpdateLayeredWindow" in circle and "EX_TRANSPARENT" in circle
assert "circular_premultiplied_bgra" in circle
assert "render_decoded_hq_preview" in circle
assert "try_latest_with_publish_id" in circle
assert "cached_frame" in circle and "latest_publish_id" in circle
assert "lens_redraws_for_cursor_or_zoom_even_when_video_frame_is_unchanged" in circle
assert "lens_position" in circle and "MAX_LENS:u32 = 720" in circle
native=read("src-tauri/src/native_live_lens.rs")
phone=read("src-tauri/src/device_frame.rs")
ui=read("src/components/DecoderPerformance.tsx")
assert "hq_cpu_lens_windows::HqDecodedCpuLens" in native
assert "start_with_hq(device,owner)" in phone
assert "hqLens" in ui and "Optional 2.35× HQ lens" in ui
assert "CPU/GDI decoded-frame preview, not final D3D11" in ui
assert "hqlens" not in producer.lower()




# Manual-only Windows session probe is a deterministic producer-side
# diagnostic; without explicit -PatchedScrcpy it must not start a client.
probe = read("hq/windows-frame-probe.ps1")
assert "MIRROLEAP_HQ_TOKEN" in probe
assert "MemoryMappedFile" in probe
assert "Get-LatestValidFrame" in probe and "ReadUInt32(24)" in probe
assert "128-bit" not in probe or "nonceBytes" in probe
assert "[switch]$SelfTest" in probe and "[string]$PatchedScrcpy" in probe
assert "No pixels stored" in probe

# Normal scrcpy is still unchanged by default. A unique '--version' marker
# must be verified BEFORE passing the session-private HQ token to the child.
assert "MIRROLEAP_HQ_TOKEN" in app
assert "if let Some(token)=hq_token" in app
assert "create_hq_session(&exe_path,&config).await?" in app
assert "HQ_VERSION_MARKER" in app and "hq_lens.unwrap_or(false)" in app
assert 'MIRROLEAP_HQ_TOKEN' not in read("src/hooks/useScrcpy.ts")
assert "MirroLeap HQ decoded-frame v1 (experimental)" in patch_script
# Lite-app invariant: 5MB EXE/MSI, separate SHA256-pinned HQ ZIP downloaded
# only on explicit opt-in, never during app startup or ordinary mirroring.
runtime=read("src-tauri/src/hq_runtime.rs")
mirroleap_config=read("src-tauri/tauri.conf.json")
workflow=read(".github/workflows/mirroleap-continuity.yml")
assert 'mod hq_runtime;' in lib
assert '"hq-preview-core/scrcpy-hq-bin"' not in mirroleap_config
assert '"productName": "MirroLeap"' in mirroleap_config
assert '"identifier": "com.kb.scrcpy-gui-v3"' in mirroleap_config
assert '"version": "5.2.8"' in mirroleap_config
assert 'write-updater-manifest.mjs' in workflow
assert 'MirroLeap_${version}_x64-setup.exe' in workflow
assert 'MIRROLEAP_HQ_CORE_SHA256' in runtime
assert 'MIRROLEAP_HQ_CORE_URL' in runtime
assert 'validated_archive_entry' in runtime and 'MAX_EXTRACT' in runtime
assert 'ensure_hq_runtime' in app and "crate::hq_runtime::ensure" in app
assert "mirroleap-hq-core-progress" in runtime
assert 'MirroLeap.HQ.Core_v1_x64.zip' in workflow
assert 'Lightweight EXE exceeds 9 MB' in workflow
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

print("HQ v1: pinned scrcpy client marker, per-device opt-in token, "
      "bounded decoded frames, CPU/GDI circle and legacy fallback invariants passed "
      "(NOT GPU and NOT Android-device end-to-end validated)")
