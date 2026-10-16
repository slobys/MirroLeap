#!/usr/bin/env python3
"""Fail-closed validation of a published Windows x64 Tauri updater feed.

This is an artifact/manifest consistency gate. Rust's updater plugin is the
cryptographic Ed25519 verifier at install time; this script does NOT claim to
replace it with SHA-256.

Usage:
    python scripts/check_signed_updater_release.py <manifest_folder> <bundle_or_asset_folder>
"""
from __future__ import annotations

from datetime import datetime
from hashlib import sha256
from pathlib import Path
import json
import re
import sys

ROOT = Path(__file__).resolve().parents[1]


def unique_file(root: Path, name: str) -> Path:
    matches = [p for p in root.rglob(name) if p.is_file() and p.name == name]
    if len(matches) != 1:
        raise AssertionError(f"Exactly one {name} required beneath {root}, got {len(matches)}")
    return matches[0]


def check_release(manifests: Path, assets: Path, version: str) -> None:
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise AssertionError(f"Invalid version {version!r}")
    nsis_name = f"MirroLeap_{version}_x64-setup.exe"
    msi_name = f"MirroLeap_{version}_x64_en-US.msi"
    portable_name = f"MirroLeap_{version}_x64_Portable.zip"
    required = [nsis_name, msi_name, portable_name]
    feed_path = unique_file(manifests, "latest.json")
    feed = json.loads(feed_path.read_text(encoding="utf-8-sig"))
    assert isinstance(feed, dict), "Updater manifest must be an object"
    assert feed.get("version") == version
    assert set(feed.keys()).issubset({"version", "pub_date", "notes", "platforms"})
    assert isinstance(feed.get("notes"), str) and len(feed["notes"]) <= 4096
    if feed.get("pub_date"):
        assert isinstance(feed["pub_date"], str)
        datetime.fromisoformat(feed["pub_date"].replace("Z", "+00:00"))
    platforms = feed.get("platforms")
    assert isinstance(platforms, dict) and set(platforms) == {"windows-x86_64"}
    entry = platforms["windows-x86_64"]
    assert isinstance(entry, dict) and set(entry) == {"url", "signature"}
    expected = f"https://github.com/slobys/MirroLeap/releases/download/v{version}/{nsis_name}"
    assert entry["url"] == expected, "Updater URL must point at this repository's exact tagged NSIS"
    assert isinstance(entry["signature"], str)
    assert 60 <= len(entry["signature"]) <= 1024
    assert "\x00" not in entry["signature"]
    assert entry["signature"] == unique_file(assets, nsis_name + ".sig").read_text(
        encoding="utf-8"
    ).strip(), "Signed updater index must match the actual signed installer"
    assert 60 <= unique_file(assets, msi_name + ".sig").stat().st_size <= 1024

    # The release's traditional SHA256SUMS protects consumers downloading
    # directly; it must be generated after Authenticode and before publishing.
    entries: dict[str, str] = {}
    checksum_file = unique_file(manifests, "SHA256SUMS.txt")
    for line in checksum_file.read_text(encoding="utf-8-sig").splitlines():
        match = re.fullmatch(r"([a-fA-F0-9]{64})  ([A-Za-z0-9_.-]+)", line)
        assert match is not None, "Checksum line is malformed"
        digest, filename = match.groups()
        assert filename not in entries, "Duplicate checksum record"
        entries[filename] = digest.lower()
    assert set(entries) == set(required), "Unexpected release artifacts in checksum manifest"
    for name in required:
        file = unique_file(assets, name)
        assert sha256(file.read_bytes()).hexdigest() == entries[name], f"SHA mismatch for {name}"
        assert file.stat().st_size >= 250_000, f"Unusually small artifact: {name}"
    print(f"PASS v{version} stable updater: canonical NSIS URL, exact .sig, release manifest, 3 SHA256")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("Usage: check_signed_updater_release.py manifest_folder asset_folder")
    version = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["version"]
    check_release(Path(sys.argv[1]), Path(sys.argv[2]), version)
