#!/usr/bin/env python3
"""Opt-in patch of ONE pinned Genymobile/scrcpy v4.0 client checkout.

No core files are changed inside the MirroLeap runtime or downloaded assets.
The official Android server and all normal CLI operation stay unchanged.
License: MIT (these additions); upstream scrcpy remains Apache-2.0.
"""
from __future__ import annotations

import argparse
import shutil
import subprocess
from pathlib import Path

PINNED_COMMIT = "2322868e9e256eb5fce0b3d659ab2a409f29bae1"
ROOT = Path(__file__).resolve().parent.parent
MODULES = [
    "mirroleap_hq_protocol.h",
    "mirroleap_hq_sink.h",
    "mirroleap_hq_sink.c",
]
START_INCLUDE = '#include "screen.h"'
START_STRUCT = "    struct sc_decoder video_decoder;"
START_SINK = "            sc_frame_source_add_sink(src, &s->screen.frame_sink);"
START_MESON = "        'src/sys/win/process.c',"
START_VERSION = "        scrcpy_print_version();"
VERSION_ADD = START_VERSION + '''
#ifdef _WIN32
        printf("MirroLeap HQ decoded-frame v1 (experimental)\\n");
#endif'''

INCLUDE_ADD = START_INCLUDE + """
#ifdef _WIN32
# include "mirroleap_hq_sink.h"
#endif"""
STRUCT_ADD = START_STRUCT + """
#ifdef _WIN32
    struct sc_mirroleap_hq_sink mirroleap_hq_sink;
#endif"""
SINK_ADD = START_SINK + """
#ifdef _WIN32
            // Adds the second decoded-frame sink BEFORE SDL display scaling.
            // Only when a parent opt-in owner exists. No second video stream.
            if (getenv("MIRROLEAP_HQ_TOKEN")) {
                sc_mirroleap_hq_sink_init(&s->mirroleap_hq_sink);
                sc_frame_source_add_sink(&s->video_decoder.frame_source,
                                      &s->mirroleap_hq_sink.frame_sink);
            }
#endif"""
MESON_ADD = START_MESON + "\n        'src/mirroleap_hq_sink.c',"


def command(*args: str, cwd: Path) -> str:
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def add_once(text: str, anchor: str, replacement: str, label: str) -> str:
    if replacement in text:
        return text
    if text.count(anchor) != 1:
        raise ValueError(f"{label}: pinned upstream anchor changed or ambiguous")
    return text.replace(anchor, replacement)


def patch(checkout: Path) -> None:
    checkout = checkout.resolve(strict=True)
    if not checkout.is_dir() or not (checkout / "app/src/scrcpy.c").is_file():
        raise ValueError("Provide a real scrcpy source checkout, not an archive or binary")
    head = command("git", "rev-parse", "HEAD", cwd=checkout)
    if head != PINNED_COMMIT:
        raise ValueError(f"Unsupported scrcpy source commit {head}; only v4.0 is pinned")
    scrcpy_path = checkout / "app/src/scrcpy.c"
    meson_path = checkout / "app/meson.build"
    main_path = checkout / "app/src/main.c"
    scrcpy_src = scrcpy_path.read_text()
    meson_src = meson_path.read_text()
    main_src = main_path.read_text()
    previously_patched = "mirroleap_hq_sink.h" in scrcpy_src
    if not previously_patched and command("git", "status", "--porcelain", cwd=checkout):
        raise ValueError("scrcpy checkout must be clean before first patch")

    expected = {
        scrcpy_path: add_once(
            add_once(
                add_once(scrcpy_src, START_INCLUDE, INCLUDE_ADD, "include"),
                START_STRUCT, STRUCT_ADD, "struct"),
            START_SINK, SINK_ADD, "frame sink"),
        meson_path: add_once(meson_src, START_MESON, MESON_ADD, "Meson Windows files"),
        main_path: add_once(main_src, START_VERSION, VERSION_ADD, "HQ version marker"),
    }
    for dst, result in expected.items():
        if dst.read_text() != result:
            dst.write_text(result)
    for name in MODULES:
        template = (ROOT / "hq/include" / name) if name.endswith("protocol.h") else (
            ROOT / "hq/scrcpy" / name
        )
        destination = checkout / "app/src" / name
        if destination.exists() and destination.read_bytes() != template.read_bytes():
            raise ValueError(f"Experimental module was edited outside the patch: {name}")
        shutil.copyfile(template, destination)

    print("OK scrcpy v4.0 HQ decoded-frame sink staged. "
          "This is source only; build/testing requires an FFmpeg/SDL3 Windows toolchain.")


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("checkout", type=Path, help="pinned scrcpy v4.0 source checkout")
    options = cli.parse_args()
    patch(options.checkout)
