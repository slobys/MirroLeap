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
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, CreateRoundRectRgn, DeleteObject, HGDIOBJ, SetWindowRgn,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindow,
    GetWindowRect, GW_HWNDPREV,
    IsIconic, IsWindowVisible, SetForegroundWindow, SetWindowPos, ShowWindow,
    HWND_TOP, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SW_MINIMIZE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
};

use crate::frame_layout::{
    expand_screen, is_frame_edge_hover, native_clip_radius, toolbar_placement,
    FrameRect, ScreenRect, ToolbarLayout, TOOLBAR_HEIGHT, TOOLBAR_WIDTH,
};
use crate::ScrcpyState;

static ACTIVE_DRAGS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static INK_ENABLED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
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
/// Fix: clip LESS aggressively than the SVG hole. Both radii are 3 physical
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
    let mut controls_y_offset: Option<i32> = None;
    let mut last_nearby: Option<Instant> = None;
    let mut video_was_unavailable = true;
    let mut restored_at: Option<Instant> = None;
    let mut previous_layout: Option<ToolbarLayout> = None;
    let mut layout_synced_at: Option<Instant> = None;
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
            controls_y_offset = None;
            last_nearby = None;
            applied_orientation = None;
            previous_layout = None;
            layout_synced_at = None;
            video_was_unavailable = true;
            continue;
        };

        let hwnd = captured.hwnd;
        let frame = captured.frame;
        let screen = captured.screen;
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
        let inking = is_inking(&device);
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
                shell_visible = true;
                previous_frame = None;
                // Re-establish HWND z-order AFTER making WebView visible.
                if let Err(error) = place_overlay(&overlay, hwnd, frame) {
                    let _ = app.emit("scrcpy-log", format!("[FRAME] Retrying restored shell: {error}"));
                    shell_visible = false;
                }
            } else {
                shell_visible = false;
            }
        }
        let shell_behind_video = overlay_hwnd.map(|shell| {
            unsafe { GetWindow(hwnd, GW_HWNDPREV) }.map(|above| above != shell).unwrap_or(true)
        }).unwrap_or(true);
        if shell_visible && (previous_frame != Some(frame) || shell_behind_video || foreground == hwnd || (inking && active)) {
            match place_overlay(&overlay, hwnd, frame) {
                Ok(()) => previous_frame = Some(frame),
                Err(error) => {
                    // Do not destroy the skin or toolbar just because Windows
                    // temporarily refuses one native window positioning call.
                    let _ = app.emit("scrcpy-log", format!("[FRAME] Position will retry: {error}"));
                    previous_frame = None;
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
        let wanted = close_to_edge || dragging || inking || inside_controls || recent_restore;
        if wanted { last_nearby = Some(Instant::now()); }
        let lingering = last_nearby.map(|t| t.elapsed() < Duration::from_millis(550))
            .unwrap_or(false);
        let show_controls = wanted || (active && lingering);

        if show_controls {
            let desktop = virtual_desktop();
            // WebView is laid out in CSS DIPs. Use the SDL monitor's physical
            // DPI to size the native window (DPI can change when dragging to
            // a different monitor).
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
            if controls_y_offset.is_none() {
                let mouse_y = cursor.filter(|(_, y)| *y >= frame.y && *y < frame.y + frame.height)
                    .map(|(_, y)| y).unwrap_or(frame.y + 100);
                let narrow_height = (TOOLBAR_HEIGHT as i64 * dpi as i64 / 96) as i32;
                let top = mouse_y - narrow_height / 2;
                let compact = toolbar_placement(frame, top, desktop, false, dpi);
                controls_y_offset = Some(compact.rect.y - frame.y);
            }
            // The top of the compact rail is stable as the phone moves; the
            // expanded flyout is allowed to move UP to fit within the desktop.
            let top = frame.y + controls_y_offset.unwrap_or(0);
            let placement = toolbar_placement(frame, top, desktop, inking, dpi);
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
                // Always place AFTER show, immediately above the visible shell,
                // not HWND_TOP above an unrelated foreground browser.
                if let Err(error) = place_toolbar(&toolbar, &overlay, rect) {
                    let _ = app.emit("scrcpy-log",
                        format!("[FRAME] Repositioning toolbar on next tick: {error}"));
                    controls_rect = None;
                } else {
                    controls_rect = Some(rect);
                }
                let refresh_layout = layout_synced_at.map(|t| t.elapsed() > Duration::from_secs(2))
                    .unwrap_or(true);
                if newly_shown || previous_layout != Some(placement.layout) || refresh_layout {
                    let script = format!("window.scrcpyControls?.setLayout('{}'); window.scrcpyControls?.sync()", placement.layout.css_name());
                    if toolbar.eval(script).is_ok() {
                        previous_layout = Some(placement.layout);
                        layout_synced_at = Some(Instant::now());
                    }
                }
            }
        } else {
            if controls_visible { let _ = toolbar.hide(); controls_visible = false; }
            controls_rect = None;
            controls_y_offset = None;
            previous_layout = None;
            layout_synced_at = None;
        }
    }

    if let Ok(mut set) = ink_sessions().lock() { set.remove(&device); }
    if let Ok(mut set) = ink_ready().lock() { set.remove(&device); }
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
        console.error('[ScrcpyGUI Ink] Canvas operation failed', error);
      }}
      const call = window.__TAURI__?.core?.invoke;
      if (call) {{
        void call('device_frame_ack', {{ token: {}, success }})
          .catch(error => console.error('[ScrcpyGUI Ink] Acknowledgement failed', error));
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
pub async fn control(app: &AppHandle, caller: &tauri::Window, action: &str) -> Result<bool, String> {
    let device = device_for_controls(app, caller)?;
    let pid = app.state::<ScrcpyState>().processes.lock()
        .map_err(|e| e.to_string())?
        .get(&device).and_then(|child| child.id())
        .ok_or("scrcpy 会话已结束")?;
    let hwnd = crate::commands::find_window_by_pid(pid).ok_or("投屏窗口不可见")?;

    if action.starts_with("ink_") {
        return control_ink(app, &device, action).await;
    }
    match action {
        "minimize" => {
            unsafe { ShowWindow(hwnd, SW_MINIMIZE); }
            Ok(is_inking(&device))
        }
        "close" => {
            crate::commands::stop_scrcpy(app.state::<ScrcpyState>(), device).await?;
            Ok(false)
        }
        "drag" => {
            begin_drag(device.clone(), hwnd)?;
            Ok(is_inking(&device))
        }
        _ => Err("Unsupported device-frame action".into()),
    }
}

fn begin_drag(device: String, hwnd: HWND) -> Result<(), String> {
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
    let dx = cursor.0 - rect.left;
    let dy = cursor.1 - rect.top;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(12));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        for _ in 0..2500 { // hard stop after 30 seconds even if mouse state fails
            interval.tick().await;
            let held = (unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8000) != 0;
            if !held { break; }
            let Some((x,y)) = pointer() else { continue };
            let video = HWND(hwnd_value as *mut _);
            let _ = unsafe {
                SetWindowPos(video, None, x-dx, y-dy, 0, 0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE)
            };
        }
        if let Ok(mut pending) = drags().lock() { pending.remove(&device); }
        // Return keyboard focus to the live video when the mouse is released.
        let video = HWND(hwnd_value as *mut _);
        let _ = unsafe { SetForegroundWindow(video) };
    });
    Ok(())
}

pub fn start(app: &AppHandle, device: &str, always_on_top: bool) -> Result<(), String> {
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
    if let Ok(mut set) = ink_sessions().lock() { set.remove(device); }
    if let Ok(mut set) = ink_ready().lock() { set.remove(device); }
    if let Some(shell) = app.get_webview_window(&label(device)) { let _ = shell.close(); }
    if let Some(toolbar) = app.get_webview_window(&toolbar_label(device)) { let _ = toolbar.close(); }
}

#[cfg(test)]
mod tests {
    use super::*;

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
