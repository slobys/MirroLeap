"""Build the bulk read adapter for the pinned MirroLeap USB runtime."""

import argparse
import hashlib
from pathlib import Path
import runpy
import subprocess
import tempfile


PINNED_LIBUSB_SHA256 = "4f18b5d2c28aa66b648c8683c6d09b52b92cbbee85984bbefad5f38a64bc2a14"
WRAPPED = {"usb_bulk_read": "mirroleap_bulk_read", "usb_close": "mirroleap_usb_close"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("payload", type=Path)
    parser.add_argument("libusb_source", type=Path, help="Pinned libusb-win32 user-mode source directory")
    parser.add_argument("--compiler", required=True)
    args = parser.parse_args()

    payload = args.payload.resolve(strict=True)
    original_path = payload / "libusb0.dll"
    original = original_path.read_bytes()
    if hashlib.sha256(original).hexdigest() != PINNED_LIBUSB_SHA256:
        parser.error("Unexpected USB Mirror libusb0.dll checksum")
    scripts = Path(__file__).resolve().parent
    libusb = args.libusb_source.resolve(strict=True)
    export_names = runpy.run_path(str(scripts / "build-airplay-sdl-background.py"))["export_names"]
    names = export_names(original)
    if not WRAPPED.keys() <= set(names):
        parser.error("Missing required libusb0 exports")

    with tempfile.TemporaryDirectory(prefix="mirroleap-libusb-") as temp:
        definition = Path(temp) / "exports.def"
        lines = [f"{name}={WRAPPED.get(name, f'libusb0_original.{name}')}" for name in names]
        definition.write_text("EXPORTS\n" + "\n".join(lines) + "\n", encoding="ascii")
        adapter = Path(temp) / "libusb0.dll"
        compiler_options = ["-fuse-ld=lld"] if "clang-cl" in args.compiler.lower() else []
        compile_c = [args.compiler, *compiler_options, "/nologo", "/O2", "/MT", f"/I{libusb}"]
        subprocess.run(
            [*compile_c, "/LD", str(scripts / "ios-libusb-read.c"), "kernel32.lib", "/link", "/MACHINE:X64",
             f"/DEF:{definition}", f"/OUT:{adapter}"],
            cwd=temp, check=True,
        )
        test = Path(temp) / "usb-read-test.exe"
        subprocess.run(
            [*compile_c, str(scripts / "ios-libusb-read-test.c"), "kernel32.lib", "/link",
             "/MACHINE:X64", f"/OUT:{test}"],
            cwd=temp, check=True,
        )
        subprocess.run([str(test)], cwd=temp, check=True, timeout=15)
        built = adapter.read_bytes()
        if set(export_names(built)) != set(names):
            raise ValueError("The libusb0 adapter does not match the original export table")
        (payload / "libusb0_original.dll").write_bytes(original)
        original_path.write_bytes(built)


if __name__ == "__main__":
    main()
