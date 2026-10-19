//! Win32/DWM compositor-state guard for the external scrcpy SDL window and
//! our transparent WebView2 phone shell. Kept separate from GDI HRGN clipping.
//!
//! scrcpy's SDL backend toggles DWMWA_WINDOW_CORNER_PREFERENCE and
//! DWMWA_BORDER_COLOR when entering/leaving fullscreen on Windows 11. SDL can
//! restore the OS defaults *after* our phone-shaped region is in place,
//! causing DWM's non-client border/shadow to remain square outside the region.
//! Use native attributes to opt out of ALL DWM-drawn chrome on these two
//! BORDERLESS windows; the SVG + GDI region provide the actual phone shape.
//! Windows 10 does not support the Windows 11 attributes: failure is benign.

use std::ffi::c_void;
use std::mem::size_of;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmSetWindowAttribute,
    DWMWA_NCRENDERING_ENABLED, DWMWA_NCRENDERING_POLICY, DWMNCRP_DISABLED,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassLongPtrW, GetWindowLongPtrW, GetWindowRect, GCL_STYLE, CS_DROPSHADOW,
    GWL_STYLE, GWL_EXSTYLE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeSnapshot {
    /// None = unsupported by current Windows or access denied; not "default".
    pub corner: Option<i32>,
    pub border: Option<u32>,
    pub nc_rendering: Option<i32>,
    pub class_shadow: bool,
    pub style: usize,
    pub ex_style: usize,
    pub outer: (i32, i32, i32, i32),
    pub dwm_frame: Option<(i32, i32, i32, i32)>,
}

fn attr_i32(hwnd: HWND, attribute: windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE) -> Option<i32> {
    let mut value = 0i32;
    unsafe {
        DwmGetWindowAttribute(hwnd, attribute,
            (&mut value as *mut i32).cast::<c_void>(), size_of::<i32>() as u32)
    }.ok().map(|_| value)
}

fn attr_u32(hwnd: HWND, attribute: windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE) -> Option<u32> {
    let mut value = 0u32;
    unsafe {
        DwmGetWindowAttribute(hwnd, attribute,
            (&mut value as *mut u32).cast::<c_void>(), size_of::<u32>() as u32)
    }.ok().map(|_| value)
}

fn frame_bounds(hwnd: HWND) -> Option<(i32,i32,i32,i32)> {
    let mut rc = RECT::default();
    unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut rc as *mut RECT).cast::<c_void>(), size_of::<RECT>() as u32)
    }.ok().map(|_| (rc.left,rc.top,rc.right,rc.bottom))
}

pub fn snapshot(hwnd: HWND) -> ChromeSnapshot {
    let class_style = unsafe { GetClassLongPtrW(hwnd, GCL_STYLE) };
    let mut rect = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
    ChromeSnapshot {
        corner: attr_i32(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE),
        border: attr_u32(hwnd, DWMWA_BORDER_COLOR),
        nc_rendering: attr_i32(hwnd, DWMWA_NCRENDERING_ENABLED),
        class_shadow: class_style & CS_DROPSHADOW.0 as usize != 0,
        style: unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as usize,
        ex_style: unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as usize,
        outer: (rect.left,rect.top,rect.right,rect.bottom),
        dwm_frame: frame_bounds(hwnd),
    }
}

pub fn needs_chrome_repair(corner: Option<i32>, border: Option<u32>, nc: Option<i32>) -> bool {
    corner.is_some_and(|value| value != DWMWCP_DONOTROUND.0)
        || border.is_some_and(|value| value != DWMWA_COLOR_NONE)
        || nc == Some(1)
}

pub struct ChromeRepair {
    pub before: ChromeSnapshot,
    pub after: ChromeSnapshot,
    pub failed: Vec<&'static str>,
}

/// Best-effort opt out of DWM-owned non-client painting. The video frame is
/// already --window-borderless and the WebView2 shell is .decorations(false),
/// so this never hides a legitimate MirroLeap system title bar. Unsupported
/// Windows 11 attributes on Windows 10 are intentionally nonfatal.
pub fn neutralize(hwnd: HWND) -> ChromeRepair {
    let before = snapshot(hwnd);
    let mut failed = Vec::new();

    let nc = DWMNCRP_DISABLED.0;
    if before.nc_rendering != Some(0) && unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_NCRENDERING_POLICY,
            (&nc as *const i32).cast::<c_void>(), size_of::<i32>() as u32)
    }.is_err() {
        failed.push("nonclient");
    }

    let corners = DWMWCP_DONOTROUND.0;
    if before.corner.is_some_and(|value| value != DWMWCP_DONOTROUND.0) && unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE,
            (&corners as *const i32).cast::<c_void>(), size_of::<i32>() as u32)
    }.is_err() {
        failed.push("corners");
    }

    let color = DWMWA_COLOR_NONE;
    if before.border.is_some_and(|value| value != DWMWA_COLOR_NONE) && unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR,
            (&color as *const u32).cast::<c_void>(), size_of::<u32>() as u32)
    }.is_err() {
        failed.push("border");
    }

    ChromeRepair {before, after:snapshot(hwnd), failed}
}

/// Short bounded log entry for real-device fullscreen diagnosis, including
/// native vs DWM bounds (the shadow/compositor may exceed GetWindowRect).
/// This is read-only; no screenshot, Android contents or device serial.
pub fn describe(role: &str, clip_ok: bool, repair: &ChromeRepair) -> String {
    let after = &repair.after;
    format!(
        "[FRAME][DWM] {role}: clip={clip_ok}, corner={:?}->{:?}, border={:?}->{:?}, nc={:?}->{:?}, class_shadow={}, style=0x{:x}, ex=0x{:x}, hwnd_rect={:?}, dwm_frame={:?}, attr_errors={:?}",
        repair.before.corner, after.corner,
        repair.before.border, after.border,
        repair.before.nc_rendering, after.nc_rendering,
        after.class_shadow, after.style,after.ex_style,after.outer,after.dwm_frame,
        repair.failed,
    )
}
