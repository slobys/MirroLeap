#!/usr/bin/env python3
"""MirroLeap v5.0 unified-release guard.

The five feature PRs were developed from the same main. Merge conflicts in
App.tsx and version manifests can silently discard a feature while standalone
CI for each PR still passes. Check that all five have actual UI entry points,
backend IPC, supported permissions and the one canonical release version.
"""
from pathlib import Path
import json
import re

root = Path(__file__).resolve().parents[1]
read = lambda path: (root / path).read_text(encoding="utf-8")
package = json.loads(read("package.json"))
lock = json.loads(read("package-lock.json"))
tauri = json.loads(read("src-tauri/tauri.conf.json"))
capabilities = json.loads(read("src-tauri/capabilities/default.json"))
crate = read("src-tauri/Cargo.toml")
cargo_lock = read("src-tauri/Cargo.lock")
app = read("src/App.tsx")
native = read("src-tauri/src/lib.rs")
hooks = read("src/hooks/useScrcpy.ts")
ci = read(".github/workflows/desktop-ci.yml")

version = package["version"]
assert re.fullmatch(r"5\.\d+\.\d+", version), "Expected a v5 release"
assert version == lock["version"] == lock["packages"][""]["version"]
assert tauri["version"] == package["version"]
assert tauri["identifier"] == "com.kb.scrcpy-gui-v3", "Existing Windows user settings must survive upgrades"
assert re.search(rf'(?m)^version = "{re.escape(version)}"$', crate)
assert re.search(rf'(?m)^name = "mirroleap"\nversion = "{re.escape(version)}"$', cargo_lock)
assert "tauri-plugin-clipboard-manager" in cargo_lock
assert "tauri-plugin-clipboard-manager" in crate
assert "@tauri-apps/plugin-clipboard-manager" in package["dependencies"]
assert capabilities["windows"] == ["main"]
assert "clipboard-manager:allow-write-image" in capabilities["permissions"]
assert not any("clipboard" in permission and "read" in permission for permission in capabilities["permissions"])

ui = {
    "src/components/ScreenshotTools.tsx": "<ScreenshotTools",
    "src/components/AppUpdateChecker.tsx": "<AppUpdateChecker",
    "src/components/DecoderPerformance.tsx": "<DecoderPerformance",
}
for file, element in ui.items():
    assert (root / file).exists(), file
    assert element in app, f"{element} must be actually mounted in App.tsx"
assert "<Sidebar" in app
assert "videoDecoderSupport" in hooks and "observedDecoders" in hooks
assert "recenterMirrorWindow" in hooks, "Existing shortcut recentering must remain"
assert 'id="present-toggle"' in read("public/phone-controls.html")
assert "<MirroPresentPanel" not in app, "Presenter controls belong on floating rail"
assert "<VersionUpdateBadge" in app and "onOpenUpdate=" in app
assert "app_update::download_official_release_asset" in native
for ipc in (
    "commands::preview_saved_screenshot",
    "commands::get_video_decoder_support",
    "commands::presentation_get_mode",
    "commands::presentation_set_mode",
    "commands::presentation_refresh_snapshot",
    "app_update::check_mirroleap_update",
):
    assert ipc in native, f"Native IPC handler missing: {ipc}"

for check in (
    "check_preview_clipboard.py", "check_wireless_center.py",
    "check_app_update.py", "check_video_decoder.py", "check_mirropresent.py",
):
    assert (root / "scripts" / check).exists(), f"Missing check {check}"
    assert "scripts/" + check in ci, f"Desktop CI must execute {check}"
assert '<canvas id="presentation"' in read("public/frame.html")
assert 'src="/present-overlay.js"' in read("public/frame.html")
assert 'data:image/png;base64,' in read("public/present-overlay.js")
assert "Run-MirroLeap.cmd" in read("README.md")
control = read("src/components/ControlPanel/ControlPanel.tsx")
behavior = read("src/components/SessionBehavior.tsx")
commands = read("src-tauri/src/commands.rs")
assert "handleChange('hidMouse'" not in control
assert "handleChange('otgPure'" not in control
assert "hidMouse: false, otgPure: false" in hooks, "Legacy HID settings must not disable video"
assert "applyRecordingNow" in hooks and "onApplyRecordingNow" in behavior
assert "recordingStatuses" in hooks and "recordingStatus" in behavior
assert "scrcpy-recording" in commands and "scrcpy-recording" in hooks
assert "prepare_recording_directory" in commands
assert "PostMessageW" in commands and "WM_CLOSE" in commands, (
    "Recording must ask scrcpy to finalize video before force-killing"
)
assert "timeout(Duration::from_secs(close_timeout), c.wait())" in commands
assert "if recording.is_some() { 6 } else { 2 }" in commands
assert "emit_recording_file_result" in commands
assert "recording_paths" in native
assert 'config.session_mode != "mirror"' in commands
assert '"--video-source=display"' in commands
assert '"--display-id=0"' in commands
assert '"--new-display"' in commands, "Virtual display mode must stay distinguishable"
assert 'recordFeed: \'录制手机屏幕\'' in read("src/i18n/locales/zh-CN.ts")
assert "Android 手机主屏幕" in behavior, "Show clear Android-only recording source"
assert "config.sessionMode !== 'mirror'" in behavior, "Disable record switch in camera/desktop"


# All four video resolutions must have distinct, real scrcpy launch flags;
# existing preferences survive upgrades and selection auto-applies via the
# guarded stream-restart transaction without manually stopping Wi-Fi ADB.
presets = read("src/utils/mirrorPresets.ts")
panel = read("src/components/ControlPanel/ControlPanel.tsx")
home = read("src/components/CompactDashboard.tsx")
preset_table = presets.split("export const MIRROR_PRESETS:",1)[1].split("};",1)[0]
for profile, size, bitrate, fps in [
    ("'low-latency'","1280",4,60),
    ("balanced","1920",8,60),
    ("quality","2560",16,60),
    ("ultra","3840",20,30),
]:
    assert re.search(rf"{re.escape(profile)}\s*:\s*\{{\s*res:\s*'{size}',\s*bitrate:\s*{bitrate},\s*fps:\s*{fps},\s*codec:\s*'h264'", preset_table)
assert all(size in preset_table for size in ("1280","1920","2560","3840"))
assert "...MIRROR_PRESETS.balanced" in hooks, "New install defaults must use canonical profile"
assert "balanced" in preset_table, "Canonical default must be defined"
assert "...prev, ...parsed" in hooks, "Do not erase old manual configuration"
assert "场景预设" not in home and "场景预设" not in panel, "Duplicate UI scene selector still exists"
assert "ml-preset" not in home and "ml-engine-preset" not in panel
assert "mirrorVideoSummary(config, locale)" in home and "mirrorVideoSummary(config, locale)" in panel
assert 'data-testid="home-video-summary"' in home
assert 'data-testid="settings-video-summary"' in panel
# Keep one concise parameter explanation under the resolution selector.
# The former four-tile overview duplicated the same values and wasted space
# below the decoder/performance cards.
assert "当前参数概览" not in app and "Configuration at a glance" not in app
assert "ml-settings-summary" not in app and "ml-settings-summary" not in read("src/settings-center.css")
assert "DecoderPerformance" in app and "LivePerformance" in app
assert "解码与性能监控" in app
assert "Advanced encoding & display" in panel
assert "videoLoadWarning(config, locale)" in panel and "videoLoadWarning(config, locale)" in home
assert "applyMirrorPreset(config:" in presets and "applyMirrorResolution(config:" in presets
assert "onResolutionChange(event.target.value)" in home
assert "mirror_scene_profile_settings_generate_expected_scrcpy_video_flags" in commands
assert "restartMirrorVideoStream" in hooks
assert "applyMirrorVideoConfig" in hooks
assert "onPresetChange" not in app and "onPresetChange" not in home and "onPresetChange" not in panel
assert "onResolutionChange={handleResolutionChange}" in app
assert "onResolutionChange" in home and "onResolutionChange" in panel
assert "canApplyMirrorVideo" in hooks
assert "applyMirrorResolution" in hooks
assert "restartMirrorVideoStream(previous, launch" in hooks
assert "stop_scrcpy" in hooks and "run_scrcpy" in hooks
assert "src/utils/streamRestart.test.ts" in [p.relative_to(root).as_posix() for p in (root / "src/utils").glob("*.test.ts")]
assert "recording in progress" in read("src/utils/resolutionSwitch.ts").lower()
assert "isSwitchingResolution" in home and "isSwitchingResolution" in panel

# v5.2 creator overlays must be real native/JS integrations, not decorative
# toggles. Click graphics must never intercept the SDL pointer.
phone_toolbar = read("public/phone-controls.html")
frame_page = read("public/frame.html")
frame_native = read("src-tauri/src/device_frame.rs")
creator_click = read("public/creator-effects.js")
present = read("public/present-overlay.js")
assert '<canvas id="creator-effects"' in frame_page
assert 'pointer-events:none' in frame_page
assert 'window.scrcpyCreator=Object.freeze' in creator_click
assert 'GetAsyncKeyState(VK_LBUTTON' in frame_native and 'is_creator_click_enabled' in frame_native
assert 'creator_click_toggle' in frame_native and 'creator-click' in phone_toolbar
# v5.2.1: 50ms geometry polling can miss quick clicks, so only the
# opt-in high-frequency sampler may dispatch them. The native WindowFromPoint
# guard ensures clicks in unrelated foreground windows are ignored.
assert 'start_creator_click_sampler' in frame_native
assert 'Duration::from_millis(12)' in frame_native
assert 'WindowFromPoint' in frame_native
assert 'promote_drawing_windows(app,&device,&overlay)' in frame_native
assert 'evaluate_ink(&verify_overlay,&command).await.is_ok()' in frame_native
assert 'window.mirroCreatorNativeClick?.(\'native\')' in frame_native
assert 'window.mirroCreatorNativeClick = (stage=' in phone_toolbar
assert 'creatorNativeClicks+=1' in phone_toolbar
native_click=read("src-tauri/src/native_click_effect.rs")
assert "UpdateLayeredWindow" in native_click and "EX_TRANSPARENT" in native_click
assert "native_click_effect::flash" in frame_native
assert "native_click_effect::stop" in frame_native
assert "ring_pixel" in native_click
assert "creatorCanvasClicks+=1" in phone_toolbar


for name in ("arrow", "rect", "step", "mask"):
    assert "ink_" + name in frame_native, name
    assert "creator-" + name in phone_toolbar, name
    assert "stroke.tool === '" + name + "'" in frame_page, name
assert 'scrcpy 原始录像' in read("README.md")
assert '原始录屏/OBS' in phone_toolbar
assert 'refresh_presenter_image(app, &device, None)' in frame_native, 'Snapshot mode must still refresh ADB stills'
# Screen video must be magnified natively by Windows instead of polling
# Android PNG screenshots, and never intercept taps or capture in background.
native_lens = read("src-tauri/src/native_live_lens.rs")
assert 'start_native_video_magnifier' in frame_native and 'present_is_live' in frame_native
assert 'native_live_lens::update' in frame_native
assert 'MagInitialize' in native_lens and 'MagSetWindowSource' in native_lens
assert 'MagSetWindowFilterList' in native_lens and 'EX_TRANSPARENT' in native_lens
assert 'lens_geometry' in native_lens and 'fn stop(' in native_lens
assert 'SetWindowsHookExW' in native_lens and 'UnhookWindowsHookEx' in native_lens
assert 'WH_MOUSE_LL' in native_lens and 'CallNextHookEx' in native_lens
assert 'WHEEL_SIZE_DELTA' in native_lens and 'WHEEL_ZOOM_DELTA' in native_lens
assert 'adjust_wheel' in native_lens and 'DEFAULT_FACTOR: f32 = 2.35' in native_lens
assert 'creator_prefers_real_235_zoom_instead_of_hiding_blur_by_reducing_zoom' in native_lens
assert (root / 'MIRROCREATOR_HQ_RENDERING_CN.md').is_file(), 'HQ decoded-frame plan must remain tracked'
assert 'wheel_target_contains' in native_lens and 'return 1;' in native_lens
assert 'FACTOR_MAX: f32 = 3.0' in native_lens
assert 'struct ScopedWheelHook' in native_lens

assert 'set_ignore_cursor_events(mode == "live")' in frame_native
assert 'start_live_zoom_sampling' not in frame_native
assert "mode === 'live'" in present and "mode === 'off' || mode === 'live'" in present
assert "document.body.classList.toggle('presenting', value === 'spotlight' || value === 'magnifier')" in present
assert 'lensX' in present and 'lensY' in present
assert 'Math.max(0, left)' in present and 'Math.max(0, top)' in present

assert '"live" => Some("live")' in frame_native and 'mode === \'live\'' in present
assert 'mode == "live"' in commands, "Main IPC must not enable a non-started sampler"
assert "set_ignore_cursor_events(true)" in frame_native, "Release Android touch on failures"

for expected in (
    f"MirroLeap_{version}_x64_Portable.zip",
    f"MirroLeap_{version}_x64-setup.exe",
    f"MirroLeap_{version}_x64_en-US.msi",
):
    assert expected in read("README.md"), expected
print(f"MirroLeap v{version}: five integrated UI features, Tauri IPC, native dependencies, security scopes, "
      "portable release metadata and CI regression gates verified")
