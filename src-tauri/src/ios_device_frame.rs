//! AirPlay video-attached iPad/iPhone shell (Windows only).
//!
//! The previous shell was another TOP-LEVEL window which chased the SDL video
//! HWND by desktop coordinates. Even perfect ClientToScreen arithmetic cannot
//! make two independently composited top-level windows move atomically.
//! Instead, re-parent our transparent/mouse-through WebView as a WS_CHILD of
//! the receiver's native SDL HWND. Windows itself moves them together. The
//! shell is a thin INWARD rim, preserving the real video HWND, focus, keyboard,
//! original renderer and audio. On any unsafe parent/geometry state hide it.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, GetMonitorInfoW,
    HGDIOBJ, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST, RGN_DIFF,
    SetWindowRgn,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetParent, GetWindowLongPtrW, GetWindowRect, GWL_EXSTYLE,
    GWL_STYLE, HWND_TOP, IsIconic, IsWindowVisible, SetParent, SetWindowLongPtrW,
    SetWindowPos, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    WS_CAPTION, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT, WS_POPUP,
    WS_THICKFRAME,
};

const LABEL: &str = "ios-device-bezel";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Extent { width: i32, height: i32 }

fn safe_extent(width: i32, height: i32) -> Option<Extent> {
    (width >= 160 && height >= 120 && width <= 9000 && height <= 9000)
        .then_some(Extent { width, height })
}

fn classify_ipad(style: &str, size: Extent) -> bool {
    match style {
        "device-ipad" => true,
        "device-iphone" => false,
        _ => (size.width.min(size.height) as f64 /
            size.width.max(size.height) as f64) >= 0.62,
    }
}

fn inset_px(ipad: bool, dpi: u32) -> i32 {
    let logical: i32 = if ipad { 12 } else { 14 };
    ((logical as i64 * dpi.clamp(96, 384) as i64 + 48) / 96)
        .clamp(8, 56) as i32
}

fn client_extent(hwnd: HWND) -> Option<Extent> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) }.ok()?;
    safe_extent(rect.right - rect.left, rect.bottom - rect.top)
}

pub(crate) fn receiver_client_size(pid: u32) -> Option<(u32, u32)> {
    let hwnd = crate::commands::find_window_by_pid(pid)?;
    let size = client_extent(hwnd)?;
    Some((size.width as u32, size.height as u32))
}

fn is_video_fullscreen(hwnd: HWND) -> bool {
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() { return false; }
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.0.is_null() { return false; }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return false;
    }
    let screen = info.rcMonitor;
    rect.left <= screen.left + 4 && rect.top <= screen.top + 4
        && rect.right >= screen.right - 4 && rect.bottom >= screen.bottom - 4
}

fn parent_matches(child: HWND, video: HWND) -> bool {
    unsafe { GetParent(child) }.ok() == Some(video)
}

/// Win32 relationship is the source of truth: never show a second top-level
/// overlay as fallback. If cross-process/DPI safety rejects SetParent, keep
/// mirroring unobstructed with the shell hidden.
fn attach_shell(child: HWND, video: HWND) -> Result<(), String> {
    if parent_matches(child, video) { return Ok(()); }
    let original = unsafe { GetWindowLongPtrW(child, GWL_STYLE) };
    let child_style = (original &
        !((WS_POPUP.0 | WS_CAPTION.0 | WS_THICKFRAME.0) as isize))
        | WS_CHILD.0 as isize;
    unsafe { SetWindowLongPtrW(child, GWL_STYLE, child_style); }
    let current = unsafe { GetWindowLongPtrW(child, GWL_STYLE) };
    if current & WS_CHILD.0 as isize == 0 {
        unsafe { SetWindowLongPtrW(child, GWL_STYLE, original); }
        return Err("Could not enable native child-window style".into());
    }
    let ex = unsafe { GetWindowLongPtrW(child, GWL_EXSTYLE) };
    unsafe {
        SetWindowLongPtrW(child, GWL_EXSTYLE,
            ex | WS_EX_NOACTIVATE.0 as isize | WS_EX_TRANSPARENT.0 as isize);
    }
    if let Err(error) = unsafe { SetParent(child, Some(video)) } {
        unsafe { SetWindowLongPtrW(child, GWL_STYLE, original); }
        return Err(format!("Windows refused iOS bezel parent attachment: {error}"));
    }
    if !parent_matches(child, video) {
        return Err("iOS bezel was not attached to the active AirPlay video".into());
    }
    Ok(())
}

/// A tiny OS-level rim INSIDE the parent video client. The middle is removed
/// using a GDI region so Chromium cannot ever paint over moving video pixels.
/// We do not touch the receiver HWND region or alter any AirPlay frames.
fn hollow_child(hwnd: HWND, size: Extent, inset: i32, ipad: bool, dpi: u32)
    -> Result<(), String>
{
    let radius = ((if ipad { 27.0 } else { 39.0 }) *
        f64::from(dpi) / 96.0).round() as i32;
    let inner_radius = ((if ipad { 15.0 } else { 25.0 }) *
        f64::from(dpi) / 96.0).round() as i32;
    let outer = unsafe {
        CreateRoundRectRgn(0, 0, size.width + 1, size.height + 1,
            radius * 2, radius * 2)
    };
    if outer.0.is_null() { return Err("Could not create iOS outer rim".into()); }
    let inner = unsafe {
        CreateRoundRectRgn(inset, inset,
            size.width - inset + 1, size.height - inset + 1,
            inner_radius * 2, inner_radius * 2)
    };
    let rim = unsafe { CreateRectRgn(0, 0, 0, 0) };
    let valid = !inner.0.is_null() && !rim.0.is_null()
        && unsafe { CombineRgn(Some(rim), Some(outer), Some(inner), RGN_DIFF) }.0 >= 2;
    let _ = unsafe { DeleteObject(HGDIOBJ(outer.0)) };
    if !inner.0.is_null() {
        let _ = unsafe { DeleteObject(HGDIOBJ(inner.0)) };
    }
    if !valid {
        if !rim.0.is_null() {
            let _ = unsafe { DeleteObject(HGDIOBJ(rim.0)) };
        }
        return Err("Could not create hollow iOS bezel".into());
    }
    if unsafe { SetWindowRgn(hwnd, Some(rim), true) } == 0 {
        let _ = unsafe { DeleteObject(HGDIOBJ(rim.0)) };
        return Err("Windows rejected hollow iOS bezel".into());
    }
    Ok(())
}

pub fn close(app: &AppHandle) {
    app.state::<crate::ios_airplay::IosAirplayState>().frame_attached
        .store(false, Ordering::Release);
    if let Some(old) = app.get_webview_window(LABEL) {
        let _ = old.close();
    }
}

pub fn start(app: &AppHandle, pid: u32) -> Result<(), String> {
    close(app);
    let view = WebviewWindowBuilder::new(
        app, LABEL, WebviewUrl::App("ios-bezel.html".into()))
        .title("MirroLeap iOS device bezel")
        .background_color(tauri::window::Color(0,0,0,0))
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .inner_size(400.0,700.0)
        .build()
        .map_err(|e|format!("Cannot create iOS bezel: {e}"))?;
    if let Err(error) = view.set_ignore_cursor_events(true) {
        let _ = view.close();
        return Err(format!("iOS bezel must be mouse-through: {error}"));
    }
    tokio::spawn(follow(app.clone(), view, pid));
    Ok(())
}

async fn follow(app: AppHandle, view: WebviewWindow, pid: u32) {
    tokio::time::sleep(Duration::from_millis(350)).await;
    let own = match view.hwnd() {
        Ok(hwnd) => hwnd.0 as usize, Err(_) => return
    };
    let mut last: Option<(Extent, bool, u32)> = None;
    let mut visible = false;
    let mut last_style_update = Instant::now() - Duration::from_secs(1);
    let mut ticker = tokio::time::interval(Duration::from_millis(80));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        if app.get_webview_window(LABEL).and_then(|v|v.hwnd().ok())
            .map(|hwnd|hwnd.0 as usize) != Some(own) { break; }
        let state = app.state::<crate::ios_airplay::IosAirplayState>();
        let active_pid = state.child.lock().ok()
            .and_then(|v|v.as_ref().map(|c|c.id()));
        if active_pid != Some(pid) { break; }
        let style = state.window_style.lock().ok()
            .map(|v|v.clone()).unwrap_or_default();
        let video = crate::commands::find_window_by_pid(pid);
        let Some(video) = video else {
            if visible { let _=view.hide(); visible=false; }
            state.frame_attached.store(false, Ordering::Release);
            continue;
        };
        if !style.starts_with("device-")
            || !unsafe { IsWindowVisible(video) }.as_bool()
            || unsafe { IsIconic(video) }.as_bool()
            || is_video_fullscreen(video) {
            if visible { let _=view.hide(); visible=false; }
            state.frame_attached.store(false, Ordering::Release);
            continue;
        }
        let Some(size) = client_extent(video) else { continue; };
        // Keep HWND values scoped to the current tick: raw HWND pointers are
        // intentionally never held across .await on the multithread runtime.
        let child = HWND(own as *mut _);
        if !parent_matches(child, video) {
            if visible { let _=view.hide(); visible=false; }
            if let Err(error) = attach_shell(child, video) {
                eprintln!("[iOS bezel] {error}");
                state.frame_attached.store(false, Ordering::Release);
                break; // No detached-window fallback.
            }
            last = None;
        }
        let dpi = unsafe { GetDpiForWindow(video) }.clamp(96, 384);
        let ipad = classify_ipad(&style, size);
        let inset = inset_px(ipad, dpi);
        let layout = (size, ipad, dpi);
        // Child-local (0,0), not screen coordinates. Moving the SDL parent
        // moves the child atomically, even between these 80-ms health ticks.
        if last != Some(layout) {
            if unsafe {
                SetWindowPos(child, Some(HWND_TOP), 0, 0,
                    size.width, size.height, SWP_NOACTIVATE | SWP_FRAMECHANGED)
            }.is_err() {
                if visible { let _=view.hide(); visible=false; }
                state.frame_attached.store(false, Ordering::Release);
                continue;
            }
            if client_extent(child) != Some(size) || !parent_matches(child,video) {
                if visible { let _=view.hide(); visible=false; }
                state.frame_attached.store(false, Ordering::Release);
                continue;
            }
            if hollow_child(child, size, inset, ipad, dpi).is_err() {
                if visible { let _=view.hide(); visible=false; }
                state.frame_attached.store(false, Ordering::Release);
                continue;
            }
            last = Some(layout);
            last_style_update = Instant::now() - Duration::from_secs(1);
        }
        if last_style_update.elapsed() > Duration::from_millis(700) {
            let _ = view.eval(format!(
                "window.mirroleapIosSkin?.('{}','{}',{})",
                if ipad {"ipad"}else{"iphone"},
                if size.width>size.height {"landscape"}else{"portrait"},
                inset as f64 * 96.0 / f64::from(dpi)
            ));
            last_style_update = Instant::now();
        }
        if !visible {
            if view.show().is_ok() { visible=true; }
        }
        if visible && parent_matches(child,video) && client_extent(child)==Some(size) {
            state.frame_attached.store(true, Ordering::Release);
        } else {
            if visible { let _=view.hide(); visible=false; }
            state.frame_attached.store(false, Ordering::Release);
            last = None;
        }
    }
    app.state::<crate::ios_airplay::IosAirplayState>().frame_attached
        .store(false, Ordering::Release);
    if app.get_webview_window(LABEL).and_then(|v|v.hwnd().ok())
        .map(|hwnd|hwnd.0 as usize)==Some(own) { let _=view.close(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_valid_native_video_client_sizes_are_allowed() {
        assert_eq!(safe_extent(1024,1366), Some(Extent{width:1024,height:1366}));
        for (w,h) in [(0,120),(100,100),(9200,800),(-1,900)] {
            assert_eq!(safe_extent(w,h),None);
        }
    }
    #[test]
    fn device_classification_and_rim_are_bounded_for_all_dpi() {
        let ipad=Extent{width:1000,height:1380};
        let iphone=Extent{width:400,height:865};
        assert!(classify_ipad("device-auto",ipad));
        assert!(!classify_ipad("device-auto",iphone));
        assert!(classify_ipad("device-ipad",iphone));
        assert!(!classify_ipad("device-iphone",ipad));
        assert_eq!(inset_px(true,96),12);
        assert_eq!(inset_px(true,144),18);
        assert_eq!(inset_px(false,192),28);
        assert!(inset_px(true,384)<=56);
    }
    #[test]
    fn attached_rim_uses_child_local_geometry_not_desktop_coordinates() {
        // Moving a parent from (-1200,200) to (750,320) changes neither
        // child position nor the hollow bezel aperture; desktop offsets
        // are no longer part of the placement equation.
        let parent=Extent{width:1024,height:1366};
        let inset=inset_px(true,96);
        assert_eq!(parent.width - 2*inset,1000);
        assert_eq!(parent.height - 2*inset,1342);
    }
}
