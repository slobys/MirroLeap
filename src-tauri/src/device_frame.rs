//! Windows-only click-through phone frame with an overscanned native clip.
//!
//! Three independent windows:
//!   - scrcpy SDL window: native rounded mask slightly wider than SVG screen aperture.
//!   - transparent SVG window: antialiased metal bezel covers the hard GDI edge.
//!   - tiny, interactive hover toolbar: drag, minimize, close.
//!
//! The toolbar is a separate native window because making the main full-screen
//! overlay mouse-active would intercept scrcpy taps and swipes.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri::webview::PageLoadEvent;
use windows::Win32::Foundation::{HWND, POINT, RECT, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, CreateRoundRectRgn, DeleteObject, GetMonitorInfoW,
    MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST, HGDIOBJ, SetWindowRgn,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_SCANCODE, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_LBUTTON,
};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, DeferWindowPos, EndDeferWindowPos,
    GetClientRect, GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindow,
    GetWindowRect, GW_HWNDPREV, GetWindowThreadProcessId,
    IsIconic, IsWindowVisible, SetForegroundWindow, SetWindowPos, ShowWindow,
    GetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOPMOST, HWND_TOPMOST, HWND_NOTOPMOST,
    HWND_TOP, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SW_MINIMIZE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
};

use crate::frame_layout::{
    companion_needs_reposition, expand_screen, is_frame_edge_hover, native_clip_radius, toolbar_placement,
    FrameRect, ScreenRect, ToolbarLayout, TOOLBAR_HEIGHT, TOOLBAR_WIDTH,
};
use crate::ScrcpyState;

static ACTIVE_DRAGS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static PRESENTATION_MODES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
static PRESENTATION_BUSY: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static INK_ENABLED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static QUICK_ENABLED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static VIDEO_TOPMOST_REQUESTS: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
fn topmost_requests() -> &'static Mutex<HashMap<String, bool>> {
    VIDEO_TOPMOST_REQUESTS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn quick_sessions() -> &'static Mutex<HashSet<String>> {
    QUICK_ENABLED.get_or_init(|| Mutex::new(HashSet::new()))
}
fn is_quick_open(device: &str) -> bool {
    quick_sessions().lock().map(|guard| guard.contains(device)).unwrap_or(false)
}
static INK_READY: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static INK_BUSY: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
// Tauri 2.10.x WebviewWindow.eval() has no eval_with_callback API. Instead
// frame.html calls a narrow native acknowledgement command after running a
// specific drawing operation. Match the nonce and caller window label.
static INK_ACK_NONCE: AtomicU64 = AtomicU64::new(1);
static INK_ACKS: OnceLock<Mutex<HashMap<u64, (String, tokio::sync::oneshot::Sender<bool>)>>> = OnceLock::new();
fn ink_acks() -> &'static Mutex<HashMap<u64, (String, tokio::sync::oneshot::Sender<bool>)>> {
    INK_ACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// One-use ink bitmap response channel. Only the specific phone-frame window
/// that Rust addressed may return the transparent drawing for compositing.
pub(crate) struct InkImageSnapshot {
    pub png_data_url: String,
    pub orientation: String,
    pub stroke_count: usize,
}
type CaptureReply = Result<InkImageSnapshot, String>;
type CaptureSender = tokio::sync::oneshot::Sender<CaptureReply>;
static CAPTURE_ACK_NONCE: AtomicU64 = AtomicU64::new(1);
static CAPTURE_ACKS: OnceLock<Mutex<HashMap<u64, (String, CaptureSender)>>> = OnceLock::new();
fn capture_acks() -> &'static Mutex<HashMap<u64, (String, CaptureSender)>> {
    CAPTURE_ACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn acknowledge_ink_capture(
    caller: &tauri::Window,
    token: u64,
    png: String,
    orientation: String,
    strokes: usize,
    error: Option<String>,
) -> Result<(), String> {
    if !caller.label().starts_with("phone-frame-") {
        return Err("Only the active phone frame can export its drawing".into());
    }
    if png.len() > 12 * 1024 * 1024 || strokes > 20_000 || error.as_deref().unwrap_or("").len() > 240 {
        return Err("Annotation export exceeds safe capture limits".into());
    }
    let sender = {
        let mut replies = capture_acks().lock().map_err(|e| e.to_string())?;
        match replies.get(&token) {
            Some((expected, _)) if expected == caller.label() => {},
            Some(_) => return Err("Annotation response came from another phone".into()),
            None => return Err("Annotation capture token is not active".into()),
        }
        replies.remove(&token).map(|(_, tx)| tx)
    }.ok_or("Annotation capture has expired")?;
    let result = if let Some(reason) = error.filter(|s| !s.is_empty()) {
        Err(reason)
    } else if !png.starts_with("data:image/png;base64,") || strokes == 0 ||
        !matches!(orientation.as_str(), "portrait" | "landscape")
    {
        Err("No drawing is ready to export. Add a pen stroke and try again.".into())
    } else {
        Ok(InkImageSnapshot { png_data_url: png, orientation, stroke_count: strokes })
    };
    let _ = sender.send(result);
    Ok(())
}

/// Capture existing WebView2 canvas pixels (including eraser transparency),
/// not a desktop screenshot that could contain unrelated foreground windows.
pub(crate) async fn capture_ink_snapshot(
    app: &AppHandle,
    device: &str,
) -> Result<InkImageSnapshot, String> {
    let pid = app.state::<ScrcpyState>().processes.lock()
        .map_err(|e| e.to_string())?
        .get(device).and_then(|process| process.id())
        .ok_or("Start mirroring before exporting the annotated picture")?;
    if capture_screen(pid).is_none() {
        return Err("Restore the phone projection before saving the annotated picture".into());
    }
    if !ready_to_ink(device) {
        return Err("Annotation canvas has not finished loading".into());
    }
    let overlay = app.get_webview_window(&label(device))
        .ok_or("Annotation frame for this phone does not exist")?;
    let token = CAPTURE_ACK_NONCE.fetch_add(1, Ordering::Relaxed);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    capture_acks().lock().map_err(|e| e.to_string())?
        .insert(token, (overlay.label().to_string(), sender));
    let script = format!(r#"(() => {{
      let snapshot = null, error = '';
      try {{
        snapshot = window.scrcpyInk?.captureSnapshot();
        if (!snapshot) throw new Error('Annotation canvas is not ready');
      }} catch (reason) {{
        error = String(reason?.message || reason).slice(0, 200);
      }}
      const invoke = window.__TAURI__?.core?.invoke;
      if (invoke) {{
        void invoke('device_frame_capture_ack', {{
          token: {token}, png: snapshot?.png || '',
          orientation: snapshot?.orientation || '',
          strokes: snapshot?.strokes || 0, error,
        }}).catch(e => console.error('[MirroCapture] Drawing export failed', e));
      }}
    }})()"#);
    if let Err(reason) = overlay.eval(script) {
        capture_acks().lock().map_err(|e| e.to_string())?.remove(&token);
        return Err(format!("Could not request the drawing from the phone frame: {reason}"));
    }
    let result = tokio::time::timeout(Duration::from_secs(5), receiver).await;
    capture_acks().lock().map_err(|e| e.to_string())?.remove(&token);
    result.map_err(|_| "Timed out retrieving the pen drawing. Retry in a moment.".to_string())?
        .map_err(|_| "Phone frame was closed during drawing capture".to_string())?
}

/// Invoked by the phone-frame-* view only. The caller's trusted Tauri label
/// must match the nonce's outstanding overlay; never trust a JS-supplied serial.
pub(crate) fn acknowledge_ink(
    caller: &tauri::Window,
    token: u64,
    success: bool,
) -> Result<(), String> {
    let caller_label = caller.label();
    if !caller_label.starts_with("phone-frame-") {
        return Err("Only the dedicated annotation window may acknowledge ink".into());
    }
    let sender = {
        let mut acks = ink_acks().lock().map_err(|e| e.to_string())?;
        match acks.get(&token) {
            Some((expected, _)) if expected == caller_label => {},
            Some(_) => return Err("Ink acknowledgement came from the wrong window".into()),
            None => return Err("Unrecognized or expired ink acknowledgement".into()),
        }
        acks.remove(&token).map(|(_, tx)| tx)
    }.ok_or("Ink acknowledgement has expired")?;
    let _ = sender.send(success);
    Ok(())
}

fn ink_ready() -> &'static Mutex<HashSet<String>> {
    INK_READY.get_or_init(|| Mutex::new(HashSet::new()))
}
fn ink_busy() -> &'static Mutex<HashSet<String>> {
    INK_BUSY.get_or_init(|| Mutex::new(HashSet::new()))
}

struct InkTransitionGuard(String);
impl InkTransitionGuard {
    fn begin(device: &str) -> Result<Self, String> {
        let mut pending = ink_busy().lock().map_err(|e| e.to_string())?;
        if !pending.insert(device.to_string()) {
            return Err("画笔正在切换，请稍候再操作。".into());
        }
        Ok(Self(device.to_string()))
    }
}
impl Drop for InkTransitionGuard {
    fn drop(&mut self) {
        if let Ok(mut pending) = ink_busy().lock() { pending.remove(&self.0); }
    }
}

fn ready_to_ink(device: &str) -> bool {
    ink_ready().lock().map(|r| r.contains(device)).unwrap_or(false)
}


fn ink_sessions() -> &'static Mutex<HashSet<String>> {
    INK_ENABLED.get_or_init(|| Mutex::new(HashSet::new()))
}
fn is_inking(device: &str) -> bool {
    ink_sessions().lock().map(|set| set.contains(device)).unwrap_or(false)
}

fn presentation_modes() -> &'static Mutex<HashMap<String, &'static str>> {
    PRESENTATION_MODES.get_or_init(|| Mutex::new(HashMap::new()))
}
fn presentation_busy() -> &'static Mutex<HashSet<String>> {
    PRESENTATION_BUSY.get_or_init(|| Mutex::new(HashSet::new()))
}

struct PresentationTransitionGuard(String);
impl PresentationTransitionGuard {
    fn begin(device: &str) -> Result<Self, String> {
        let mut busy = presentation_busy().lock().map_err(|e| e.to_string())?;
        if !busy.insert(device.to_string()) {
            return Err("MirroPresent is already switching modes. Retry shortly.".into());
        }
        Ok(Self(device.to_string()))
    }
}
impl Drop for PresentationTransitionGuard {
    fn drop(&mut self) {
        if let Ok(mut busy) = presentation_busy().lock() { busy.remove(&self.0); }
    }
}

fn known_present_mode(mode: &str) -> Option<&'static str> {
    match mode {
        "off" => Some("off"),
        "spotlight" => Some("spotlight"),
        "magnifier" => Some("magnifier"),
        _ => None,
    }
}
pub(crate) fn presentation_mode(device: &str) -> String {
    presentation_modes().lock().ok()
        .and_then(|modes| modes.get(device).copied())
        .unwrap_or("off").to_string()
}
fn update_presentation_mode(app: &AppHandle, device: &str, mode: &'static str) {
    if let Ok(mut modes) = presentation_modes().lock() {
        if mode == "off" { modes.remove(device); }
        else { modes.insert(device.to_string(), mode); }
    }
    let _ = app.emit("mirroleap-present-mode", serde_json::json!({
        "device": device, "mode": mode,
    }));
}

fn drags() -> &'static Mutex<HashSet<String>> {
    ACTIVE_DRAGS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn is_dragging(device: &str) -> bool {
    drags().lock().map(|items| items.contains(device)).unwrap_or(false)
}

fn hashed_label(prefix: &str, device: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    device.hash(&mut hasher);
    format!("{prefix}-{:016x}", hasher.finish())
}

fn label(device: &str) -> String { hashed_label("phone-frame", device) }
fn toolbar_label(device: &str) -> String { hashed_label("phone-controls", device) }

struct CapturedWindow {
    hwnd: HWND,
    frame: FrameRect,
    screen: ScreenRect,
}

fn capture_screen(pid: u32) -> Option<CapturedWindow> {
    let hwnd = crate::commands::find_window_by_pid(pid)?;
    // IsIconic means the SDL HWND was really minimized. Do not reject
    // IsZoomed: watching landscape full-screen video may maximize the native
    // window, and the device skin should continue to follow that client.
    if !unsafe { IsWindowVisible(hwnd) }.as_bool()
        || unsafe { IsIconic(hwnd) }.as_bool()
    {
        return None;
    }

    let mut rect = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rect) }.is_err() { return None; }
    let mut origin = POINT::default();
    if !unsafe { ClientToScreen(hwnd, &mut origin) }.as_bool() { return None; }

    let screen = ScreenRect {
        x: origin.x,
        y: origin.y,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    };
    // Landscape falls back to the unmodified scrcpy video.
    let frame = expand_screen(screen)?;
    Some(CapturedWindow { hwnd, frame, screen })
}

/// The SDL window needs native corner clipping: otherwise its rectangular
/// video protrudes past the physical rounded case (v4.3.3 regression).
///
/// A matching GDI region (v4.3.2) exposed wallpaper-colored 1px cracks because
/// GDI masks are *not* antialiased, whereas the SVG's transparent aperture is.
///
/// Fix: clip LESS aggressively than the SVG hole. Both radii are 4 physical
/// pixels smaller, so the entire visible rounded aperture remains backed by
/// live SDL video. The SVG bezel and dark gasket overpaint the hard GDI pixels.
/// Do not clip more aggressively or return to the square SVG matte.
fn apply_native_corner_clip(hwnd: HWND, screen: ScreenRect) -> Result<(), String> {
    let mut outer = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut outer) }
        .map_err(|e| format!("Cannot read scrcpy window boundary: {e}"))?;

    let (rx, ry) = native_clip_radius(screen.width, screen.height)
        .ok_or("Invalid screen dimensions for native corner rounding")?;
    let left = screen.x - outer.left;
    let top = screen.y - outer.top;
    let right = left + screen.width;
    let bottom = top + screen.height;

    let region = unsafe {
        // +1 accounts for Win32's exclusive right/bottom clipping coordinates.
        CreateRoundRectRgn(left, top, right + 1, bottom + 1, 2 * rx, 2 * ry)
    };
    if region.0.is_null() {
        return Err("Could not create native rounded video mask".into());
    }
    let result = unsafe { SetWindowRgn(hwnd, Some(region), true) };
    if result == 0 {
        // SetWindowRgn takes ownership of the HRGN only on success.
        let _ = unsafe { DeleteObject(HGDIOBJ(region.0)) };
        return Err("Windows rejected the video mask".into());
    }
    Ok(())
}

fn reset_native_corner_clip(hwnd: HWND) {
    // Restore a full rectangular SDL window for landscape/maximized sessions.
    let _ = unsafe { SetWindowRgn(hwnd, None, true) };
}

fn pointer() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&mut p) }.is_ok() { Some((p.x, p.y)) } else { None }
}

fn virtual_desktop() -> ScreenRect {
    ScreenRect {
        x: unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
        y: unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
        width: unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(TOOLBAR_WIDTH),
        height: unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(TOOLBAR_HEIGHT),
    }
}

/// Use the current monitor's WORK AREA, not the virtual desktop (which
/// includes the Windows taskbar). Also supports side-mounted taskbars and
/// separate usable areas for multiple monitors.
fn monitor_work_area(video: HWND) -> ScreenRect {
    let monitor = unsafe { MonitorFromWindow(video, MONITOR_DEFAULTTONEAREST) };
    if !monitor.0.is_null() {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            let area = info.rcWork;
            if area.right > area.left && area.bottom > area.top {
                return ScreenRect {
                    x: area.left,
                    y: area.top,
                    width: area.right - area.left,
                    height: area.bottom - area.top,
                };
            }
        }
    }
    virtual_desktop()
}

fn inside(x: i32, y: i32, rect: ScreenRect, margin: i32) -> bool {
    x >= rect.x - margin && x < rect.x + rect.width + margin
        && y >= rect.y - margin && y < rect.y + rect.height + margin
}

/// Insert a companion IMMEDIATELY above its video/shell anchor, instead of
/// HWND_TOP. This matters when a browser is foreground: elevating a floating
/// WebView to the absolute top would paint a ghost toolbar over the browser;
/// keeping it above SDL but below the browser preserves the video stack.
fn position_above(window: HWND, anchor: HWND, rect: ScreenRect) -> Result<(), String> {
    let (after, flags) = match unsafe { GetWindow(anchor, GW_HWNDPREV) } {
        Ok(previous) if previous == window => (None, SWP_NOACTIVATE | SWP_NOZORDER),
        Ok(previous) => (Some(previous), SWP_NOACTIVATE),
        // No predecessor means the anchor is the top of the desktop stack.
        Err(_) => (Some(HWND_TOP), SWP_NOACTIVATE),
    };
    unsafe { SetWindowPos(window, after, rect.x, rect.y, rect.width, rect.height, flags) }
        .map_err(|e| format!("Could not position companion above its owner: {e}"))
}

fn place_overlay(overlay: &WebviewWindow, video: HWND, frame: FrameRect) -> Result<(), String> {
    let shell = overlay.hwnd().map_err(|e| e.to_string())?;
    position_above(shell, video, ScreenRect {
        x: frame.x, y: frame.y, width: frame.width, height: frame.height,
    })
}

fn place_toolbar(toolbar: &WebviewWindow, overlay: &WebviewWindow, rect: ScreenRect) -> Result<(), String> {
    let hwnd = toolbar.hwnd().map_err(|e| e.to_string())?;
    // A user is actively pressing toolbar buttons: don't sink that focused
    // HWND behind an unrelated browser until focus actually changes away.
    if unsafe { GetForegroundWindow() } == hwnd {
        return unsafe {
            SetWindowPos(hwnd, Some(HWND_TOP), rect.x, rect.y, rect.width, rect.height, SWP_NOACTIVATE)
        }.map_err(|e| format!("Could not position focused toolbar: {e}"));
    }
    let shell = overlay.hwnd().map_err(|e| e.to_string())?;
    position_above(hwnd, shell, rect)
}

async fn follow(app: AppHandle, device: String, overlay: WebviewWindow, toolbar: WebviewWindow) {
    let mut shell_visible = false;
    let mut controls_visible = false;
    let mut previous_frame: Option<FrameRect> = None;
    let mut applied_orientation: Option<bool> = None;
    let mut clipped_pid: Option<u32> = None;
    let mut last_client_size: Option<(u32, i32, i32)> = None;
    let mut controls_rect: Option<ScreenRect> = None;
    let mut shell_position_checked_at: Option<Instant> = None;
    let mut toolbar_position_checked_at: Option<Instant> = None;
    let mut controls_y_offset: Option<i32> = None;
    let mut last_nearby: Option<Instant> = None;
    let mut video_was_unavailable = true;
    let mut restored_at: Option<Instant> = None;
    let mut previous_layout: Option<ToolbarLayout> = None;
    let mut previous_zoom: Option<u32> = None;
    let mut layout_synced_at: Option<Instant> = None;
    let mut topmost_checked_at: Option<Instant> = None;
    let mut clip_retry_reported = false;
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;
        let pid = app.state::<ScrcpyState>()
            .processes.lock().ok()
            .and_then(|processes| processes.get(&device).and_then(|child| child.id()));
        let Some(pid) = pid else { break };

        let Some(captured) = capture_screen(pid) else {
            // The underlying video is temporarily minimized or unavailable.
            // Don't terminate the follow task: it must reattach when the user
            // restores scrcpy via taskbar/Alt+Tab, even after several minutes.
            if clipped_pid == Some(pid) {
                if let Some(hwnd) = crate::commands::find_window_by_pid(pid) {
                    reset_native_corner_clip(hwnd);
                }
            }
            clipped_pid = None;
            last_client_size = None;
            if shell_visible { let _ = overlay.hide(); shell_visible = false; }
            if controls_visible { let _ = toolbar.hide(); controls_visible = false; }
            previous_frame = None;
            controls_rect = None;
            shell_position_checked_at = None;
            toolbar_position_checked_at = None;
            controls_y_offset = None;
            last_nearby = None;
            applied_orientation = None;
            previous_layout = None;
            previous_zoom = None;
            layout_synced_at = None;
            video_was_unavailable = true;
            continue;
        };

        let hwnd = captured.hwnd;
        let frame = captured.frame;
        let screen = captured.screen;
        // The interactive drag loop moves SDL, the transparent bezel and
        // toolbar together through one native DeferWindowPos batch. The
        // slower 50ms health follower must not move the same windows in
        // parallel, which previously caused jitter and visible separation.
        if is_dragging(&device) {
            previous_frame = None;
            controls_rect = None;
            shell_position_checked_at = None;
            toolbar_position_checked_at = None;
            continue;
        }
        if topmost_checked_at.map(|t| t.elapsed() > Duration::from_millis(650)).unwrap_or(true) {
            if let Ok(state) = topmost_requests().lock() {
                if let Some(&desired) = state.get(&device) {
                    let companions_match = [overlay.hwnd().ok(), toolbar.hwnd().ok()]
                        .into_iter().flatten()
                        .all(|handle| video_is_topmost(handle) == desired);
                    if video_is_topmost(hwnd) != desired || !companions_match {
                        let _ = apply_video_topmost(&app, &device, hwnd, desired);
                    }
                }
            }
            topmost_checked_at = Some(Instant::now());
        }
        if video_was_unavailable {
            // The video can be restored while the browser still owns focus.
            // Show the control handle briefly so the user can discover it
            // without having to guess the edge-hover gesture again.
            restored_at = Some(Instant::now());
            video_was_unavailable = false;
        }
        let landscape = crate::frame_layout::is_landscape(screen.width, screen.height);

        // Switch the external phone skin, NOT the native video stream. Retry
        // until the actual WebView has finished loading or reloading.
        if !ready_to_ink(&device) {
            applied_orientation = None;
        } else if applied_orientation != Some(landscape) {
            let orientation = if landscape { "landscape" } else { "portrait" };
            if overlay.eval(format!("window.scrcpyFrame?.setOrientation('{}')", orientation)).is_ok() {
                applied_orientation = Some(landscape);
            }
        }

        // Avoid redundant SetWindowRgn calls while the phone is stationary.
        // Re-create it after scrcpy rotation, resize, or codec-retry PID change.
        if last_client_size != Some((pid, screen.width, screen.height)) {
            if let Err(error) = apply_native_corner_clip(hwnd, screen) {
                if !clip_retry_reported {
                    let _ = app.emit("scrcpy-log",
                        format!("[FRAME] Native screen corners unavailable; retrying: {error}"));
                    clip_retry_reported = true;
                }
                // Immediately after taskbar restoration SDL can still be
                // resizing its HWND. Losing one GDI region request must NOT
                // permanently terminate the overlay + toolbar follow loop.
                continue;
            }
            clip_retry_reported = false;
            clipped_pid = Some(pid);
            last_client_size = Some((pid, screen.width, screen.height));
        }

        let toolbar_hwnd = toolbar.hwnd().ok();
        let overlay_hwnd = overlay.hwnd().ok();
        let foreground = unsafe { GetForegroundWindow() };
        // Quick Android navigation uses the same native expanded rail
        // geometry as the pen palette, but never intercepts phone touches.
        let inking = is_inking(&device) || is_quick_open(&device);
        let presenting = presentation_mode(&device) != "off";
        let active = foreground == hwnd
            || toolbar_hwnd == Some(foreground)
            || overlay_hwnd == Some(foreground);

        // Tauri .hide/.show and a separate SDL HWND can lose z-order when
        // minimized then restored. The old code treated an *attempted* show
        // as proof that the overlay was visible forever, and broke the whole
        // follow task on any transient SetWindowPos error.
        let actual_visible = overlay.hwnd().ok()
            .map(|h| unsafe { IsWindowVisible(h).as_bool() }).unwrap_or(false);
        let needs_restore = !shell_visible || !actual_visible;
        if needs_restore {
            if overlay.show().is_ok() {
                // Tauri/WebView2 may reset HWND hit-testing across hide/show.
                // Presenter and ink are the only modes allowed to intercept
                // Android mouse events after the SDL window is restored.
                let _ = overlay.set_ignore_cursor_events(!(inking || presenting));
                shell_visible = true;
                previous_frame = None;
                // Re-establish HWND z-order AFTER making WebView visible.
                if let Err(error) = place_overlay(&overlay, hwnd, frame) {
                    let _ = app.emit("scrcpy-log", format!("[FRAME] Retrying restored shell: {error}"));
                    shell_visible = false;
                    shell_position_checked_at = None;
                } else {
                    previous_frame = Some(frame);
                    shell_position_checked_at = Some(Instant::now());
                }
            } else {
                shell_visible = false;
            }
        }
        let shell_behind_video = overlay_hwnd.map(|shell| {
            unsafe { GetWindow(hwnd, GW_HWNDPREV) }.map(|above| above != shell).unwrap_or(true)
        }).unwrap_or(true);
        let shell_rect = ScreenRect {
            x: frame.x, y: frame.y, width: frame.width, height: frame.height,
        };
        let shell_health_due = shell_position_checked_at
            .map(|last| last.elapsed() > Duration::from_secs(2)).unwrap_or(true);
        if shell_visible && companion_needs_reposition(
            previous_frame.map(|rect| ScreenRect {
                x: rect.x, y: rect.y, width: rect.width, height: rect.height,
            }),
            shell_rect,
            actual_visible || needs_restore,
            !shell_behind_video,
            shell_health_due,
        ) {
            match place_overlay(&overlay, hwnd, frame) {
                Ok(()) => {
                    previous_frame = Some(frame);
                    shell_position_checked_at = Some(Instant::now());
                },
                Err(error) => {
                    // Do not destroy the skin or toolbar just because Windows
                    // temporarily refuses one native window positioning call.
                    let _ = app.emit("scrcpy-log", format!("[FRAME] Position will retry: {error}"));
                    previous_frame = None;
                    shell_position_checked_at = None;
                }
            }
        }

        // Keep Android interactions native. The palette is contained in the
        // same WebView window as the rail but never increases its *height*
        // until a narrow desktop margin requires a compact stacked layout.
        let cursor = pointer();
        let dragging = is_dragging(&device);
        let close_to_edge = cursor.map(|(x,y)| is_frame_edge_hover(x,y,frame)).unwrap_or(false);
        let inside_controls = match (cursor, controls_rect) {
            (Some((x,y)), Some(r)) => inside(x,y,r,12),
            _ => false,
        };
        // After a Windows taskbar restore the SDL window may be visible even
        // while another application owns foreground focus. An actual pointer
        // hover over the phone bezel should STILL reveal its control handle.
        let recent_restore = restored_at.map(|t| t.elapsed() < Duration::from_millis(2200))
            .unwrap_or(false);
        let wanted = close_to_edge || dragging || inking || presenting || inside_controls || recent_restore;
        if wanted { last_nearby = Some(Instant::now()); }
        let lingering = last_nearby.map(|t| t.elapsed() < Duration::from_millis(550))
            .unwrap_or(false);
        let show_controls = wanted || (active && lingering);

        if show_controls {
            let desktop = monitor_work_area(hwnd);
            // WebView is laid out in CSS DIPs. Use the SDL monitor's physical
            // DPI to size the native window (DPI can change when dragging to
            // a different monitor).
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
            if controls_y_offset.is_none() {
                let mouse_y = cursor.filter(|(_, y)| *y >= frame.y && *y < frame.y + frame.height)
                    .map(|(_, y)| y).unwrap_or(frame.y + 100);
                let narrow_height = (TOOLBAR_HEIGHT as i64 * dpi as i64 / 96) as i32;
                let top = mouse_y - narrow_height / 2;
                if let Some(compact) = toolbar_placement(frame, top, desktop, false, dpi) {
                    controls_y_offset = Some(compact.rect.y - frame.y);
                }
            }
            // The top of the compact rail is stable as the phone moves; the
            // expanded flyout is allowed to move UP to fit within the desktop.
            let top = frame.y + controls_y_offset.unwrap_or(0);
            let Some(placement) = toolbar_placement(frame, top, desktop, inking, dpi) else {
                // If scrcpy has no visible intersection with the monitor's
                // work area, hide controls rather than show a floating ghost.
                if controls_visible { let _ = toolbar.hide(); controls_visible = false; }
                controls_rect = None;
                controls_y_offset = None;
                previous_layout = None;
                previous_zoom = None;
                toolbar_position_checked_at = None;
                continue;
            };
            let rect = placement.rect;
            let actually_visible = toolbar.hwnd().ok()
                .map(|h| unsafe { IsWindowVisible(h).as_bool() }).unwrap_or(false);
            let mut newly_shown = false;
            if !controls_visible || !actually_visible {
                if toolbar.show().is_ok() {
                    controls_visible = true;
                    newly_shown = true;
                } else {
                    controls_visible = false;
                }
            }
            if controls_visible {
                // Tauri show() can reset the native HWND's position and z-order.
                // Position immediately on restore, drag, DPI/rotation change
                // or z-order loss. A stationary companion should not get
                // SetWindowPos() 20 times per second: it causes unnecessary
                // DWM compositor work and occasional focus flicker.
                let toolbar_stacked = match (overlay_hwnd, toolbar_hwnd) {
                    (Some(_shell), Some(control)) if foreground == control => true,
                    (Some(shell), Some(control)) =>
                        unsafe { GetWindow(shell, GW_HWNDPREV) }
                            .map(|above| above == control).unwrap_or(false),
                    _ => false,
                };
                let health_due = toolbar_position_checked_at
                    .map(|last| last.elapsed() > Duration::from_secs(2)).unwrap_or(true);
                if companion_needs_reposition(
                    controls_rect, rect, actually_visible && !newly_shown,
                    toolbar_stacked, health_due,
                ) {
                    if let Err(error) = place_toolbar(&toolbar, &overlay, rect) {
                        let _ = app.emit("scrcpy-log",
                            format!("[FRAME] Repositioning toolbar on next tick: {error}"));
                        controls_rect = None;
                        toolbar_position_checked_at = None;
                    } else {
                        controls_rect = Some(rect);
                        toolbar_position_checked_at = Some(Instant::now());
                    }
                }
                let refresh_layout = layout_synced_at.map(|t| t.elapsed() > Duration::from_secs(2))
                    .unwrap_or(true);
                if newly_shown || previous_layout != Some(placement.layout)
                    || previous_zoom != Some(placement.zoom_percent) || refresh_layout
                {
                    let script = format!(
                        "window.scrcpyControls?.setLayout('{}', {}); window.scrcpyControls?.sync()",
                        placement.layout.css_name(), placement.zoom_percent
                    );
                    if toolbar.eval(script).is_ok() {
                        previous_layout = Some(placement.layout);
                        previous_zoom = Some(placement.zoom_percent);
                        layout_synced_at = Some(Instant::now());
                    }
                }
            }
        } else {
            if controls_visible { let _ = toolbar.hide(); controls_visible = false; }
            controls_rect = None;
            toolbar_position_checked_at = None;
            controls_y_offset = None;
            previous_layout = None;
            previous_zoom = None;
            layout_synced_at = None;
        }
    }

    if let Ok(mut set) = ink_sessions().lock() { set.remove(&device); }
    if let Ok(mut set) = ink_ready().lock() { set.remove(&device); }
    update_presentation_mode(&app, &device, "off");
    let _ = overlay.set_ignore_cursor_events(true);
    if let Some(pid) = clipped_pid {
        if let Some(hwnd) = crate::commands::find_window_by_pid(pid) {
            reset_native_corner_clip(hwnd);
        }
    }
    let _ = toolbar.close();
    let _ = overlay.close();
}

fn device_for_controls(app: &AppHandle, caller: &tauri::Window) -> Result<String, String> {
    let processes = app.state::<ScrcpyState>();
    let guard = processes.processes.lock().map_err(|e| e.to_string())?;
    guard.keys()
        .find(|device| toolbar_label(device) == caller.label())
        .cloned()
        .ok_or_else(|| "This control bar is not attached to an active scrcpy window".into())
}

/// Called only by the dedicated phone-controls-* toolbar window.
/// The full-screen visual overlay cannot receive pointer events by design.
fn ink_action_script(action: &str) -> Option<&'static str> {
    match action {
        "ink_pen" => Some("window.scrcpyInk?.setTool('pen')"),
        "ink_erase" => Some("window.scrcpyInk?.setTool('erase')"),
        "ink_color_yellow" => Some("window.scrcpyInk?.setColor('#ffd54b')"),
        "ink_color_red" => Some("window.scrcpyInk?.setColor('#ff5252')"),
        "ink_color_white" => Some("window.scrcpyInk?.setColor('#ffffff')"),
        "ink_color_cyan" => Some("window.scrcpyInk?.setColor('#40e0d0')"),
        "ink_size_3" => Some("window.scrcpyInk?.setSize(3)"),
        "ink_size_5" => Some("window.scrcpyInk?.setSize(5)"),
        "ink_size_9" => Some("window.scrcpyInk?.setSize(9)"),
        "ink_undo" => Some("window.scrcpyInk?.undo()"),
        "ink_clear" => Some("window.scrcpyInk?.clear()"),
        _ => None,
    }
}

/// Actually execute an ink command on the WebView2 canvas, then await its
/// one-time acknowledgement through Tauri's supported native IPC command.
///
/// Merely calling WebviewWindow.eval() returns when the script is QUEUED, not
/// when it has RUN. Previous versions optimistically turned green and stole
/// mouse events even if this canvas had not finished loading.
///
/// Tauri 2.10.x does not yet implement eval_with_callback on WebviewWindow,
/// so we use an authenticated nonce and the caller's trusted window label.
async fn evaluate_ink(overlay: &WebviewWindow, operation: &str) -> Result<bool, String> {
    let token = INK_ACK_NONCE.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    ink_acks().lock().map_err(|e| e.to_string())?
        .insert(token, (overlay.label().to_string(), tx));
    let js = format!(r#"(() => {{
      let success = false;
      try {{
        const ink = window.scrcpyInk;
        if (ink && ink.ready === true) {{
          success = Boolean((() => {{ {} }})());
        }}
      }} catch (error) {{
        console.error('[MirroLeap Ink] Canvas operation failed', error);
      }}
      const call = window.__TAURI__?.core?.invoke;
      if (call) {{
        void call('device_frame_ack', {{ token: {}, success }})
          .catch(error => console.error('[MirroLeap Ink] Acknowledgement failed', error));
      }}
    }})()"#, operation, token);
    if let Err(error) = overlay.eval(js) {
        ink_acks().lock().map_err(|e| e.to_string())?.remove(&token);
        return Err(format!("向画布发送绘制指令失败: {error}"));
    }
    let received = tokio::time::timeout(Duration::from_secs(3), rx).await;
    // Timeout and cancellation must never leave one-time senders in memory.
    ink_acks().lock().map_err(|e| e.to_string())?.remove(&token);
    let ok = received
        .map_err(|_| "画笔画布未响应，请稍后重新点击画笔按钮。".to_string())?
        .map_err(|_| "画笔窗口已断开，请重新启动投屏。".to_string())?;
    if !ok {
        return Err("画笔还没有就绪，未接管手机鼠标。请稍后重试。".into());
    }
    Ok(true)
}

fn promote_drawing_windows(
    app: &AppHandle,
    device: &str,
    overlay: &WebviewWindow,
) -> Result<(), String> {
    let shell = overlay.hwnd().map_err(|e| e.to_string())?;
    unsafe {
        SetWindowPos(shell, Some(HWND_TOP), 0, 0, 0, 0,
                     SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE)
    }.map_err(|e| format!("无法将画笔放到实时画面上方: {e}"))?;
    // Promoting the shell must not cover pen/close/minimize buttons.
    if let Some(toolbar) = app.get_webview_window(&toolbar_label(device)) {
        let handle = toolbar.hwnd().map_err(|e| e.to_string())?;
        unsafe {
            SetWindowPos(handle, Some(HWND_TOP), 0, 0, 0, 0,
                         SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE)
        }.map_err(|e| format!("无法保持画笔工具栏在前景: {e}"))?;
    }
    Ok(())
}

/// This is the ONLY code path enabling a native SDL-covering presentation
/// overlay. The ordinary phone shell stays mouse-through. Spotlight and
/// snapshot magnifier intentionally intercept Android input until exited.
pub(crate) async fn set_presentation_mode(
    app: &AppHandle, device: &str, requested: &str,
) -> Result<String, String> {
    let mode = known_present_mode(requested).ok_or("Unknown MirroPresent mode")?;
    let _guard = PresentationTransitionGuard::begin(device)?;
    if ink_busy().lock().map_err(|e| e.to_string())?.contains(device) {
        return Err("Please wait for the pen mode to finish switching".into());
    }
    let overlay = app.get_webview_window(&label(device))
        .ok_or("Start a framed Windows phone mirror before enabling MirroPresent")?;

    if mode == "off" {
        // Always restore Android taps FIRST, even when WebView2 reloaded.
        overlay.set_ignore_cursor_events(true)
            .map_err(|e| format!("Cannot restore phone touch: {e}"))?;
        update_presentation_mode(app, device, "off");
        if ready_to_ink(device) {
            let _ = evaluate_ink(&overlay,
                "return window.scrcpyPresent?.setMode('off') === 'off';").await;
        }
        return Ok("off".into());
    }
    let pid = app.state::<ScrcpyState>().processes.lock()
        .map_err(|e| e.to_string())?
        .get(device).and_then(|child| child.id())
        .ok_or("The selected Android mirror is not running")?;
    // HWND contains a raw pointer; convert it to a plain integer BEFORE
    // the async WebView ACK so this command remains Send in Tauri's executor.
    let video_hwnd = capture_screen(pid)
        .ok_or("Restore the phone projection before starting the presentation")?
        .hwnd.0 as usize;
    if !ready_to_ink(device) {
        return Err("Phone presentation layer is still loading".into());
    }

    if is_inking(device) {
        // Mutually exclusive POINTER modes. Preserve existing ink strokes,
        // but stop routing mouse input to drawing before activating focus.
        evaluate_ink(&overlay,
            "window.scrcpyInk.setEnabled(false); return window.scrcpyInk.isEnabled() === false;"
        ).await?;
        ink_sessions().lock().map_err(|e| e.to_string())?.remove(device);
    }
    let js = match mode {
        "spotlight" => "return window.scrcpyPresent?.setMode('spotlight') === 'spotlight';",
        "magnifier" => "return window.scrcpyPresent?.setMode('magnifier') === 'magnifier';",
        _ => unreachable!(),
    };
    if let Err(error) = evaluate_ink(&overlay, js).await {
        // If activation fails after leaving pen mode, immediately return
        // input to Android. Never keep an invisible hit-test window active.
        let _ = overlay.set_ignore_cursor_events(true);
        let _ = overlay.eval("window.scrcpyPresent?.setMode('off')");
        update_presentation_mode(app, device, "off");
        return Err(error);
    }
    if let Err(error) = overlay.set_ignore_cursor_events(false) {
        let _ = overlay.eval("window.scrcpyPresent?.setMode('off')");
        update_presentation_mode(app, device, "off");
        return Err(format!("Could not enable presenter focus pointer: {error}"));
    }
    if let Err(error) = promote_drawing_windows(app, device, &overlay) {
        let _ = overlay.set_ignore_cursor_events(true);
        let _ = overlay.eval("window.scrcpyPresent?.setMode('off')");
        update_presentation_mode(app, device, "off");
        return Err(error);
    }
    let _ = unsafe { SetForegroundWindow(HWND(video_hwnd as *mut _)) };
    if let Err(error) = promote_drawing_windows(app, device, &overlay) {
        let _ = overlay.set_ignore_cursor_events(true);
        let _ = overlay.eval("window.scrcpyPresent?.setMode('off')");
        update_presentation_mode(app, device, "off");
        return Err(error);
    }
    update_presentation_mode(app, device, mode);
    Ok(mode.to_string())
}

/// Transfer ONE explicit Android PNG to the current phone's presentation
/// WebView. This is a FROZEN snapshot, not scrcpy's live SDL image. Reuse the
/// nonce/Window-label acknowledgement already trusted by ink operations, and
/// wait for Image.onload before reporting success to the user.
pub(crate) async fn set_presentation_snapshot(
    app: &AppHandle, device: &str, png: &[u8],
) -> Result<(), String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    if presentation_mode(device) != "magnifier" {
        return Err("Switch to MirroPresent snapshot magnifier before capturing".into());
    }
    if !ready_to_ink(device) {
        return Err("Phone presentation view has not loaded".into());
    }
    // A very large PNG expanded into a JS eval string can freeze WebView2.
    const MAX_PRESENTER_BYTES: usize = 8 * 1024 * 1024;
    if png.len() < 45 || png.len() > MAX_PRESENTER_BYTES ||
        !png.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        return Err("Snapshot is not a supported PNG (8 MiB limit)".into());
    }
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap_or([0; 4])) as u64;
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap_or([0; 4])) as u64;
    if width == 0 || height == 0 || width.saturating_mul(height) > 16_000_000 {
        return Err("Snapshot dimensions are outside safe bounds".into());
    }
    let pid = app.state::<ScrcpyState>().processes.lock()
        .map_err(|e| e.to_string())?
        .get(device).and_then(|child| child.id())
        .ok_or("Phone session ended before snapshot capture")?;
    let screenshot_ratio = width as f64 / height as f64;
    let video_ratio = {
        let actual = capture_screen(pid).ok_or("Restore the phone projection first")?;
        actual.screen.width as f64 / actual.screen.height as f64
    };
    if (screenshot_ratio / video_ratio - 1.0).abs() > 0.06 {
        return Err("Phone rotated while capturing snapshot. Wait for landscape to settle and retry.".into());
    }
    let orientation = if width > height { "landscape" } else { "portrait" };
    let overlay = app.get_webview_window(&label(device))
        .ok_or("Phone presentation layer closed")?;
    let token = INK_ACK_NONCE.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    ink_acks().lock().map_err(|e| e.to_string())?
        .insert(token, (overlay.label().to_string(), tx));
    let encoded = STANDARD.encode(png);
    let js = format!(r#"(() => {{
      void (async () => {{
        let success = false;
        try {{
          success = await window.scrcpyPresent?.loadSnapshot(
            'data:image/png;base64,{encoded}', '{orientation}'
          ) === true;
        }} catch (error) {{
          console.error('[MirroPresent] Snapshot rejected', error);
        }}
        const invoke = window.__TAURI__?.core?.invoke;
        if (invoke) {{
          await invoke('device_frame_ack', {{token: {token}, success}})
            .catch(error => console.error('[MirroPresent] Snapshot acknowledgement failed', error));
        }}
      }})();
    }})()"#);
    if let Err(error) = overlay.eval(js) {
        ink_acks().lock().map_err(|e| e.to_string())?.remove(&token);
        return Err(format!("Cannot transfer phone screenshot to magnifier: {error}"));
    }
    let reply = tokio::time::timeout(Duration::from_secs(8), rx).await;
    ink_acks().lock().map_err(|e| e.to_string())?.remove(&token);
    let accepted = reply
        .map_err(|_| "Phone snapshot preview timed out after 8 seconds".to_string())?
        .map_err(|_| "Presentation view closed before the snapshot appeared".to_string())?;
    if !accepted {
        return Err("Snapshot could not load or the phone rotated. Retry after the screen settles.".into());
    }
    Ok(())
}

async fn control_ink(app: &AppHandle, device: &str, action: &str) -> Result<bool, String> {
    // UI asks native state instead of a separate toggle flag which could drift.
    if action == "ink_status" { return Ok(is_inking(device)); }

    let overlay = app.get_webview_window(&label(device))
        .ok_or("真机外框不存在，无法开启画笔。")?;
    if action == "ink_reset" {
        let _guard = InkTransitionGuard::begin(device)?;
        // One-click restore is deliberately allowed with pen mode ON or OFF.
        // Recover Android input first even if the canvas has reloaded.
        overlay.set_ignore_cursor_events(true)
            .map_err(|e| format!("恢复手机鼠标输入失败: {e}"))?;
        update_presentation_mode(app, device, "off");
        let _ = overlay.eval("window.scrcpyPresent?.setMode('off')");
        ink_sessions().lock().map_err(|e| e.to_string())?.remove(device);
        let clear = if ready_to_ink(device) {
            evaluate_ink(&overlay,
                "window.scrcpyInk.clear(); window.scrcpyInk.setEnabled(false); return window.scrcpyInk.isEnabled() === false;"
            ).await
        } else {
            // A reloaded canvas starts empty; there cannot be a previous
            // surviving ink bitmap to erase in the new WebView context.
            Ok(true)
        };
        if let Some(pid) = app.state::<ScrcpyState>()
            .processes.lock().ok()
            .and_then(|map| map.get(device).and_then(|child| child.id()))
        {
            if let Some(video) = crate::commands::find_window_by_pid(pid) {
                let _ = unsafe { SetForegroundWindow(video) };
            }
        }
        clear.map_err(|e| format!("手机触控已恢复，但画笔清除确认失败: {e}"))?;
        let _ = app.emit("scrcpy-log", format!("[INK] One-click reset annotations for {device}"));
        return Ok(false);
    }
    if action == "ink_toggle" {
        let _guard = InkTransitionGuard::begin(device)?;
        let turn_on = !is_inking(device);

        if turn_on {
            if !ready_to_ink(device) {
                return Err("画笔画布尚未加载完毕，请稍后重试。".into());
            }
            if presentation_mode(device) != "off" {
                overlay.set_ignore_cursor_events(true)
                    .map_err(|e| format!("Cannot leave presentation mode: {e}"))?;
                evaluate_ink(&overlay,
                    "return window.scrcpyPresent?.setMode('off') === 'off';").await?;
                update_presentation_mode(app, device, "off");
            }
            // Verify JS has entered ink mode before intercepting SDL clicks.
            evaluate_ink(&overlay, "window.scrcpyInk.setEnabled(true); return window.scrcpyInk.isEnabled() === true;").await?;
            if let Err(error) = overlay.set_ignore_cursor_events(false) {
                let _ = overlay.eval("window.scrcpyInk?.setEnabled(false)");
                return Err(format!("启用画笔鼠标捕获失败: {error}"));
            }
            if let Err(error) = promote_drawing_windows(app, device, &overlay) {
                let _ = overlay.set_ignore_cursor_events(true);
                let _ = overlay.eval("window.scrcpyInk?.setEnabled(false)");
                return Err(error);
            }
            ink_sessions().lock().map_err(|e| e.to_string())?.insert(device.to_owned());
            let _ = app.emit("scrcpy-log", format!("[INK] Enabled annotation on {device}"));
            return Ok(true);
        }

        // Restore real phone interaction first. Even if HTML reloaded,
        // no invisible overlay should continue capturing Android taps.
        overlay.set_ignore_cursor_events(true)
            .map_err(|e| format!("无法恢复手机鼠标操作: {e}"))?;
        if ready_to_ink(device) {
            if let Err(e) = evaluate_ink(&overlay, "window.scrcpyInk.setEnabled(false); return window.scrcpyInk.isEnabled() === false;").await {
                let _ = app.emit("scrcpy-log", format!("[INK] Drawing page reset while disabling: {e}"));
            }
        }
        ink_sessions().lock().map_err(|e| e.to_string())?.remove(device);
        if let Some(pid) = app.state::<ScrcpyState>()
            .processes.lock().ok()
            .and_then(|map| map.get(device).and_then(|child| child.id()))
        {
            if let Some(video) = crate::commands::find_window_by_pid(pid) {
                let _ = unsafe { SetForegroundWindow(video) };
            }
        }
        let _ = app.emit("scrcpy-log", format!("[INK] Restored phone control on {device}"));
        return Ok(false);
    }

    if ink_busy().lock().map_err(|e| e.to_string())?.contains(device) {
        return Err("画笔正在切换，请稍候再操作。".into());
    }
    if !is_inking(device) {
        return Err("请先点击画笔按钮进入标注模式。".into());
    }
    let code = ink_action_script(action).ok_or("不支持的画笔操作")?;
    let command = format!("{}; return true;", code);
    evaluate_ink(&overlay, &command).await?;
    Ok(true)
}

/// The dedicated phone-controls-* window is the ONLY trusted caller.
/// The SVG overlay captures pointer events solely during annotation mode.
fn send_video_shortcut(hwnd: HWND, action: &str) -> Result<(), String> {
    // SDL polls real keyboard state. PostMessage(WM_SYSKEYDOWN) returns
    // success even when SDL ignores the synthetic event. Instead move input
    // focus to the PID-validated scrcpy video and inject real scan codes.
    let scan = match action {
        "quick_fullscreen" => 0x21_u16,
        "quick_rotate" => 0x13_u16,
        "quick_paste" => 0x2F_u16,
        _ => return Err("Unsupported scrcpy shortcut".into()),
    };
    if unsafe { IsIconic(hwnd).as_bool() } {
        return Err("Restore the mirrored phone before using a shortcut".into());
    }
    let current_thread = unsafe { GetCurrentThreadId() };
    let current_foreground = unsafe { GetForegroundWindow() };
    let foreground_thread = unsafe { GetWindowThreadProcessId(current_foreground, None) };
    // Foreground activation is permitted as a consequence of the actual
    // phone-toolbar user click. Briefly attach to its input thread if needed;
    // always detach before returning.
    let attached = foreground_thread != 0 && foreground_thread != current_thread
        && unsafe { AttachThreadInput(current_thread, foreground_thread, true) }.as_bool();
    let _ = unsafe { SetForegroundWindow(hwnd) };
    if attached {
        let _ = unsafe { AttachThreadInput(current_thread, foreground_thread, false) };
    }
    if unsafe { GetForegroundWindow() } != hwnd {
        return Err("Windows did not allow scrcpy to receive keyboard focus; retry after clicking the video".into());
    }
    let key = |scan: u16, release: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT {
            wVk: VIRTUAL_KEY(0), wScan: scan,
            dwFlags: KEYEVENTF_SCANCODE | if release { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
            time: 0, dwExtraInfo: 0,
        }},
    };
    const ALT: u16 = 0x38;
    let input = [key(ALT, false), key(scan, false), key(scan, true), key(ALT, true)];
    if unsafe { SendInput(&input, std::mem::size_of::<INPUT>() as i32) } != input.len() as u32 {
        return Err("Windows refused to send the scrcpy shortcut".into());
    }
    Ok(())
}

fn video_is_topmost(hwnd: HWND) -> bool {
    (unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0) != 0
}
fn apply_video_topmost(app: &AppHandle, device: &str, hwnd: HWND, desired: bool) -> Result<(), String> {
    let placement = if desired { HWND_TOPMOST } else { HWND_NOTOPMOST };
    unsafe { SetWindowPos(hwnd, Some(placement), 0, 0, 0, 0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) }
        .map_err(|e| format!("Cannot update video topmost: {e}"))?;
    for name in [label(device), toolbar_label(device)] {
        if let Some(window) = app.get_webview_window(&name) {
            if let Ok(handle) = window.hwnd() {
                unsafe { SetWindowPos(handle, Some(placement), 0, 0, 0, 0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) }
                    .map_err(|e| format!("Cannot synchronize phone bezel topmost: {e}"))?;
            }
        }
    }
    if video_is_topmost(hwnd) != desired {
        return Err("Windows did not apply the requested topmost status".into());
    }
    Ok(())
}
fn toggle_video_topmost(app: &AppHandle, device: &str, hwnd: HWND) -> Result<bool, String> {
    let requested = !video_is_topmost(hwnd);
    apply_video_topmost(app, device, hwnd, requested)?;
    topmost_requests().lock().map_err(|e| e.to_string())?
        .insert(device.to_string(), requested);
    Ok(requested)
}

pub async fn control(app: &AppHandle, caller: &tauri::Window, action: &str) -> Result<bool, String> {
    let device = device_for_controls(app, caller)?;
    let pid = app.state::<ScrcpyState>().processes.lock()
        .map_err(|e| e.to_string())?
        .get(&device).and_then(|child| child.id())
        .ok_or("scrcpy 会话已结束")?;
    let hwnd = crate::commands::find_window_by_pid(pid).ok_or("投屏窗口不可见")?;

    if action == "quick_status" { return Ok(is_quick_open(&device)); }
    if action == "quick_top_status" {
        return Ok((unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32
            & WS_EX_TOPMOST.0) != 0);
    }
    if action == "quick_toggle" {
        if !is_quick_open(&device) && is_inking(&device) {
            control_ink(app, &device, "ink_toggle").await?;
        }
        let mut guard = quick_sessions().lock().map_err(|e| e.to_string())?;
        if !guard.insert(device.clone()) { guard.remove(&device); }
        return Ok(guard.contains(&device));
    }
    if action.starts_with("quick_") {
        if !is_quick_open(&device) {
            return Err("Open the phone quick-control panel first".into());
        }
        if action == "quick_top_toggle" {
            return toggle_video_topmost(app, &device, hwnd);
        }
        if action == "quick_screen_off" {
            crate::commands::phone_toolbar_keyevent(&device, "quick_sleep").await?;
            return Ok(true);
        }
        if matches!(action, "quick_fullscreen" | "quick_rotate" | "quick_paste" | "quick_screen_off") {
            send_video_shortcut(hwnd, action)?;
        } else {
            crate::commands::phone_toolbar_keyevent(&device, action).await?;
        }
        return Ok(true);
    }
    if action.starts_with("ink_") {
        if action == "ink_toggle" || action == "ink_reset" {
            quick_sessions().lock().map_err(|e| e.to_string())?.remove(&device);
        }
        return control_ink(app, &device, action).await;
    }
    match action {
        "minimize" => {
            unsafe { ShowWindow(hwnd, SW_MINIMIZE); }
            Ok(is_inking(&device))
        }
        "close" => {
            quick_sessions().lock().map_err(|e| e.to_string())?.remove(&device);
            crate::commands::stop_scrcpy_impl(app.state::<ScrcpyState>(), device).await?;
            Ok(false)
        }
        "drag" => {
            begin_drag(app, device.clone(), hwnd)?;
            Ok(is_inking(&device))
        }
        _ => Err("Unsupported device-frame action".into()),
    }
}

fn begin_drag(app: &AppHandle, device: String, hwnd: HWND) -> Result<(), String> {
    let cursor = pointer().ok_or("Could not read mouse position")?;
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.map_err(|e| e.to_string())?;
    {
        let mut pending = drags().lock().map_err(|e| e.to_string())?;
        if !pending.insert(device.clone()) {
            return Ok(());
        }
    }

    // HWND is kept as an integer across await boundaries (Win32 HWND wrappers
    // carry pointer types that must not be held across Send async suspension).
    let hwnd_value = hwnd.0 as usize;
    let companions: Vec<(usize, i32, i32)> = [label(&device), toolbar_label(&device)]
        .iter()
        .filter_map(|name| app.get_webview_window(name))
        .filter_map(|window| window.hwnd().ok())
        .filter_map(|handle| {
            let mut current = RECT::default();
            unsafe { GetWindowRect(handle, &mut current) }.ok()?;
            Some((handle.0 as usize, current.left, current.top))
        })
        .collect();
    let start_x = rect.left;
    let start_y = rect.top;
    let dx = cursor.0 - rect.left;
    let dy = cursor.1 - rect.top;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(12));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_position: Option<(i32, i32)> = None;
        let mut native_batch_available = true;
        for _ in 0..2500 { // hard stop after 30 seconds even if mouse state fails
            interval.tick().await;
            let held = (unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8000) != 0;
            if !held { break; }
            let Some((x,y)) = pointer() else { continue };
            let video = HWND(hwnd_value as *mut _);
            let new_x = x-dx;
            let new_y = y-dy;
            if last_position == Some((new_x, new_y)) { continue; }
            last_position = Some((new_x, new_y));
            // One deferred Win32 batch eliminates the three sequential
            // compositor frames produced by separate SetWindowPos calls.
            // If Windows rejects batching (e.g. HWND destroyed during drag),
            // fall back to the previous direct native reposition logic.
            let mut moved = false;
            if native_batch_available {
                let batch_result = (|| -> windows::core::Result<()> {
                    let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
                    let mut batch = unsafe { BeginDeferWindowPos(1 + companions.len() as i32) }?;
                    batch = unsafe {
                        DeferWindowPos(batch, video, None, new_x, new_y, 0, 0, flags)
                    }?;
                    for &(window, left, top) in &companions {
                        batch = unsafe { DeferWindowPos(batch, HWND(window as *mut _), None,
                            left.saturating_add(new_x.saturating_sub(start_x)),
                            top.saturating_add(new_y.saturating_sub(start_y)),
                            0, 0, flags) }?;
                    }
                    unsafe { EndDeferWindowPos(batch) }
                })();
                if batch_result.is_ok() {
                    moved = true;
                } else {
                    native_batch_available = false;
                }
            }
            if !moved {
                let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
                let _ = unsafe { SetWindowPos(video, None, new_x, new_y, 0, 0, flags) };
                for &(window, left, top) in &companions {
                    let _ = unsafe { SetWindowPos(HWND(window as *mut _), None,
                        left.saturating_add(new_x.saturating_sub(start_x)),
                        top.saturating_add(new_y.saturating_sub(start_y)),
                        0, 0, flags) };
                }
            }
        }
        if let Ok(mut pending) = drags().lock() { pending.remove(&device); }
        // Return keyboard focus to the live video when the mouse is released.
        let video = HWND(hwnd_value as *mut _);
        let _ = unsafe { SetForegroundWindow(video) };
    });
    Ok(())
}

pub fn start(app: &AppHandle, device: &str, always_on_top: bool) -> Result<(), String> {
    if let Ok(mut quick) = quick_sessions().lock() { quick.remove(device); }
    if let Ok(mut states) = topmost_requests().lock() {
        states.insert(device.to_string(), always_on_top);
    }
    let shell_name = label(device);
    let controls_name = toolbar_label(device);
    if let Some(old) = app.get_webview_window(&shell_name) { let _ = old.close(); }
    if let Some(old) = app.get_webview_window(&controls_name) { let _ = old.close(); }

    let page_device = device.to_owned();
    let overlay = WebviewWindowBuilder::new(
        app, shell_name, WebviewUrl::App("frame.html".into())
    )
        .on_page_load(move |view, payload| {
            match payload.event() {
                PageLoadEvent::Started => {
                    if let Ok(mut ready) = ink_ready().lock() { ready.remove(&page_device); }
                    if let Ok(mut sessions) = ink_sessions().lock() { sessions.remove(&page_device); }
                    // JS mode resets on reload; never keep native cursor
                    // capture or presentation status referring to old state.
                    update_presentation_mode(view.app_handle(), &page_device, "off");
                    // On navigation/reload never leave an opaque input-capturing
                    // overlay ahead of native scrcpy when its canvas script resets.
                    let _ = view.set_ignore_cursor_events(true);
                },
                PageLoadEvent::Finished => {
                    if let Ok(mut ready) = ink_ready().lock() {
                        ready.insert(page_device.clone());
                    }
                }
            }
        })
        .title("Device frame")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(always_on_top)
        // Remain mouse-through at rest, but allow real pointer capture after
        // the user explicitly enables annotation in the separate tool rail.
        .focusable(true)
        .focused(false)
        .visible(false)
        .inner_size(430.0, 900.0)
        .build()
        .map_err(|e| format!("Cannot create phone frame: {e}"))?;

    if let Err(error) = overlay.set_ignore_cursor_events(true) {
        let _ = overlay.close();
        return Err(format!("Frame is not click-through: {error}"));
    }

    let toolbar = match WebviewWindowBuilder::new(
        app, controls_name, WebviewUrl::App("phone-controls.html".into())
    )
        .title("Phone controls")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(always_on_top)
        .visible(false)
        .inner_size(TOOLBAR_WIDTH as f64, TOOLBAR_HEIGHT as f64)
        .build()
    {
        Ok(w) => w,
        Err(e) => {
            let _ = overlay.close();
            return Err(format!("Cannot create phone controls: {e}"));
        }
    };

    tokio::spawn(follow(app.clone(), device.to_owned(), overlay, toolbar));
    Ok(())
}

pub fn close(app: &AppHandle, device: &str) {
    if let Ok(mut topmost) = topmost_requests().lock() { topmost.remove(device); }
    if let Ok(mut quick) = quick_sessions().lock() { quick.remove(device); }
    if let Ok(mut set) = ink_sessions().lock() { set.remove(device); }
    if let Ok(mut set) = ink_ready().lock() { set.remove(device); }
    update_presentation_mode(app, device, "off");
    if let Some(shell) = app.get_webview_window(&label(device)) { let _ = shell.close(); }
    if let Some(toolbar) = app.get_webview_window(&toolbar_label(device)) { let _ = toolbar.close(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presenter_modes_are_strict_and_off_does_not_capture_input() {
        assert_eq!(known_present_mode("off"), Some("off"));
        assert_eq!(known_present_mode("spotlight"), Some("spotlight"));
        assert_eq!(known_present_mode("magnifier"), Some("magnifier"));
        for value in ["", "snapshot", "live", "magnifier();alert(1)", "../run",
            "Off", "spotlight\n", "ink_toggle"] {
            assert_eq!(known_present_mode(value), None, "{value}");
        }
        assert_eq!(presentation_mode("device-unknown"), "off");
    }

    #[test]
    fn one_click_restore_is_part_of_strict_ink_action_namespace() {
        assert!("ink_reset".starts_with("ink_"));
        assert!("ink_clear".starts_with("ink_"));
        assert!(ink_action_script("ink_reset").is_none(),
            "reset must run native touch restoration, not just JS canvas.clear()");
    }

    #[test]
    fn annotation_scripts_allow_only_fixed_palette_and_edit_commands() {
        assert_eq!(ink_action_script("ink_color_red"), Some("window.scrcpyInk?.setColor('#ff5252')"));
        assert!(ink_action_script("ink_clear").is_some());
        assert!(ink_action_script("ink_size_9").is_some());
        assert!(ink_action_script("ink_color_' + alert(1)").is_none());
        assert!(ink_action_script("exit").is_none());
    }

    #[test]
    fn both_window_labels_are_stable_and_unique() {
        let a = label("192.168.2.226:5555");
        let b = toolbar_label("192.168.2.226:5555");
        assert!(a.starts_with("phone-frame-"));
        assert!(b.starts_with("phone-controls-"));
        assert_ne!(a, b);
        assert_eq!(a, label("192.168.2.226:5555"));
        assert_ne!(b, toolbar_label("192.168.2.226:43001"));
    }
}
