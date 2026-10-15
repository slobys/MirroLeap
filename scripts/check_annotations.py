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
               "eraser", "size", "undo", "clear"]:
    assert f'id="{button}"' in bar, f"Missing annotation/control button: {button}"
assert "'ink_toggle'" in bar and "'ink_erase'" in bar and "'ink_undo'" in bar
assert "'ink_clear'" in bar
assert "window.__TAURI__?.core?.invoke" in bar
assert "device_frame_control" in bar

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
assert hook.index("detach = await listen<") < hook.index("await invoke('stop_scrcpy', { device });")
assert hook.index("await stopped;") < hook.index("await invoke('run_scrcpy', {")
assert "new Error(" in hook and "within 8 seconds" in hook
assert "config: {" in hook
assert "adb shell wm size" not in control, "Do not change Android's global system resolution"
assert "onResolutionChange" in control and "isSwitchingResolution" in control

print("Annotation: pointer isolation, palette, sizing & session restart handshake verified")
