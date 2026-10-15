//! Windows-only click-through phone frame with an overscanned native clip.
//!
//! Three independent windows:
//!   - scrcpy SDL window: native rounded mask slightly wider than SVG screen aperture.
//!   - transparent SVG window: antialiased metal bezel covers the hard GDI edge.
//!   - tiny, interactive hover toolbar: drag, minimize, close.
//!
//! The toolbar is a separate native window because making the main full-screen
//! overlay mouse-active would intercept scrcpy taps and swipes.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, CreateRoundRectRgn, DeleteObject, HGDIOBJ, SetWindowRgn,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindowRect,
    IsIconic, IsWindowVisible, IsZoomed, SetForegroundWindow, SetWindowPos, ShowWindow,
    HWND_TOP, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SW_MINIMIZE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
};

use crate::frame_layout::{
    expand_screen, is_frame_edge_hover, native_clip_radius, toolbar_position,
    FrameRect, ScreenRect, TOOLBAR_HEIGHT, ANNOTATION_TOOLBAR_HEIGHT, TOOLBAR_WIDTH,
};
use crate::ScrcpyState;

static ACTIVE_DRAGS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static INK_ENABLED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

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
    if !unsafe { IsWindowVisible(hwnd) }.as_bool()
        || unsafe { IsIconic(hwnd) }.as_bool()
        || unsafe { IsZoomed(hwnd) }.as_bool()
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

fn place_overlay(overlay: &WebviewWindow, video: HWND, frame: FrameRect) -> Result<(), String> {
    let shell = overlay.hwnd().map_err(|e| e.to_string())?;
    let foreground = unsafe { GetForegroundWindow() } == video;
    let order = if foreground { Some(HWND_TOP) } else { None };
    let flags = if foreground { SWP_NOACTIVATE } else { SWP_NOACTIVATE | SWP_NOZORDER };
    unsafe { SetWindowPos(shell, order, frame.x, frame.y, frame.width, frame.height, flags) }
        .map_err(|e| format!("Could not position device shell: {e}"))
}

fn place_toolbar(toolbar: &WebviewWindow, rect: ScreenRect) -> Result<(), String> {
    let hwnd = toolbar.hwnd().map_err(|e| e.to_string())?;
    unsafe {
        SetWindowPos(hwnd, Some(HWND_TOP), rect.x, rect.y, rect.width, rect.height, SWP_NOACTIVATE)
    }.map_err(|e| format!("Could not position hover controls: {e}"))
}

async fn follow(app: AppHandle, device: String, overlay: WebviewWindow, toolbar: WebviewWindow) {
    let mut shell_visible = false;
    let mut controls_visible = false;
    let mut previous_frame: Option<FrameRect> = None;
    let mut clipped_pid: Option<u32> = None;
    let mut last_client_size: Option<(u32, i32, i32)> = None;
    let mut controls_rect: Option<ScreenRect> = None;
    let mut controls_y_offset: Option<i32> = None;
    let mut last_nearby: Option<Instant> = None;
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;
        let pid = app.state::<ScrcpyState>()
            .processes.lock().ok()
            .and_then(|processes| processes.get(&device).and_then(|child| child.id()));
        let Some(pid) = pid else { break };

        let Some(captured) = capture_screen(pid) else {
            // Landscape, minimized or maximized: restore the original SDL area.
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
            continue;
        };

        let hwnd = captured.hwnd;
        let frame = captured.frame;
        let screen = captured.screen;

        // Avoid redundant SetWindowRgn calls while the phone is stationary.
        // Re-create it after scrcpy rotation, resize, or codec-retry PID change.
        if last_client_size != Some((pid, screen.width, screen.height)) {
            if let Err(error) = apply_native_corner_clip(hwnd, screen) {
                let _ = app.emit("scrcpy-log",
                    format!("[FRAME] Native screen corners unavailable: {error}"));
                break;
            }
            clipped_pid = Some(pid);
            last_client_size = Some((pid, screen.width, screen.height));
        }

        if previous_frame != Some(frame) || unsafe { GetForegroundWindow() } == hwnd {
            if place_overlay(&overlay, hwnd, frame).is_err() { break; }
            previous_frame = Some(frame);
        }
        if !shell_visible {
            if overlay.show().is_err() { break; }
            shell_visible = true;
            previous_frame = None;
        }

        // Keep phone interactions native. Detect edge hover at the OS level,
        // then show ONLY a small interactive popover to the right of the phone.
        let cursor = pointer();
        let toolbar_hwnd = toolbar.hwnd().ok();
        let overlay_hwnd = overlay.hwnd().ok();
        let foreground = unsafe { GetForegroundWindow() };
        let active = foreground == hwnd
            || toolbar_hwnd == Some(foreground)
            || overlay_hwnd == Some(foreground);
        let dragging = is_dragging(&device);
        let inking = is_inking(&device);
        let close_to_edge = cursor.map(|(x,y)| is_frame_edge_hover(x,y,frame)).unwrap_or(false);
        let inside_controls = match (cursor, controls_rect) {
            (Some((x,y)), Some(r)) => inside(x,y,r,12),
            _ => false,
        };
        let wanted = active && (close_to_edge || inside_controls || dragging || inking);
        if wanted { last_nearby = Some(Instant::now()); }
        let lingering = last_nearby.map(|t| t.elapsed() < Duration::from_millis(420))
            .unwrap_or(false);
        let show_controls = active && (wanted || lingering);

        if show_controls {
            if controls_rect.is_none() {
                let desktop = virtual_desktop();
                let y = cursor.map(|(_,y)| y).unwrap_or(frame.y + 80);
                let mut rect = toolbar_position(frame, y, desktop);
                rect.height = if inking { ANNOTATION_TOOLBAR_HEIGHT } else { TOOLBAR_HEIGHT };
                rect.y = rect.y.clamp(desktop.y,
                    (desktop.y + desktop.height - rect.height).max(desktop.y));
                controls_y_offset = Some(rect.y - frame.y);
                controls_rect = Some(rect);
            }
            // Follow the *moving phone*, but don't make the toolbar chase the
            // pointer vertically while the user tries to press a button.
            if let Some(mut rect) = controls_rect {
                let desktop = virtual_desktop();
                rect.height = if inking { ANNOTATION_TOOLBAR_HEIGHT } else { TOOLBAR_HEIGHT };
                let left_gap = frame.x - 8 - TOOLBAR_WIDTH;
                let right_gap = frame.x + frame.width + 8;
                let prefer_right = right_gap + TOOLBAR_WIDTH <= desktop.x + desktop.width;
                rect.x = if prefer_right { right_gap } else { left_gap };
                rect.x = rect.x.clamp(desktop.x, (desktop.x+desktop.width-rect.width).max(desktop.x));
                rect.y = (frame.y + controls_y_offset.unwrap_or(rect.y - frame.y))
                    .clamp(desktop.y, (desktop.y + desktop.height - rect.height).max(desktop.y));
                if place_toolbar(&toolbar, rect).is_err() { break; }
                controls_rect = Some(rect);
            }
            if !controls_visible {
                if toolbar.show().is_err() { break; }
                controls_visible = true;
            }
        } else {
            if controls_visible { let _ = toolbar.hide(); controls_visible = false; }
            controls_rect = None;
            controls_y_offset = None;
        }
        let _ = last_toolbar_size; // tracks current width/height when palette opens
    }

    if let Ok(mut set) = ink_sessions().lock() { set.remove(&device); }
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

fn control_ink(app: &AppHandle, device: &str, action: &str) -> Result<(), String> {
    let overlay = app.get_webview_window(&label(device))
        .ok_or("Live phone frame is not available for annotation")?;
    if action == "ink_toggle" {
        let turn_on = !is_inking(device);
        if turn_on {
            // Turn on hit testing only in drawing mode; outside this mode the
            // entire visual overlay remains click-through to the SDL window.
            overlay.eval("window.scrcpyInk?.setEnabled(true)")
                .map_err(|e| format!("Cannot activate drawing surface: {e}"))?;
            overlay.set_ignore_cursor_events(false)
                .map_err(|e| format!("Cannot enable drawing pointer input: {e}"))?;
            ink_sessions().lock().map_err(|e| e.to_string())?.insert(device.to_owned());
        } else {
            overlay.eval("window.scrcpyInk?.setEnabled(false)")
                .map_err(|e| format!("Cannot close drawing surface: {e}"))?;
            overlay.set_ignore_cursor_events(true)
                .map_err(|e| format!("Cannot restore scrcpy touch input: {e}"))?;
            ink_sessions().lock().map_err(|e| e.to_string())?.remove(device);
        }
        return Ok(());
    }
    if !is_inking(device) {
        return Err("Enable the annotation pen before using drawing tools".into());
    }
    let code = ink_action_script(action).ok_or("Unsupported annotation action")?;
    overlay.eval(code).map_err(|e| format!("Cannot apply annotation tool: {e}"))
}

/// Called only by the dedicated phone-controls-* toolbar window.
/// The full-screen visual overlay receives mouse input only while annotating.
pub async fn control(app: &AppHandle, caller: &tauri::Window, action: &str) -> Result<(), String> {
    let device = device_for_controls(app, caller)?;
    let pid = app.state::<ScrcpyState>().processes.lock()
        .map_err(|e| e.to_string())?
        .get(&device).and_then(|child| child.id())
        .ok_or("scrcpy session has ended")?;
    let hwnd = crate::commands::find_window_by_pid(pid).ok_or("scrcpy window is not visible")?;

    if action.starts_with("ink_") {
        return control_ink(app, &device, action);
    }
    match action {
        "minimize" => {
            unsafe { ShowWindow(hwnd, SW_MINIMIZE); }
            Ok(())
        }
        "close" => crate::commands::stop_scrcpy(app.state::<ScrcpyState>(), device).await,
        "drag" => begin_drag(device, hwnd),
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

    let overlay = WebviewWindowBuilder::new(
        app, shell_name, WebviewUrl::App("frame.html".into())
    )
        .title("Device frame")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(always_on_top)
        .focusable(false)
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
    if let Some(shell) = app.get_webview_window(&label(device)) { let _ = shell.close(); }
    if let Some(toolbar) = app.get_webview_window(&toolbar_label(device)) { let _ = toolbar.close(); }
}

#[cfg(test)]
mod tests {
    use super::*;

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
