"""Build the x64 SDL forwarding DLL for MirroLeap's pinned AirPlay runtime."""

import argparse
import hashlib
import os
from pathlib import Path
import re
import struct
import subprocess
import tempfile


PINNED_SDL_SHA256 = "d698174b2370588935acbe2847806cb340ef70597b009c490fde8c7b5efecad4"


def export_names(dll):
    pe, = struct.unpack_from("<I", dll, 0x3C)
    if dll[pe:pe + 4] != b"PE\0\0" or struct.unpack_from("<H", dll, pe + 4)[0] != 0x8664:
        raise ValueError("Expected a Windows x64 PE file")
    sections, = struct.unpack_from("<H", dll, pe + 6)
    optional_size, = struct.unpack_from("<H", dll, pe + 20)
    optional = pe + 24
    section_table = optional + optional_size
    mappings = [
        struct.unpack_from("<IIII", dll, section_table + 40 * section + 8)
        for section in range(sections)
    ]

    def offset(rva):
        for _, address, raw_size, raw_offset in mappings:
            if address <= rva < address + raw_size:
                return rva - address + raw_offset
        raise ValueError(f"Unmapped export RVA: {rva}")

    export_rva, = struct.unpack_from("<I", dll, optional + 112)
    functions, names, _, name_table, _ = struct.unpack_from("<IIIII", dll, offset(export_rva) + 20)
    if functions != names:
        raise ValueError("Expected all pinned SDL exports to have names")
    exports = []
    for name_rva, in struct.iter_unpack("<I", dll[offset(name_table):offset(name_table) + 4 * names]):
        start = offset(name_rva)
        name = dll[start:dll.index(b"\0", start)].decode("ascii")
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name):
            raise ValueError(f"Unexpected SDL export: {name}")
        exports.append(name)
    return exports


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("original_sdl", type=Path, help="SDL2.dll from AirPlay-Windows v0.4.1 x64")
    parser.add_argument("output", type=Path, help="Path to the forwarding DLL")
    parser.add_argument("--compiler", default=os.environ.get("CC", "cl.exe"))
    args = parser.parse_args()

    dll = args.original_sdl.read_bytes()
    if hashlib.sha256(dll).hexdigest() != PINNED_SDL_SHA256:
        parser.error("SDL2.dll does not match the pinned AirPlay-Windows release")
    names = export_names(dll)
    if "SDL_CreateWindow" not in names:
        parser.error("The pinned SDL does not export SDL_CreateWindow")
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    source = Path(__file__).resolve().with_name("airplay-sdl-background.c")
    with tempfile.TemporaryDirectory(prefix="mirroleap-sdl-") as temp:
        definition = Path(temp) / "exports.def"
        forwarded = [
            f"{name}=mirroleap_create_window" if name == "SDL_CreateWindow" else f"{name}=SDL2_original.{name}"
            for name in names
        ]
        definition.write_text("EXPORTS\n" + "\n".join(forwarded) + "\n", encoding="ascii")
        subprocess.run(
            [args.compiler, "/nologo", "/LD", "/O2", "/MT", str(source), "kernel32.lib",
             "/link", "/MACHINE:X64", f"/DEF:{definition}", f"/OUT:{output}"],
            cwd=temp, check=True,
        )
    print(output)


if __name__ == "__main__":
    main()
