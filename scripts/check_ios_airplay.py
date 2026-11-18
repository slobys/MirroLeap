#!/usr/bin/env python3
"""AirPlay optional-engine security and UX wiring regression checks."""
from pathlib import Path
root=Path(__file__).resolve().parents[1]
native=(root/"src-tauri/src/ios_airplay.rs").read_text(encoding="utf-8")
lib=(root/"src-tauri/src/lib.rs").read_text(encoding="utf-8")
home=(root/"src/components/CompactDashboard.tsx").read_text(encoding="utf-8")
ui=(root/"src/components/IosReceiverPanel.tsx").read_text(encoding="utf-8")
workflow=(root/".github/workflows/desktop-ci.yml").read_text(encoding="utf-8")
release=(root/".github/workflows/mirroleap-continuity.yml").read_text(encoding="utf-8")
doc=(root/"IOS_AIRPLAY_CN.md").read_text(encoding="utf-8")

pin="5f1792a96efffe84b220edfa7286fa7670806226aa4d0b4b6c7e33ddd7747bf9"
assert pin in native and pin in doc
assert "https://github.com/moieric11/AirPlay-Windows/releases/download/v0.4.1/airplay-windows-v0.4.1-x64.zip" in native
assert 'format!("{:x}",Sha256::digest(bytes))' in native
assert 'if digest!=ENGINE_ARCHIVE_SHA256' in native
assert "safe_zip_member" in native and "segment != \"..\"" in native
assert "item.unix_mode()" in native and "180_000_000" in native
assert "ENGINE_RELEASE" in native and "reqwest::Client::builder()" in native
assert 'root.join("LICENSE").is_file()' in native
assert 'root.join("THIRD_PARTY_NOTICES.md").is_file()' in native
assert "only after a user click" in native and "start" in native
assert "Only the main MirroLeap window" in native
assert "running: running(state)?" in native
assert "status_for(&app,&state)" in native
assert "app.manage(ios_airplay::IosAirplayState::default())" in lib
for command in ("ios_airplay_status","ios_airplay_install","ios_airplay_start",
                "ios_airplay_stop","ios_airplay_open_source"):
    assert "ios_airplay::"+command in lib
    if command in ('ios_airplay_status','ios_airplay_open_source'):
        assert command in ui
assert "'ios_airplay_'+action" in ui
assert "'install'|'start'|'stop'" in ui
assert "IosReceiverPanel" in home and "setIosPanelOpen(true)" in home
assert "屏幕镜像" in ui and "专用网络" in ui
assert "接收器进程启动并不代表 iPhone 已连接" in doc
assert "GPL-3.0" in ui and "DRM" in ui
assert "scripts/check_ios_airplay.py" in workflow
assert "scripts/check_ios_airplay.py" in release
assert "'ios_airplay::tests'" in release
assert '"version": "5.4.1"' in (root/"package.json").read_text()
assert '"version": "5.4.1"' in (root/"src-tauri/tauri.conf.json").read_text()
assert "ios_airplay::ios_airplay_set_compact_mode" in lib
assert "ios_airplay::ios_airplay_window_action" in lib
assert "configure_receiver_window" in native and "update_receiver_ini" in native
assert "find_window_by_pid" in native and "PostMessageW(Some(hwnd)" in native
assert "chromeless" in native and "简洁投屏" in ui and "compactMode" in ui
assert "ios_airplay_set_compact_mode" in ui and "ios_airplay_window_action" in ui
assert "ios_airplay::tests" in workflow
print("iOS AirPlay v5.4.1: compact screen-only view, bounded keyboard actions and original GPL receiver checks passed")
