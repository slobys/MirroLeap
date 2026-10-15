#!/usr/bin/env python3
"""Offline contract check for Windows installer-recovery release artifacts.

This script checks that the *portable* build comes from a real, separately
compiled standalone Rust EXE, not a renamed NSIS/MSI installer; that SHA256 is
created after optional signing, and that a release cannot silently omit its ZIP.
Windows Actions additionally verify all ZIP members and release file hashes.
"""
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1]
config = json.loads((root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
pkg = json.loads((root / "package.json").read_text(encoding="utf-8"))
assert config["version"] == pkg["version"] == "4.5.1"
assert config["identifier"] == "com.kb.scrcpy-gui-v3", (
    "Changing the bundle identifier would silently break upgrade compatibility"
)
ci = (root / ".github/workflows/desktop-ci.yml").read_text(encoding="utf-8")
stage = (root / "scripts/windows-stage-portable.ps1").read_text(encoding="utf-8")
sign = (root / "scripts/windows-sign-and-checksum.ps1").read_text(encoding="utf-8")
docs = (root / "INSTALL_RECOVERY_CN.md").read_text(encoding="utf-8")
must_have = [
    "npm run tauri build -- --no-bundle",
    "scripts/windows-stage-portable.ps1",
    "npm run tauri build",
    "scripts/windows-sign-and-checksum.ps1",
    "ScrcpyGUI_4.5.1",
    "*_x64_Portable.zip",
    "release-checksums/SHA256SUMS.txt",
    "sha256sum --check SHA256SUMS.txt",
]
for text in must_have:
    assert text in ci, f"Release workflow missing portable requirement: {text}"
steps = [
    ci.index("npm run tauri build -- --no-bundle"),
    ci.index("scripts/windows-stage-portable.ps1"),
    ci.index("run: npm run tauri build", ci.index("scripts/windows-stage-portable.ps1")),
    ci.index("scripts/windows-sign-and-checksum.ps1"),
]
assert steps == sorted(steps), "Portable EXE must be staged before installer patches the binary"
assert "src-tauri\\target\\release\\scrcpy-gui-v3.exe" in stage
assert "Run-ScrcpyGUI.cmd" in stage and "README-PORTABLE.txt" in stage
assert "Compress-Archive -Path $portableStage" in sign
assert "SCRCPY_SIGNING_PFX_BASE64" in sign
assert "SCRCPY_SIGNING_PFX_PASSWORD" in sign
assert "foreach ($asset in $signTargets)" in sign
assert "Get-FileHash" in sign and "$releaseFiles" in sign
assert "Expected EXE and MSI installers" in sign
assert "$entries.Count -ne 3" in sign
assert "Portable.zip" in docs and "Do not uninstall" in docs
assert "/NCRC" in docs and "不要" in docs
assert "disable Windows Defender" not in ci
print("Portable recovery build, signature order and 3-file SHA256 manifest contracts pass")
