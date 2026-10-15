//! Windows-only click-through device frame around the *live* scrcpy SDL window.
//!
//! This is NOT a screenshot or a second video stream: scrcpy continues to draw
//! and receive mouse/keyboard events unmodified. A small transparent WebView2
//! window paints only the metallic bezel, button shapes and camera island.
//! The whole overlay ignores mouse events so clicks reach scrcpy below it.
//!
//! The frame stays in the normal Windows z-order (not globally TOPMOST) to
//! avoid obscuring unrelated applications. It follows the SDL client rect,
//! automatically hides while minimized/maximized and closes with the session.

use std::hash::{Hash, Hasher};
use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetForegroundWindow, IsIconic, IsWindowVisible, IsZoomed, SetWindowPos,
    HWND_TOP, SWP_NOACTIVATE, SWP_NOZORDER,
};

use crate::frame_layout::{expand_screen, FrameRect, ScreenRect};
use crate::ScrcpyState;

fn label(device: &str) -> String {
    // A stable short label avoids special characters in network/mDNS serials.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    device.hash(&mut hasher);
    format!("phone-frame-{:016x}", hasher.finish())
}

fn screen_rect(pid: u32) -> Option<(HWND, FrameRect)> {
    let hwnd = crate::commands::find_window_by_pid(pid)?;
    if !unsafe { IsWindowVisible(hwnd) }.as_bool()
        || unsafe { IsIconic(hwnd) }.as_bool()
        || unsafe { IsZoomed(hwnd) }.as_bool()
    {
        return None;
    }

    let mut rect = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rect) }.is_err() {
        return None;
    }
    let mut origin = POINT::default();
    if !unsafe { ClientToScreen(hwnd, &mut origin) }.as_bool() {
        return None;
    }
    let frame = expand_screen(ScreenRect {
        x: origin.x,
        y: origin.y,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    })?;
    Some((hwnd, frame))
}

fn place_overlay(overlay: &WebviewWindow, video: HWND, frame: FrameRect) -> Result<(), String> {
    let shell = overlay.hwnd().map_err(|e| format!("Missing frame window handle: {e}"))?;
    // Scrcpy receives keyboard focus and ordinary mouse input. When it is the
    // foreground window, raise the click-through shell *without activating it*.
    // Otherwise preserve z-order so other applications may cover both windows.
    let foreground = unsafe { GetForegroundWindow() } == video;
    let order = if foreground { Some(HWND_TOP) } else { None };
    let flags = if foreground { SWP_NOACTIVATE } else { SWP_NOACTIVATE | SWP_NOZORDER };

    unsafe { SetWindowPos(shell, order, frame.x, frame.y, frame.width, frame.height, flags) }
        .map_err(|e| format!("Could not position device frame: {e}"))
}

async fn follow(app: AppHandle, device: String, overlay: WebviewWindow) {
    let mut was_visible = false;
    let mut last_rect: Option<FrameRect> = None;
    let mut ticker = tokio::time::interval(Duration::from_millis(60));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;
        let pid = app.state::<ScrcpyState>()
            .processes.lock().ok()
            .and_then(|processes| processes.get(&device).and_then(|child| child.id()));
        let Some(pid) = pid else { break; };

        let Some((scrcpy_hwnd, rect)) = screen_rect(pid) else {
            if was_visible {
                let _ = overlay.hide();
                was_visible = false;
                last_rect = None;
            }
            continue;
        };

        if last_rect != Some(rect) || unsafe { GetForegroundWindow() } == scrcpy_hwnd {
            if place_overlay(&overlay, scrcpy_hwnd, rect).is_err() {
                break;
            }
            last_rect = Some(rect);
        }
        if !was_visible {
            if overlay.show().is_err() { break; }
            was_visible = true;
            // .show() may raise scrcpy above the overlay. Reorder on next tick.
            last_rect = None;
        }
    }

    let _ = overlay.close();
}

pub fn start(app: &AppHandle, device: &str, always_on_top: bool) -> Result<(), String> {
    let name = label(device);
    if let Some(existing) = app.get_webview_window(&name) {
        let _ = existing.close();
    }

    let overlay = WebviewWindowBuilder::new(app, name, WebviewUrl::App("frame.html".into()))
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
        .map_err(|e| format!("Unable to create phone frame overlay: {e}"))?;

    if let Err(error) = overlay.set_ignore_cursor_events(true) {
        let _ = overlay.close();
        return Err(format!("Unable to enable click-through frame: {error}"));
    }

    tokio::spawn(follow(app.clone(), device.to_owned(), overlay));
    Ok(())
}

pub fn close(app: &AppHandle, device: &str) {
    if let Some(overlay) = app.get_webview_window(&label(device)) {
        let _ = overlay.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_are_stable_and_safe_for_network_serials() {
        let a = label("192.168.2.226:5555");
        assert!(a.starts_with("phone-frame-"));
        assert_eq!(a, label("192.168.2.226:5555"));
        assert_ne!(a, label("adb-phone._adb-tls-connect._tcp"));
        assert_eq!(a.len(), "phone-frame-".len() + 16);
    }
}
