#!/usr/bin/env python3
"""Fail-closed guards for the isolated scrcpy 5.0.1 hardware-aware HQ lab.

Source checks and Linux stub tests do NOT prove actual hardware decode or
Android stream latency. Windows CI additionally builds the native patched EXE.
"""
from pathlib import Path
import re

ROOT=Path(__file__).resolve().parents[1]
def read(p:str)->str:
    return (ROOT/p).read_text(encoding="utf-8")

patch=read("hq/apply_scrcpy_v5.py")
producer=read("hq/scrcpy/mirroleap_hq_sink.c")
wrapper=read("hq/scrcpy/mirroleap_hq_sink_v5.c")
header=read("hq/scrcpy/mirroleap_hq_sink.h")
workflow=read(".github/workflows/hq-scrcpy5-lab.yml")
assert 'a60891aea193d92e7e5c3942700eca63f9d19a5f' in patch
assert 'scrcpy 5.0.1' in patch
assert 'sc_frame_source_add_sink(&s->video_decoder.frame_source' in patch
assert 'MIRROLEAP_HQ_TOKEN' in patch
assert 'SINK_MARKER' in patch and 'HQ scrcpy5 raw-frame v3 (experimental)' in patch
assert 'mirroleap_hq_sink_v5.c' in patch
assert '#define MLHQ_ENABLE_D3D11 1' in wrapper
# scrcpy.c allocates without MLHQ_ENABLE_D3D11, sink_v5.c uses it.
# Conditional struct fields would overwrite adjacent scrcpy state.
assert '#ifdef MLHQ_ENABLE_D3D11' not in header
assert 'test_sink_layout.c' in workflow
for field in ('last_enqueued_at', 'gpu_interval_ms', 'gpu_slow_count', 'gpu_readback_disabled'):
    assert field in header
assert 'av_hwframe_transfer_data(cpu, frame, 0)' in producer
assert 'av_frame_clone(frame)' in producer
assert 'AV_PIX_FMT_D3D11' in producer
assert 'MLHQ_PUSH_RESULT enum sc_sink_result' in producer
assert 'MLHQ_PUSH_OK SC_SINK_OK' in producer
assert 'if (!sink->worker || !mlhq_consumer_active(sink))' in producer
assert 'last_enqueued_at' in header
assert 'if (last && (DWORD)(now-last)<interval)' in producer
assert 'mlhq_gpu_capture_interval_ms(readback_ms)' in producer
assert 'gpu_interval_ms' in producer and 'gpu_interval_ms' in header
assert 'mlhq_gpu_capture_interval_ms(0)==80' in read('hq/tests/test_sink_v5.c')
assert '-Dcompile_server=false' in workflow
assert 'NOT release' in workflow
assert 'scrcpy-win64-v5.0.1.zip' in workflow
assert 'b12a2c4ee8be317422451fc7dcf8ee20a71b5ea7ef9ad73ddd825a25316227a5' in workflow
assert 'actions/upload-artifact' in workflow
assert 'MLHQ_G_GPU_READBACK_SLOW' in read("hq/include/mirroleap_hq_protocol.h")
assert 'gpu_readback_disabled' in producer and 'MLHQ_G_GPU_READBACK_SLOW' in producer
assert 'PRODUCER_HEALTH_OFFSET' in read("src-tauri/src/hq_bridge_windows.rs")
assert 'MirroLeap.HQ.Core_v3_x64.zip' in read(".github/workflows/mirroleap-continuity.yml")
# Original user application MUST NOT be affected by a lab-only branch.
conf=read("src-tauri/tauri.conf.json")
command=read("src-tauri/src/commands.rs")
assert '"version": "5.3.7"' in conf
assert 'const HQ_VERSION_MARKER:&str="MirroLeap HQ scrcpy5 raw-frame v3 (experimental)";' in command
assert 'experimental_hq_selected' in command
print("PASS: pinned scrcpy5 lab remains isolated, D3D11 refs bounded and v5 push result truthful")
