#!/usr/bin/env python3
"""MirroLeap v4.8.0: guard wireless hub UI and native safety invariants."""
from pathlib import Path

root = Path(__file__).resolve().parents[1]
ui = (root / "src/components/Sidebar.tsx").read_text(encoding="utf-8")
status = (root / "src/components/WirelessConnectionStatus.tsx").read_text(encoding="utf-8")
workflow = (root / ".github/workflows/desktop-ci.yml").read_text(encoding="utf-8")
rust = (root / "src-tauri/src/commands.rs").read_text(encoding="utf-8")
hooks = (root / "src/hooks/useScrcpy.ts").read_text(encoding="utf-8")
util = (root / "src/utils/wirelessCenter.ts").read_text(encoding="utf-8")

for requirement in (
    "normalizeWirelessEndpoint(input: string)", "safeSameHostAlternatives",
    "mdnsServiceKind", "historicWirelessStatus",
):
    assert requirement in util, f"Missing endpoint safety helper: {requirement}"
for requirement in (
    "operationRef.current", "generationRef.current", "onDiagnose(address)",
    "onSelectPort={updateIp}", "handleUsbToWireless(serial)",
    "mdnsServiceKind(dev.service)", "historicWirelessStatus(ip, devices, pendingDevices)",
):
    assert requirement in ui, f"Missing wireless UI operation check: {requirement}"
assert 'result.diagnosis' in ui and 'diagnosis: report' in hooks
assert "t('sidebar.diagnostics.' + current)" in status
assert "safeDiagnosisCode(feedback.diagnosis)" in status
assert "safeSameHostAlternatives(feedback.diagnosis, feedback.address)" in status
assert "parse_diagnostic_endpoint(&ip)" in rust
assert "require_main_window(&window)" in rust
assert "fn checked_pairing_inputs(" in rust
assert "Duration::from_secs(20)" in rust
assert "scripts/check_wireless_center.py" in workflow

for lang in ("en", "zh-CN"):
    locale = (root / "src/i18n/locales" / f"{lang}.ts").read_text(encoding="utf-8")
    assert "wirelessCenter: {" in locale, lang
assert (root / "src/utils/wirelessCenter.test.ts").exists()
print("Wireless state, same-IP discovery, native argv safety, locales and CI contracts passed")
