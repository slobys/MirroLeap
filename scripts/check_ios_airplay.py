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
controls=(root/"src-tauri/src/ios_window_controls.rs").read_text(encoding="utf-8")
assert "mod ios_window_controls;" in lib
assert "WM_LBUTTONUP" in controls and "ControlAction::Minimize" in controls
assert "ControlAction::Close" in controls and "minimize_receiver" in controls
assert "ios_window_controls::tests" in workflow
assert "NativeControls::create()" in frame and "ControlAction::Close" in frame
assert "apply_video_corner_clip" in frame and "clear_video_corner_clip" in frame
assert "native_clip_intact" in frame and "BezelGeometry" in frame
assert "full HWND - rectangular client" in frame
assert "outer_inner_and_native_video_arcs_are_concentric_at_all_dpis" in frame
assert "round_signed_distance" in frame and "metal_band" in frame
assert "silver_metal_contour_is_continuous_across_all_four_curved_corners" in frame
assert "metal_round_caps_leave_only_the_correct_transparent_outside" in frame
assert "MotionGate" in frame and "STABLE_RESHOW_MS:u64=110" in frame
assert "dragging_temporarily_hides_shell_until_source_has_settled" in frame
assert "ShowWindow(bezel,SW_HIDE)" in frame and "motion_hidden()" in frame
assert "DwmSetWindowAttribute" in frame and "DWMWA_BORDER_COLOR" in frame
assert "DWMWA_COLOR_NONE" in frame and "ReceiverDwmStyle" in frame
assert "native_dwm_border_suppression_is_per_window_and_restorable" in frame
assert "WM_PAINT" in controls and "WM_ERASEBKGND" in controls
assert "paint_button" in controls and "button_colors" in controls
assert "stable_placement_needed" in controls
assert "steady_hover_does_not_restack_or_flicker_every_frame" in controls
assert "update_hover" in frame and "HIDE_AFTER_MS" in controls
assert "GetCursorPos" in controls and "SIZE:i32=46" in controls
assert "safe_corner_x" in frame and "each_inner_round_corner_is_covered" in frame
assert "adaptive_quality_cap" in native and "last-display-size.txt" in native
assert "智能清晰" in ui and "qualityTier" in ui
assert "ios_receiver_process::tests" in workflow
assert "ios_device_frame::tests" in workflow
assert 'arg("--log")' in native and "decoded_texture_from_line" in native
assert "source_pixels" in native and "sourceWidth" in ui and "displayWidth" in ui
assert (root/"public/ios-bezel.html").is_file()  # Legacy unused reference file
assert "ios_airplay::tests" in workflow
print("iOS AirPlay preview: inline home, aspect-safe quality caps, passive phone/tablet frame and verified GPL receiver checks passed")
