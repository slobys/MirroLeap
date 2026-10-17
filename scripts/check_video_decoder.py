#!/usr/bin/env python3
"""Cross-layer regression contract for safe scrcpy v5 hardware decoding.

This is NOT a GPU benchmark. Actual driver compatibility and latency require
Windows GPU hardware and an Android device, beyond GitHub Windows UI CI.
"""
from pathlib import Path
import json

root = Path(__file__).resolve().parents[1]
native = (root / "src-tauri/src/commands.rs").read_text(encoding="utf-8")
decoder = (root / "src-tauri/src/video_decoder.rs").read_text(encoding="utf-8")
backend = (root / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
hook = (root / "src/hooks/useScrcpy.ts").read_text(encoding="utf-8")
view = (root / "src/components/DecoderPerformance.tsx").read_text(encoding="utf-8")
app = (root / "src/App.tsx").read_text(encoding="utf-8")
cfg = json.loads((root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
package = json.loads((root / "package.json").read_text(encoding="utf-8"))

assert package["version"] == cfg["version"]
assert cfg["identifier"] == "com.kb.scrcpy-gui-v3"
assert 'commands::get_video_decoder_support' in backend
assert 'require_main_window(&window)?;' in native
assert "probe_video_decoder_support(&exe_path).await" in native
assert "Duration::from_secs(5)" in native
assert "--version" in native and "--help" in native
assert "build_scrcpy_args_with_decoder(" in native
assert "video_decoder::choose_decoder(" in native
assert "video_decoder::is_hardware_decode_error" in native
assert "forced_decoder_retry_used = true" in native
assert native.count("restore_phone_frame_after_retry(") >= 4, (
    "GPU/audio decoder fallback must reattach the native borderless video frame"
)
assert 'if wants_device_frame(config) {' in native
assert 'if config.borderless.unwrap_or(false) || wants_device_frame(config)' not in native
assert "current_decoder_decision.flag = None" in native
assert "scrcpy-video-decoder" in native
assert "is_v5 && advertised" in decoder
assert "None = do not pass an --hwdec option" in decoder
assert "is_hardware_decode_error(" in decoder
assert "observed_decoder(" in decoder
assert "['auto', 'disabled']" not in decoder  # Rust module, not a JS config literal
assert 'hardwareDecoder?:' in hook and 'printFps?: boolean' in hook
assert "get_video_decoder_support" in hook and "scrcpy-video-decoder" in hook
assert "observedDecoders[activeDevice]" in app
assert "<DecoderPerformance " in app
assert "GPU selection never upgrades your core" in view
assert (root / "src/components/__tests__/DecoderPerformance.test.tsx").exists()
print("scrcpy decoder fallback reattaches native phone frame, safe flags and telemetry verified")
