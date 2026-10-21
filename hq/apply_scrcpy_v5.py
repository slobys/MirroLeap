#!/usr/bin/env python3
"""Opt-in decoded-frame sink patch for one PINNED scrcpy 5.0.1 source checkout.

This experimental source code retains the normal scrcpy 5.x D3D11VA decoder,
SDL rendering path and Android server. It adds a second frame sink which refs
GPU AVFrames on the decoder thread and only performs bounded, *full-frame*
GPU→CPU transfers on an independent worker while the HQ lens is active.

IMPORTANT: this is NOT zero-copy GPU magnification, a final HQ release, or
proof that real Android video is smooth. The next milestone is GPU ROI readback
and Windows/Android end-to-end latency/fps testing.

MirroLeap additions: MIT license; upstream scrcpy: Apache-2.0.
"""
from __future__ import annotations

import argparse
import shutil
import subprocess
from pathlib import Path

PINNED_COMMIT = "a60891aea193d92e7e5c3942700eca63f9d19a5f"
ROOT = Path(__file__).resolve().parent.parent
MODULES = (
    "mirroleap_hq_protocol.h",
    "mirroleap_hq_sink.h",
    "mirroleap_hq_sink.c",
    "mirroleap_hq_sink_v5.c",
)
SINK_MARKER = "MirroLeap HQ scrcpy5 raw-frame v3 (experimental)"
INCLUDE_ANCHOR = '#include "screen.h"'
STRUCT_ANCHOR = "    struct sc_decoder video_decoder;"
SINK_ANCHOR = "        sc_frame_source_add_sink(src, &s->screen.frame_sink);"
MESON_ANCHOR = "        'src/sys/win/process.c',"
VERSION_ANCHOR = "        scrcpy_print_version();"

INCLUDE_ADD = INCLUDE_ANCHOR + """
#ifdef _WIN32
# include "mirroleap_hq_sink.h"
#endif"""
STRUCT_ADD = STRUCT_ANCHOR + """
#ifdef _WIN32
    struct sc_mirroleap_hq_sink mirroleap_hq_sink;
#endif"""
SINK_ADD = SINK_ANCHOR + """
#ifdef _WIN32
        // Receives decoded v5 AVFrames (including D3D11VA texture references)
        // from the EXISTING decoder. Never starts another Android stream.
        if (getenv("MIRROLEAP_HQ_TOKEN")) {
            sc_mirroleap_hq_sink_init(&s->mirroleap_hq_sink);
            sc_frame_source_add_sink(&s->video_decoder.frame_source,
                                     &s->mirroleap_hq_sink.frame_sink);
        }
#endif"""
MESON_ADD = MESON_ANCHOR + """
        'src/mirroleap_hq_sink_v5.c',"""
VERSION_ADD = VERSION_ANCHOR + f'''
#ifdef _WIN32
        printf("{SINK_MARKER}\\n");
#endif'''


def run(*args: str, cwd: Path) -> str:
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def replace_once(source: str, anchor: str, replacement: str, label: str) -> str:
    if replacement in source:
        return source
    count = source.count(anchor)
    if count != 1:
        raise ValueError(f"{label}: expected unique scrcpy 5.0.1 anchor; found {count}")
    return source.replace(anchor, replacement)


def patch(checkout: Path) -> None:
    checkout = checkout.resolve(strict=True)
    if not (checkout / "app/src/scrcpy.c").is_file():
        raise ValueError("Expected a real scrcpy source checkout")
    actual = run("git", "rev-parse", "HEAD", cwd=checkout)
    if actual != PINNED_COMMIT:
        raise ValueError(f"Refusing unknown scrcpy revision {actual}")
    scrcpy = checkout / "app/src/scrcpy.c"
    meson = checkout / "app/meson.build"
    main = checkout / "app/src/main.c"
    s, m, v = scrcpy.read_text(), meson.read_text(), main.read_text()
    previously_patched = "mirroleap_hq_sink.h" in s
    if not previously_patched and run("git", "status", "--porcelain", cwd=checkout):
        raise ValueError("Pinned upstream checkout must be clean before first patch")

    new = {
        scrcpy: replace_once(
            replace_once(
                replace_once(s, INCLUDE_ANCHOR, INCLUDE_ADD, "header"),
                STRUCT_ANCHOR, STRUCT_ADD, "decoder state"),
            SINK_ANCHOR, SINK_ADD, "second decoded sink"),
        meson: replace_once(m, MESON_ANCHOR, MESON_ADD, "Meson source"),
        main: replace_once(v, VERSION_ANCHOR, VERSION_ADD, "version identity"),
    }
    for target, body in new.items():
        if target.read_text() != body:
            target.write_text(body)

    for module in MODULES:
        source = (ROOT / "hq/include" / module) if module.endswith("protocol.h") else (
            ROOT / "hq/scrcpy" / module
        )
        dest = checkout / "app/src" / module
        if dest.exists() and dest.read_bytes() != source.read_bytes():
            raise ValueError(f"Refusing inconsistent experimental module {module}")
        shutil.copyfile(source, dest)

    print("OK: pinned scrcpy 5.0.1 D3D11-capable decoded-frame sink staged")
    print("This is an experimental developer source patch, NOT a replacement for MirroLeap")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkout", type=Path, help="clean pinned scrcpy 5.0.1 checkout")
    patch(parser.parse_args().checkout)
