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
assert "let is_running=running(app,state)?" in native and "running: is_running" in native
assert "status_for(&app,&state)" in native
assert "app.manage(ios_airplay::IosAirplayState::default())" in lib
for command in ("ios_airplay_status","ios_airplay_install","ios_airplay_start",
                "ios_airplay_stop","ios_airplay_open_source"):
    assert "ios_airplay::"+command in lib
    if command in ('ios_airplay_status','ios_airplay_open_source'):
        assert command in ui
assert "'ios_airplay_'+action" in ui
assert "'install'|'start'|'stop'" in ui
assert "IosReceiverPanel" in home and "setIosPanelOpen(true)" not in home
assert 'data-testid="ios-home-panel"' in ui and 'role="dialog"' not in ui
assert "屏幕镜像" in ui and "专用网络" in ui
assert "接收器进程启动并不代表 iPhone 已连接" in doc
assert "GPL-3.0" in ui and "DRM" in ui
assert "scripts/check_ios_airplay.py" in workflow
assert "scripts/check_ios_airplay.py" in release
assert "'ios_airplay::tests'" in release
assert '"version": "5.4.1"' in (root/"package.json").read_text()
assert '"version": "5.4.1"' in (root/"src-tauri/tauri.conf.json").read_text()
assert "ios_airplay::ios_airplay_set_compact_mode" in lib
assert "ios_airplay::ios_airplay_set_window_style" in lib
assert "ios_airplay::ios_airplay_set_quality_tier" in lib
assert "ios_airplay::ios_airplay_window_action" in lib
assert "configure_receiver_window" in native and "update_receiver_ini" in native
assert "find_window_by_pid" in native and "PostMessageW(Some(hwnd)" in native
assert "chromeless" in native and "纯画面" in ui and "compactMode" in ui
assert "ios_airplay_set_window_style" in ui and "ios_airplay_set_quality_tier" in ui
assert "ios_airplay_window_action" in ui
assert home.index('className="ml-dashboard-grid') < home.index('<IosReceiverPanel locale={locale}/>') < home.index('className="ml-home-footer')
assert 'data-testid="ios-after-android"' in home
assert "quality_cap" in native and "2868" in native and "3840" in native
assert "ios_device_frame::start" in native and "ios_device_frame::close" in native
assert "mod ios_device_frame;" in lib
frame=(root/"src-tauri/src/ios_device_frame.rs").read_text(encoding="utf-8")
assert "CreateWindowExW" in frame and "UpdateLayeredWindow" in frame
assert "device-auto" in ui
assert "GetClientRect" in frame and "ClientToScreen" in frame
assert "render_bezel" in frame and "Surface::create()" in frame
assert "SetParent(" not in frame and "WS_CHILD" not in frame
assert "WebviewWindowBuilder" not in frame and "get_webview_window" not in frame
assert "target_layout" in frame and "place_ios_companion(" in frame
assert "active_for" in frame and "frame_error" in frame
assert "BEZEL_CLIP_OVERSCAN_PX" in frame and "seam_cover_px" in frame
assert "SetWinEventHook" in frame and "UnhookWinEvent" in frame
assert "moved_video_target" in frame and "on_video_location_change" in frame
assert "frame_attached" in native and "frame_attached" in frame
process=(root/"src-tauri/src/ios_receiver_process.rs").read_text(encoding="utf-8")
assert "CreateToolhelp32Snapshot" in process
assert "QueryFullProcessImageNameW" in process and "same_executable" in process
assert "JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE" in process
assert "stop_verified_pid" in process and "TerminateProcess" in process
assert "on_window_event" in lib and "ios_airplay::shutdown" in lib
assert "ios_receiver_process::tests" in workflow
assert "ios_device_frame::tests" in workflow
assert 'arg("--log")' in native and "decoded_texture_from_line" in native
assert "source_pixels" in native and "sourceWidth" in ui and "displayWidth" in ui
assert (root/"public/ios-bezel.html").is_file()  # Legacy unused reference file
assert "ios_airplay::tests" in workflow
print("iOS AirPlay preview: inline home, aspect-safe quality caps, passive phone/tablet frame and verified GPL receiver checks passed")
