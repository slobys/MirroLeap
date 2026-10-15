#!/usr/bin/env python3
"""Keep the independent MirroLeap repository and release links in sync.

This checks project code only: GitHub branch/Release binary parity is verified
separately during the one-time migration and release publishing.
"""
import json
import tomllib
from pathlib import Path

root = Path(__file__).resolve().parents[1]
url = "https://github.com/slobys/MirroLeap"
readme = (root / "README.md").read_text(encoding="utf-8")
guide = (root / "GUIDE.md").read_text(encoding="utf-8")
install = (root / "INSTALL_RECOVERY_CN.md").read_text(encoding="utf-8")
footer = (root / "src/components/Footer.tsx").read_text(encoding="utf-8")
flake = (root / "flake.nix").read_text(encoding="utf-8")
workflow = (root / ".github/workflows/desktop-ci.yml").read_text(encoding="utf-8")
bump = (root / ".github/workflows/bump_version.yml").read_text(encoding="utf-8")
config = json.loads((root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
package = json.loads((root / "package.json").read_text(encoding="utf-8"))
crate = tomllib.loads((root / "src-tauri/Cargo.toml").read_text(encoding="utf-8"))

assert f"{url}/releases/latest" in readme, "README points at the wrong Latest"
assert f"{url}/releases/latest" in install or f"{url}/releases" in install
assert f"{url}/releases" in guide and f"{url}/issues" in guide
assert url in footer and url in flake
assert "https://github.com/kil0bit-kb/scrcpy-gui" in readme, (
    "Retain attribution to the original project"
)
assert "Genymobile/scrcpy" in readme
assert (root / "MIGRATION_CN.md").is_file()
assert "slobys/scrcpy-gui" in (root / "MIGRATION_CN.md").read_text(encoding="utf-8")
assert config["productName"] == "MirroLeap"
assert config["identifier"] == "com.kb.scrcpy-gui-v3", "Preserve user app data"
assert crate["package"]["name"] == package["name"] == "mirroleap"
assert crate["package"]["version"] == package["version"] == config["version"]
assert f"MirroLeap_{package['version']}_x64_Portable.zip" in readme
assert "gh release create" in workflow and "--notes-file release-notes.md --latest" in workflow
assert "[publish release]" in workflow and "[publish release]" in bump
assert "secrets.RELEASE_TOKEN" not in bump, "The new repo has no old PAT secret"
assert not (root / ".github/workflows/tag_on_merge.yml").exists(), (
    "Do not create tag-only versions separate from full Releases"
)
print("Independent MirroLeap URLs, attribution, versioning and full Release workflow verified")
