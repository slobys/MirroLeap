"""Apply the QuickTime stream fix and rebuild the pinned Windows capture core."""

import argparse
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--compiler", required=True)
    args = parser.parse_args()

    source = args.source.resolve(strict=True)
    output = args.output.resolve()
    scripts = Path(__file__).resolve().parent
    is_clang = "clang-cl" in args.compiler.lower()

    with tempfile.TemporaryDirectory(prefix="mirroleap-capture-core-") as work:
        work = Path(work)
        staged = work / "source"
        shutil.copytree(source / "src" / "Core", staged / "src" / "Core")
        subprocess.run(["git", "apply", str(scripts / "ios-quicktime-video-recovery.patch")],
                       cwd=staged, check=True)
        core = staged / "src" / "Core"
        cmake = (core / "CMakeLists.txt").read_text(encoding="utf-8")
        sources = re.search(r"set\(IPHONEMIRROR_CORE_SOURCES\s+(.*?)\)", cmake, re.S).group(1).split()
        sdk_libraries = re.search(
            r"target_link_libraries\(iPhoneMirror\.Core PRIVATE\s+(.*?)\)", cmake, re.S
        ).group(1).replace("${PROJECT_SOURCE_DIR}", str(source)).splitlines()
        libraries = [entry.strip().strip('"') for entry in sdk_libraries if entry.strip()]
        libraries = [entry if entry.endswith(".lib") else f"{entry}.lib" for entry in libraries]
        libraries.extend(["user32.lib", "oleaut32.lib"])
        includes = [
            core / "include", core / "src", source / "src" / "WirelessHost",
            source / "third_party" / "libusb" / "include",
            source / "third_party" / "libusb-win32" / "include",
        ]
        compiler = [args.compiler, "/nologo", "/std:c++20", "/O2", "/MT", "/utf-8", "/EHsc",
                    "/DUNICODE", "/D_UNICODE", "/DWIN32_LEAN_AND_MEAN", "/DNOMINMAX",
                    *[f"/I{path}" for path in includes]]

        def compile_source(src, export_api=False):
            obj = work / f"{src.stem}.obj"
            if src.suffix == ".rc":
                obj = work / f"{src.stem}.res"
                rc = "llvm-rc.exe" if is_clang else "rc.exe"
                subprocess.run([rc, "/nologo", f"/fo{obj}", str(src)], cwd=work, check=True)
            else:
                define = ["/DIPHONEMIRROR_CORE_EXPORTS"] if export_api else []
                subprocess.run([*compiler, *define, "/c", f"/Fo{obj}", str(src)], cwd=work, check=True)
            return obj

        with ThreadPoolExecutor(max_workers=4) as workers:
            objects = list(workers.map(lambda name: compile_source(core / name, True), sources))

        linker = "lld-link.exe" if is_clang else "link.exe"
        core_dll = work / output.name
        subprocess.run([linker, "/nologo", "/dll", "/MACHINE:X64", "/DELAYLOAD:libusb0.dll",
                        f"/OUT:{core_dll}", *map(str, objects), *libraries], cwd=work, check=True)

        test = work / "ios-video-recovery-test.exe"
        test_object = compile_source(scripts / "ios-video-recovery-test.cpp")
        protocol_names = {"CoreMedia", "H264", "Plist", "QuickTimePacket", "QuickTimeSession"}
        test_objects = [test_object, *(obj for obj in objects if obj.stem in protocol_names)]
        subprocess.run([linker, "/nologo", "/MACHINE:X64", f"/OUT:{test}", *map(str, test_objects)],
                       cwd=work, check=True)
        fixture = core / "tests" / "fixtures" / "quicktime_video_hack"
        subprocess.run([str(test), str(fixture)], cwd=work, check=True, timeout=15)
        shutil.copyfile(core_dll, output)


if __name__ == "__main__":
    main()
