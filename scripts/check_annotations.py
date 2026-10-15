#!/usr/bin/env python3
"""Static contract for live annotation and automatic resolution switching.

Not a substitute for Windows/WebView2 hit testing; its purpose is to make
regressions detectable before distributing a build to a real Windows user.
"""
from pathlib import Path
import re

root = Path(__file__).resolve().parents[1]
frame = (root / "public/frame.html").read_text(encoding="utf-8")
bar = (root / "public/phone-controls.html").read_text(encoding="utf-8")
native = (root / "src-tauri/src/device_frame.rs").read_text(encoding="utf-8")
layout = (root / "src-tauri/src/frame_layout.rs").read_text(encoding="utf-8")
hook = (root / "src/hooks/useScrcpy.ts").read_text(encoding="utf-8")
control = (root / "src/components/ControlPanel/ControlPanel.tsx").read_text(encoding="utf-8")

assert '<canvas id="annotation"' in frame
assert "window.scrcpyInk = Object.freeze" in frame
assert "pointer-events: none;" in frame, "Drawing surface must be passive by default"
assert "body.inking #annotation { pointer-events: auto; }" in frame
assert "new ResizeObserver(fit)" in frame
assert "bound((event.clientX" in frame and "bound((event.clientY" in frame
assert "canvas.setPointerCapture" in frame and "canvas.releasePointerCapture" in frame
assert "destination-out" in frame, "Eraser must erase ink, not draw over video"
assert all("setColor" in frame for _ in range(1))
assert all(item in frame for item in ("setEnabled", "setTool", "setColor", "setSize", "undo()", "clear()"))

for button in ["drag", "ink-toggle", "minimize", "close",
               "yellow", "red", "white", "cyan", "pen",
               "eraser", "size-3", "size-5", "size-9", "undo", "clear"]:
    assert f'id="{button}"' in bar, f"Missing annotation/control button: {button}"
assert "'ink_toggle'" in bar and "'ink_erase'" in bar and "'ink_undo'" in bar
assert "'ink_clear'" in bar
assert 'overflow-y: auto' not in bar, "Annotation flyout must never require scrolling"
assert '⌫' not in bar and 'id="eraser"' in bar and 'M7 9l8 8' in bar, "Use a visible eraser SVG"
assert 'class="palette-actions"' in bar and 'class="sizes"' in bar
assert 'window.scrcpyControls = Object.freeze' in bar
assert 'side-left' in bar and 'stack-right' in bar and 'stack-left' in bar
assert "window.__TAURI__?.core?.invoke" in bar
assert "device_frame_control" in bar

# V4.4.0 could light up the pen button without a working canvas:
# WebViewWindow.eval() only QUEUES JavaScript; it does not acknowledge that the
# canvas has loaded, enabled input, or stayed above scrcpy's SDL window.
# Tauri 2.10 WebviewWindow has no eval_with_callback: the Canvas must
# acknowledge its execution via a dedicated one-time native IPC command.
assert "ink_acks()" in native and "INK_ACK_NONCE.fetch_add" in native
assert "device_frame_ack" in native and "acknowledge_ink(" in native
assert "tokio::time::timeout(Duration::from_secs(3), rx)" in native
assert "caller.label()" in native and "phone-frame-" in native
assert "commands::device_frame_ack" in (root / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
frame_perms = (root / "src-tauri/capabilities/frame-ack.json").read_text(encoding="utf-8")
assert '"phone-frame-*"' in frame_perms
assert '"shell:default"' not in frame_perms and '"dialog:default"' not in frame_perms
assert "on_page_load" in native and "PageLoadEvent::Finished" in native
assert "ready_to_ink(device)" in native and "InkTransitionGuard::begin(device)" in native
assert "evaluate_ink(&overlay" in native
assert "promote_drawing_windows" in native
assert "position_above(shell, video" in native, "Overlay must layer immediately above SDL"
assert "GW_HWNDPREV" in native and "GetDpiForWindow(hwnd)" in native
assert "window.scrcpyInk.isEnabled() === true" in native
assert "window.scrcpyInk.isEnabled() === false" in native
assert "ink_status" in native
assert "ready: true" in frame and "isEnabled()" in frame
assert "let switching = false;" in bar and "inkButton.disabled = true" in bar
assert "displayInkState(await invoke('ink_status'))" in bar
assert "const actual = await invoke('ink_toggle')" in bar
assert "enabled = !enabled" not in bar
assert "aria-pressed" in bar and "report(error, true)" in bar

assert "set_ignore_cursor_events(true)" in native, "Pass-through must be restored when leaving ink mode"
assert "set_ignore_cursor_events(false)" in native, "Pen mode must capture mouse events"
assert "fn ink_action_script" in native and "fn control_ink" in native
assert "INK_ENABLED" in native and "is_inking(&device)" in native
assert "ANNOTATION_TOOLBAR_HEIGHT" in layout
assert all(command in native for command in (
    "ink_color_yellow", "ink_color_red", "ink_color_white", "ink_color_cyan",
    "ink_size_3", "ink_size_5", "ink_size_9", "ink_clear", "ink_undo"
))

assert "changeMirrorResolution" in hook
assert "canQuickSwitchResolution" in hook
assert "listen<{ device: string; running: boolean }>" in hook
quick_restart = hook.split("const changeMirrorResolution =", 1)[1].split(
    "const stopScrcpy =", 1
)[0]
assert quick_restart.index("detach = await listen<") < quick_restart.index(
    "await invoke('stop_scrcpy', { device });"
)
assert quick_restart.index("await stopped;") < quick_restart.index(
    "await invoke('run_scrcpy', {"
)
assert "new Error(" in hook and "within 8 seconds" in hook
assert "config: {" in hook
assert "adb shell wm size" not in control, "Do not change Android's global system resolution"
assert "onResolutionChange" in control and "isSwitchingResolution" in control

print("Annotation: scoped native IPC ack, window focus, input recovery and state sync verified")
