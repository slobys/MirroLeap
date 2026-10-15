#!/usr/bin/env python3
"""Cheap cross-platform checks for high-risk Tauri release defaults."""
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1]
config = json.loads((root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
caps = {
    p.stem: json.loads(p.read_text(encoding="utf-8"))
    for p in (root / "src-tauri/capabilities").glob("*.json")
}
csp = config["app"]["security"]["csp"]
assert isinstance(csp, str) and "default-src 'self'" in csp
for critical in ("object-src 'none'", "frame-src 'none'", "connect-src", "script-src"):
    assert critical in csp, f"Missing CSP restriction: {critical}"
assert "'unsafe-eval'" not in csp
assert not any('https://fonts.googleapis.com' in line for line in
               (root / "index.html").read_text().splitlines())
assert caps["default"]["windows"] == ["main"]
assert not any("shell" in permission for permission in caps["default"]["permissions"])
for name in ("controls", "splash", "frame-ack"):
    assert not any("shell" in permission or "dialog" in permission
                   for permission in caps[name]["permissions"]), name
assert caps["controls"]["windows"] == ["phone-controls-*"]
assert caps["splash"]["windows"] == ["splashscreen"]
native = (root / "src-tauri/src/commands.rs").read_text(encoding="utf-8")
for requirement in (
    "validated_report_filename(&name)?",
    "OpenOptions::new().write(true).create_new(true)",
    "window.label() != \"main\"",
    "parse_terminal_input(&cmd)",
    "kill_on_drop(true)",
    "Duration::from_secs(20)",
    "capped_console_output(&o.stdout)",
    "stop_scrcpy_impl(state, device).await",
    "require_main_window(&window)?",
):
    assert requirement in native, f"Missing native protection: {requirement}"
diagnostic = (root / "src/utils/diagnostics.ts").read_text(encoding="utf-8")
assert "safeDiagnosticSettings" in diagnostic and "redactDiagnosticText" in diagnostic
assert "localStorage:" not in (root / "src/components/ErrorBoundary.tsx").read_text()
assert "localStorage:" not in (root / "src/components/LogPanel.tsx").read_text()
assert "场景预设" in (root / "README.md").read_text()
print("Security: CSP, scoped Tauri capabilities, safe reports, bounded terminal and diagnostics passed")
