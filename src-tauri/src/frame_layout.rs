//! Pixel-accurate placement for the phone-frame SVG, measured against the
//! *actual* scrcpy SDL client area in physical Windows desktop pixels.
//!
//! Asset: public/device-frame/iphone-black-portrait.svg
//! viewBox: 1000 x 2150; transparent aperture: x=18 y=18 w=964 h=2114.
//! The insets are independent in X/Y to accommodate different Android screen
//! ratios without leaving a thick black gap or drifting at high Windows DPI.
//! A rotated SVG skin and swapped X/Y aperture ratios also support landscape video.

pub const ARTBOARD_WIDTH: f64 = 1000.0;
pub const ARTBOARD_HEIGHT: f64 = 2150.0;
pub const INNER_X: f64 = 18.0;
pub const INNER_Y: f64 = 18.0;
pub const INNER_WIDTH: f64 = ARTBOARD_WIDTH - 2.0 * INNER_X;
pub const INNER_HEIGHT: f64 = ARTBOARD_HEIGHT - 2.0 * INNER_Y;
pub const LANDSCAPE_ARTBOARD_WIDTH: f64 = ARTBOARD_HEIGHT;
pub const LANDSCAPE_ARTBOARD_HEIGHT: f64 = ARTBOARD_WIDTH;
pub const LANDSCAPE_INNER_WIDTH: f64 = INNER_HEIGHT;
pub const LANDSCAPE_INNER_HEIGHT: f64 = INNER_WIDTH;

/// Video rotation follows the actual SDL client dimensions; no guess from the
/// phone model and no forced scrcpy video restart.
pub fn is_landscape(video_width: i32, video_height: i32) -> bool {
    video_width > video_height
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub inset_x: i32,
    pub inset_y: i32,
}

pub fn expand_screen(screen: ScreenRect) -> Option<FrameRect> {
    // Reject only invalid geometry. A sideways Android video uses the
    // landscape.svg artboard and keeps the same native SDL stream.
    if screen.width < 80 || screen.height < 80
        || screen.width > 10000 || screen.height > 10000
    {
        return None;
    }

    // Solve the geometry equation exactly:
    //   inset_x / (video_width + 2*inset_x) = INNER_X / ARTBOARD_WIDTH
    // so the SVG transparent hole lies on the SDL client, even when the
    // Android device differs from the SVG's design aspect ratio.
    let (inner_width, inner_height) = if is_landscape(screen.width, screen.height) {
        (LANDSCAPE_INNER_WIDTH, LANDSCAPE_INNER_HEIGHT)
    } else {
        (INNER_WIDTH, INNER_HEIGHT)
    };
    let inset_x = ((screen.width as f64) * INNER_X / inner_width).round() as i32;
    let inset_y = ((screen.height as f64) * INNER_Y / inner_height).round() as i32;
    let width = screen.width.checked_add(inset_x.checked_mul(2)?)?;
    let height = screen.height.checked_add(inset_y.checked_mul(2)?)?;

    Some(FrameRect {
        x: screen.x.checked_sub(inset_x)?,
        y: screen.y.checked_sub(inset_y)?,
        width,
        height,
        inset_x,
        inset_y,
    })
}


pub const TOOLBAR_WIDTH: i32 = 62;
pub const TOOLBAR_HEIGHT: i32 = 318;
/// Breathing room both below the display's top edge and above the taskbar,
/// AND inside the visible top/bottom edge of the mirrored phone.
pub const TOOLBAR_SAFE_MARGIN: i32 = 12;
/// In landscape/portrait alike the session controls keep their own 62px rail.
/// The small palette opens to the side when there is room; on narrow desktop
/// margins it docks BELOW the rail rather than obscuring the Android screen.
pub const ANNOTATION_TOOLBAR_WIDTH: i32 = 268;
pub const ANNOTATION_STACK_WIDTH: i32 = 168;
pub const ANNOTATION_TOOLBAR_HEIGHT: i32 = 534;
/// A short (or partly off-screen) landscape video does not have enough
/// vertical space for the usual 270px rail. Keep the five actions in two ROWS
/// rather than cropping the last buttons or overlapping the Windows taskbar.
pub const SHORT_TOOLBAR_WIDTH: i32 = 168;
pub const SHORT_TOOLBAR_HEIGHT: i32 = 118;
pub const SHORT_INK_TOOLBAR_WIDTH: i32 = 374;
pub const SHORT_INK_TOOLBAR_HEIGHT: i32 = 212;
pub const TIGHT_INK_TOOLBAR_HEIGHT: i32 = 136;
pub const TOOLBAR_GAP: i32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarLayout {
    Compact,
    SideRight,
    SideLeft,
    StackRight,
    StackLeft,
    ShortRight,
    ShortLeft,
    ShortInkRight,
    ShortInkLeft,
    TightInkRight,
    TightInkLeft,
}

impl ToolbarLayout {
    pub fn css_name(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::SideRight => "side-right",
            Self::SideLeft => "side-left",
            Self::StackRight => "stack-right",
            Self::StackLeft => "stack-left",
            Self::ShortRight => "short-right",
            Self::ShortLeft => "short-left",
            Self::ShortInkRight => "short-ink-right",
            Self::ShortInkLeft => "short-ink-left",
            Self::TightInkRight => "tight-ink-right",
            Self::TightInkLeft => "tight-ink-left",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarPlacement {
    pub rect: ScreenRect,
    pub layout: ToolbarLayout,
    /// CSS zoom is adjusted with the native HWND rectangle when even the
    /// horizontal rail cannot fit the visible part of the phone. Never just
    /// shrink the HWND and leave WebView buttons clipped underneath.
    pub zoom_percent: u32,
}

/// Native HWND companions do not need a SetWindowPos call every 50 ms.
/// Only actual motion, visibility restoration, unexpected z-order or a
/// low-frequency health check require the compositor to be repositioned.
pub fn companion_needs_reposition(
    previous: Option<ScreenRect>,
    desired: ScreenRect,
    visible: bool,
    correctly_stacked: bool,
    health_due: bool,
) -> bool {
    !visible || previous != Some(desired) || !correctly_stacked || health_due
}

/// All desktop/frame coordinates are Win32 PHYSICAL pixels, but WebView CSS
/// sizes are DIPs. Multiply the control geometry by the video window DPI so
/// that e.g. a 62 CSS-pixel rail is not clipped to 41 CSS pixels at 150%.
fn dip(dip: i32, dpi: u32) -> i32 {
    ((dip as i64 * (dpi.clamp(72, 480)) as i64 + 48) / 96) as i32
}

/// `top` is the saved top edge of the toolbar (not the mouse position).
///
/// `desktop` MUST be the current monitor's WORK AREA (rcWork), rather than
/// SM_*VIRTUALSCREEN or rcMonitor: the latter includes the Windows taskbar.
/// A toolbar is shown ONLY in the intersection with the visible phone frame,
/// with 12 DIP padding on both ends. The short landscape rail preserves
/// accessible 44 DIP targets. When even that can't fit, scale the entire
/// WebView + native HWND together instead of silently clipping a button.
pub fn toolbar_placement(
    frame: FrameRect, top: i32, desktop: ScreenRect, inking: bool, dpi: u32,
) -> Option<ToolbarPlacement> {
    if desktop.width <= 0 || desktop.height <= 0 || frame.width <= 0 || frame.height <= 0 {
        return None;
    }
    let right = frame.x.saturating_add(frame.width).saturating_add(dip(TOOLBAR_GAP, dpi));
    let left = frame.x.saturating_sub(dip(TOOLBAR_GAP, dpi));
    let desktop_right = desktop.x.saturating_add(desktop.width);
    let desktop_bottom = desktop.y.saturating_add(desktop.height);
    let visible_top = frame.y.max(desktop.y);
    let visible_bottom = frame.y.saturating_add(frame.height).min(desktop_bottom);
    if visible_bottom <= visible_top {
        return None;
    }
    let visible_height = visible_bottom - visible_top;
    let margin = dip(TOOLBAR_SAFE_MARGIN, dpi).min((visible_height - 1) / 4);
    let safe_top = visible_top.saturating_add(margin);
    let safe_bottom = visible_bottom.saturating_sub(margin);
    let usable_height = safe_bottom.saturating_sub(safe_top).max(1);
    let right_room = desktop_right.saturating_sub(right);
    let left_room = left.saturating_sub(desktop.x);
    let narrow = dip(TOOLBAR_WIDTH, dpi);
    let wide = dip(ANNOTATION_TOOLBAR_WIDTH, dpi);
    let stacked = dip(ANNOTATION_STACK_WIDTH, dpi);
    let stacked_height = dip(ANNOTATION_TOOLBAR_HEIGHT, dpi);
    let compact_height = dip(TOOLBAR_HEIGHT, dpi);
    let can_stack = usable_height >= stacked_height;
    let enough_for_rail = usable_height >= compact_height;
    let enough_for_short_ink = usable_height >= dip(SHORT_INK_TOOLBAR_HEIGHT, dpi);
    let short_width = dip(SHORT_TOOLBAR_WIDTH, dpi);
    let short_ink_width = dip(SHORT_INK_TOOLBAR_WIDTH, dpi);
    let (layout, _requested_x, base_width, base_height) = if !enough_for_rail {
        // Compact *horizontal* rail is much easier to click than five tiny
        // vertical targets. It also makes minimized/partially off-screen
        // windows recover without controls protruding from the desktop.
        if !inking {
            if right_room >= short_width || right_room >= left_room {
                (ToolbarLayout::ShortRight, right, SHORT_TOOLBAR_WIDTH, SHORT_TOOLBAR_HEIGHT)
            } else {
                (ToolbarLayout::ShortLeft, left - short_width, SHORT_TOOLBAR_WIDTH, SHORT_TOOLBAR_HEIGHT)
            }
        } else {
            let right_side = right_room >= short_ink_width || right_room >= left_room;
            let height = if enough_for_short_ink {
                SHORT_INK_TOOLBAR_HEIGHT
            } else {
                TIGHT_INK_TOOLBAR_HEIGHT
            };
            let layout = match (right_side, enough_for_short_ink) {
                (true, true) => ToolbarLayout::ShortInkRight,
                (false, true) => ToolbarLayout::ShortInkLeft,
                (true, false) => ToolbarLayout::TightInkRight,
                (false, false) => ToolbarLayout::TightInkLeft,
            };
            (layout, if right_side { right } else { left - short_ink_width },
                SHORT_INK_TOOLBAR_WIDTH, height)
        }
    } else if !inking {
        if right_room >= narrow {
            (ToolbarLayout::Compact, right, TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
        } else {
            (ToolbarLayout::Compact, left - narrow, TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
        }
    } else if right_room >= wide {
        (ToolbarLayout::SideRight, right, ANNOTATION_TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
    } else if left_room >= wide {
        (ToolbarLayout::SideLeft, left - wide, ANNOTATION_TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
    } else if can_stack && right_room >= stacked {
        (ToolbarLayout::StackRight, right, ANNOTATION_STACK_WIDTH, ANNOTATION_TOOLBAR_HEIGHT)
    } else if can_stack && left_room >= stacked {
        (ToolbarLayout::StackLeft, left - stacked, ANNOTATION_STACK_WIDTH, ANNOTATION_TOOLBAR_HEIGHT)
    } else if right_room >= left_room {
        (ToolbarLayout::SideRight, right, ANNOTATION_TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
    } else {
        (ToolbarLayout::SideLeft, left - wide, ANNOTATION_TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
    };
    let requested_height = dip(base_height, dpi);
    let requested_width = dip(base_width, dpi);
    let height_scale = usable_height as i64 * 100 / requested_height.max(1) as i64;
    let width_scale = desktop.width as i64 * 100 / requested_width.max(1) as i64;
    let zoom_percent = height_scale.min(width_scale).clamp(1, 100) as u32;
    let width = ((requested_width as i64 * zoom_percent as i64) / 100)
        .max(1).min(desktop.width as i64) as i32;
    let height = ((requested_height as i64 * zoom_percent as i64) / 100)
        .max(1).min(usable_height as i64) as i32;
    // The CSS zoom shrinks the toolbar width too. Re-evaluate which side fits
    // after scaling before allowing a rail to overlap the mirrored screen.
    let is_left = (layout == ToolbarLayout::Compact && right_room < narrow)
        || matches!(layout, ToolbarLayout::SideLeft | ToolbarLayout::StackLeft |
            ToolbarLayout::ShortLeft | ToolbarLayout::ShortInkLeft | ToolbarLayout::TightInkLeft);
    let x = if is_left { left.saturating_sub(width) } else { right };
    // Tiny viewports must not cause a panic or a coordinate beyond rcWork.
    Some(ToolbarPlacement {
        rect: ScreenRect {
            x: x.clamp(desktop.x, (desktop_right - width).max(desktop.x)),
            y: top.clamp(safe_top, (safe_bottom - height).max(safe_top)),
            width,
            height,
        },
        layout,
        zoom_percent,
    })
}

/// Ellipse radii for the REAL SDL client region, in physical screen pixels.
///
/// The antialiased SVG aperture uses a 57-unit rounded corner. The GDI region
/// MUST be deliberately **less rounded** by 3 physical pixels in both axes:
/// this makes every SVG-transparent screen pixel backed by live video, while
/// the dark bezel/gasket hides the invisible hard GDI boundary underneath.
///
/// v4.3.2 matched the radii exactly and exposed cyan wallpaper-colored seams.
/// v4.3.3 removed the native region and introduced square black corners.
/// Never match radii exactly or use a square overpainting rectangle again.
pub const NATIVE_CORNER_OVERSCAN_PX: i32 = 3;
pub fn native_clip_radius(video_width: i32, video_height: i32) -> Option<(i32, i32)> {
    if video_width < 80 || video_height < 80 { return None; }
    let (inner_width, inner_height) = if is_landscape(video_width, video_height) {
        (LANDSCAPE_INNER_WIDTH, LANDSCAPE_INNER_HEIGHT)
    } else {
        (INNER_WIDTH, INNER_HEIGHT)
    };
    let rx = ((video_width as f64) * 57.0 / inner_width).round() as i32;
    let ry = ((video_height as f64) * 57.0 / inner_height).round() as i32;
    Some(((rx - NATIVE_CORNER_OVERSCAN_PX).clamp(2, video_width / 3),
          (ry - NATIVE_CORNER_OVERSCAN_PX).clamp(2, video_height / 3)))
}

#[cfg(test)]
fn svg_aperture_radius(video_width: i32, video_height: i32) -> Option<(f64, f64)> {
    if video_width < 80 || video_height < 80 { return None; }
    let (inner_width, inner_height) = if is_landscape(video_width, video_height) {
        (LANDSCAPE_INNER_WIDTH, LANDSCAPE_INNER_HEIGHT)
    } else {
        (INNER_WIDTH, INNER_HEIGHT)
    };
    Some((video_width as f64 * 57.0 / inner_width,
          video_height as f64 * 57.0 / inner_height))
}

/// Hover near the right/left bezel, or the top edge; the controller never
/// covers the live video when not requested.
pub fn is_frame_edge_hover(x: i32, y: i32, frame: FrameRect) -> bool {
    let within_y = y >= frame.y && y <= frame.y + frame.height;
    let within_x = x >= frame.x && x <= frame.x + frame.width;
    let on_side = within_y && (
        (x >= frame.x - 10 && x <= frame.x + 38)
            || (x >= frame.x + frame.width - 38 && x <= frame.x + frame.width + 10)
    );
    let on_top = within_x && y >= frame.y - 10 && y <= frame.y + 32;
    on_side || on_top
}

/// Prefer a non-overlapping right-side popover, fall back to the left if the
/// device is too close to the desktop edge. Clamp on multi-monitor desktops.
pub fn toolbar_position(frame: FrameRect, cursor_y: i32, desktop: ScreenRect) -> ScreenRect {
    let bounds_right = desktop.x + desktop.width;
    let right = frame.x + frame.width + TOOLBAR_GAP;
    let left = frame.x - TOOLBAR_GAP - TOOLBAR_WIDTH;
    let fits_right = right + TOOLBAR_WIDTH <= bounds_right;
    let requested_x = if fits_right { right } else { left };
    let max_x = (bounds_right - TOOLBAR_WIDTH).max(desktop.x);
    let max_y = (desktop.y + desktop.height - TOOLBAR_HEIGHT).max(desktop.y);
    ScreenRect {
        x: requested_x.clamp(desktop.x, max_x),
        y: (cursor_y - TOOLBAR_HEIGHT / 2).clamp(desktop.y, max_y),
        width: TOOLBAR_WIDTH,
        height: TOOLBAR_HEIGHT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipped_video_covers_the_transparent_svg_aperture() {
        let samples = [
            (400, 880), (540, 1280), (720, 1600), (500, 1050),
            (800, 1760), (390, 844), (130, 260)
        ];
        for (width, height) in samples {
            let (native_x, native_y) = native_clip_radius(width, height).unwrap();
            let (svg_x, svg_y) = svg_aperture_radius(width, height).unwrap();
            assert!((native_x as f64) < svg_x, "GDI must overlap into masked bezel");
            assert!((native_y as f64) < svg_y, "GDI must overlap into masked bezel");
            // At every Y through the corner, GDI cannot clip the exposed
            // aperture: the native rounded shape includes the SVG hole.
            for pixel_y in 0..height {
                let arc_x = |rx: f64, ry: f64| {
                    if pixel_y as f64 >= ry { 0.0 } else {
                        let normalized = (pixel_y as f64 - ry) / ry;
                        rx * (1.0 - (1.0 - normalized.powi(2)).max(0.0).sqrt())
                    }
                };
                let native_edge = arc_x(native_x as f64, native_y as f64);
                let svg_edge = arc_x(svg_x, svg_y);
                assert!(native_edge <= svg_edge + 0.01,
                    "GDI edge intrudes into visible Android pixels at Y={pixel_y} for {width}x{height}");
            }
        }
        assert!(native_clip_radius(0, 100).is_none());
    }

    #[test]
    fn native_corner_stays_within_outer_iphone_silhouette() {
        // At the top-left client pixel the phone's outer metallic contour is
        // several physical pixels further left than the underscanned region,
        // so scrcpy does not protrude outside the rounded handset.
        for (width, height) in [(400,880), (540,1280), (720,1600)] {
            let f = expand_screen(ScreenRect { x: 0,y: 0,width,height }).unwrap();
            let (clip_x, _) = native_clip_radius(width,height).unwrap();
            let sx = f.width as f64 / ARTBOARD_WIDTH;
            let sy = f.height as f64 / ARTBOARD_HEIGHT;
            let outer_rx = 76.0 * sx;
            let outer_ry = 76.0 * sy;
            let y = f.inset_y as f64 - sy;
            let outer_x = sx - f.inset_x as f64
              + outer_rx * (1.0 - (1.0-((y-outer_ry)/outer_ry).powi(2)).sqrt());
            assert!(clip_x as f64 > outer_x, "SDL video may protrude outside metal casing");
        }
    }

    #[test]
    fn annotation_palette_fits_on_a_typical_monitor() {
        let frame = expand_screen(ScreenRect { x: 240, y: 10, width: 410, height: 850 }).unwrap();
        let desktop = ScreenRect { x: 0, y: 0, width: 1920, height: 1080 };
        let placement = toolbar_placement(frame, frame.y, desktop, true, 96).unwrap();
        assert_eq!(placement.layout, ToolbarLayout::SideRight);
        assert_eq!(placement.rect.width, ANNOTATION_TOOLBAR_WIDTH);
        assert_eq!(placement.rect.height, TOOLBAR_HEIGHT);
        assert!(placement.rect.y >= desktop.y);
        assert!(placement.rect.y + placement.rect.height <= 1080);
        assert!(placement.rect.x > frame.x + frame.width);
    }

    #[test]
    fn stable_companion_does_not_reorder_windows_on_every_follow_tick() {
        let current = ScreenRect { x: 300, y: 40, width: 500, height: 700 };
        assert!(!companion_needs_reposition(Some(current), current, true, true, false));
        assert!(companion_needs_reposition(None, current, true, true, false));
        assert!(companion_needs_reposition(Some(current), current, false, true, false));
        assert!(companion_needs_reposition(Some(current), current, true, false, false));
        assert!(companion_needs_reposition(Some(current), current, true, true, true));
        let moved = ScreenRect { x: 305, ..current };
        assert!(companion_needs_reposition(Some(current), moved, true, true, false));
    }

    #[test]
    fn flyout_flips_left_instead_of_running_off_right_desktop_edge() {
        let desktop = ScreenRect { x: 0, y: 0, width: 1920, height: 1080 };
        let frame = FrameRect { x: 1170, y: 150, width: 680, height: 800, inset_x: 7, inset_y: 7 };
        let placement = toolbar_placement(frame, 200, desktop, true, 96).unwrap();
        assert_eq!(placement.layout, ToolbarLayout::SideLeft);
        assert!(placement.rect.x + placement.rect.width < frame.x);
        assert!(placement.rect.x >= desktop.x);
    }

    #[test]
    fn narrow_outer_margin_stacks_palette_below_rail_without_covering_phone() {
        let desktop = ScreenRect { x: 0, y: 0, width: 810, height: 1440 };
        let right_frame = FrameRect { x: 40, y: 45, width: 590, height: 1250, inset_x: 7, inset_y: 7 };
        let right = toolbar_placement(right_frame, 620, desktop, true, 96).unwrap();
        assert_eq!(right.layout, ToolbarLayout::StackRight);
        assert_eq!(right.rect.width, ANNOTATION_STACK_WIDTH);
        assert_eq!(right.rect.height, ANNOTATION_TOOLBAR_HEIGHT);
        assert!(right.rect.x >= right_frame.x + right_frame.width);
        assert!(right.rect.y + right.rect.height <= desktop.height);

        let left_frame = FrameRect { x: 185, y: 50, width: 600, height: 1200, inset_x: 7, inset_y: 7 };
        let left = toolbar_placement(left_frame, 680, desktop, true, 96).unwrap();
        assert_eq!(left.layout, ToolbarLayout::StackLeft);
        assert!(left.rect.x + left.rect.width < left_frame.x);
    }

    #[test]
    fn placement_uses_physical_pixels_on_scaled_and_negative_origin_displays() {
        let desktop = ScreenRect { x: -2560, y: -150, width: 5120, height: 1800 };
        let frame = FrameRect { x: -1200, y: -100, width: 690, height: 1100, inset_x: 12, inset_y: 12 };
        let placement = toolbar_placement(frame, -200, desktop, true, 144).unwrap();
        assert_eq!(placement.layout, ToolbarLayout::SideRight);
        assert_eq!(placement.rect.width, ANNOTATION_TOOLBAR_WIDTH * 3 / 2);
        assert_eq!(placement.rect.height, TOOLBAR_HEIGHT * 3 / 2);
        assert!(placement.rect.x >= desktop.x);
        assert!(placement.rect.y >= desktop.y);
        assert!(placement.rect.x + placement.rect.width <= desktop.x + desktop.width);
        assert!(placement.rect.y + placement.rect.height <= desktop.y + desktop.height);
    }

    #[test]
    fn stacked_layout_never_clips_below_bottom_of_the_desktop() {
        let desktop = ScreenRect { x: 0, y: 0, width: 810, height: 720 };
        let frame = FrameRect { x: 40, y: 20, width: 590, height: 680, inset_x: 7, inset_y: 7 };
        let placement = toolbar_placement(frame, 690, desktop, true, 96).unwrap();
        assert_eq!(placement.layout, ToolbarLayout::StackRight);
        assert!(placement.rect.y + placement.rect.height <= 720);
    }

    /// Regression for user's screenshot: phone is taller than the monitor,
    /// desktop has a bottom taskbar, and the cursor starts near the taskbar.
    #[test]
    fn oversize_portrait_rail_never_goes_below_taskbar_or_above_phone() {
        let work = ScreenRect { x: 0, y: 0, width: 1920, height: 1030 };
        let phone = FrameRect { x: 45, y: 25, width: 700, height: 1560, inset_x: 7, inset_y: 7 };
        for initial_top in [-1000, 0, 180, 975, 1600, 10000] {
            for ink in [false, true] {
                let p = toolbar_placement(phone, initial_top, work, ink, 96).unwrap();
                assert_eq!(p.zoom_percent, 100);
                assert!(p.rect.y >= phone.y + TOOLBAR_SAFE_MARGIN);
                assert!(p.rect.y >= work.y + TOOLBAR_SAFE_MARGIN);
                assert!(p.rect.y + p.rect.height <= work.y + work.height - TOOLBAR_SAFE_MARGIN);
                assert!(p.rect.y + p.rect.height <= phone.y + phone.height - TOOLBAR_SAFE_MARGIN);
            }
        }
    }

    /// User's browser screenshot: phone begins below the top of a maximized
    /// browser, but the toolbar formerly drifted to Y=0 above the phone.
    #[test]
    fn rail_does_not_cross_the_top_of_the_phone() {
        let work = ScreenRect { x: 0, y: 0, width: 1920, height: 1040 };
        let phone = FrameRect { x: 38, y: 65, width: 880, height: 1100, inset_x: 8, inset_y: 8 };
        let p = toolbar_placement(phone, -1200, work, false, 96).unwrap();
        assert_eq!(p.rect.y, 65 + TOOLBAR_SAFE_MARGIN);
        assert_eq!(p.rect.height, TOOLBAR_HEIGHT);
    }

    #[test]
    fn short_landscape_uses_a_wide_two_row_rail_instead_of_clipped_buttons() {
        let work = ScreenRect { x: 0, y: 0, width: 1920, height: 1020 };
        let phone = FrameRect { x: 200, y: 370, width: 1180, height: 250, inset_x: 7, inset_y: 7 };
        let p = toolbar_placement(phone, 615, work, false, 96).unwrap();
        assert_eq!(p.layout, ToolbarLayout::ShortRight);
        assert_eq!(p.rect.width, SHORT_TOOLBAR_WIDTH);
        assert_eq!(p.rect.height, SHORT_TOOLBAR_HEIGHT);
        assert!(p.rect.y >= phone.y);
        assert!(p.rect.y + p.rect.height <= phone.y + phone.height);

        let ink = toolbar_placement(phone, 615, work, true, 96).unwrap();
        assert_eq!(ink.layout, ToolbarLayout::ShortInkRight);
        assert_eq!(ink.rect.height, SHORT_INK_TOOLBAR_HEIGHT);
        assert!(ink.rect.y + ink.rect.height <= phone.y + phone.height);
    }

    #[test]
    fn small_landscape_palette_keeps_all_tools_with_condensed_layout() {
        let work = ScreenRect { x: 0, y: 0, width: 1600, height: 900 };
        let phone = FrameRect { x: 120, y: 250, width: 1190, height: 180, inset_x: 6, inset_y: 6 };
        let p = toolbar_placement(phone, 0, work, true, 96).unwrap();
        assert_eq!(p.layout, ToolbarLayout::TightInkRight);
        assert_eq!(p.rect.height, TIGHT_INK_TOOLBAR_HEIGHT);
        assert_eq!(p.zoom_percent, 100);
        assert!(p.rect.y >= 262);
        assert!(p.rect.y + p.rect.height <= 418);
    }

    #[test]
    fn partly_offscreen_video_scales_the_entire_webview_and_stays_visible() {
        let work = ScreenRect { x: 0, y: 0, width: 1920, height: 1010 };
        let phone = FrameRect { x: 100, y: 950, width: 900, height: 500, inset_x: 8, inset_y: 8 };
        for inking in [false, true] {
            let p = toolbar_placement(phone, 5000, work, inking, 96).unwrap();
            assert!(p.zoom_percent < 100);
            assert!(p.zoom_percent >= 1);
            assert!(p.rect.y >= 950);
            assert!(p.rect.y + p.rect.height <= 1010);
            assert!(p.rect.width > 0 && p.rect.height > 0);
            assert!(p.rect.x >= 0 && p.rect.x + p.rect.width <= 1920);
        }
    }

    #[test]
    fn secondary_monitor_and_high_dpi_have_safe_margins() {
        let work = ScreenRect { x: -2400, y: -120, width: 2400, height: 1310 };
        let phone = FrameRect { x: -510, y: -70, width: 490, height: 1470, inset_x: 9, inset_y: 9 };
        let p = toolbar_placement(phone, -9999, work, true, 144).unwrap();
        assert_eq!(p.layout, ToolbarLayout::SideLeft);
        assert_eq!(p.rect.height, TOOLBAR_HEIGHT * 3 / 2);
        assert_eq!(p.rect.y, -70 + 18);
        assert!(p.rect.x >= work.x && p.rect.x + p.rect.width <= 0);
        assert!(p.rect.y + p.rect.height <= work.y + work.height - 18);
    }

    #[test]
    fn fully_invisible_phone_or_invalid_work_area_never_spawns_ghost_toolbar() {
        let work = ScreenRect { x: 0, y: 0, width: 1920, height: 1040 };
        let phone = FrameRect { x: 100, y: 1400, width: 700, height: 700, inset_x: 8, inset_y: 8 };
        assert!(toolbar_placement(phone, 1400, work, false, 96).is_none());
        let invalid = ScreenRect { x: 0, y: 0, width: 1920, height: 0 };
        assert!(toolbar_placement(phone, 1400, invalid, true, 96).is_none());
    }

    #[test]
    fn toolbar_appears_at_edge_and_stays_outside_video() {
        let phone = expand_screen(ScreenRect { x: 200, y: 30, width: 450, height: 940 }).unwrap();
        let desktop = ScreenRect { x: 0, y: 0, width: 1920, height: 1080 };
        assert!(is_frame_edge_hover(phone.x + phone.width - 2, 450, phone));
        assert!(is_frame_edge_hover(phone.x + 120, phone.y + 4, phone));
        assert!(!is_frame_edge_hover(phone.x + 225, 450, phone));
        let ctrl = toolbar_position(phone, 450, desktop);
        assert!(ctrl.x >= phone.x + phone.width);
        assert!(ctrl.y >= 0 && ctrl.y + ctrl.height <= 1080);
    }

    #[test]
    fn toolbar_falls_back_to_left_on_rightmost_monitor() {
        let phone = expand_screen(ScreenRect { x: 1450, y: 100, width: 440, height: 900 }).unwrap();
        let desktop = ScreenRect { x: 0, y: 0, width: 1920, height: 1080 };
        let ctrl = toolbar_position(phone, 220, desktop);
        assert!(ctrl.x + ctrl.width <= phone.x);
    }

    #[test]
    fn toolbar_clamps_to_negative_origin_virtual_desktop() {
        let phone = expand_screen(ScreenRect { x: -650, y: -80, width: 450, height: 900 }).unwrap();
        let desktop = ScreenRect { x: -1920, y: -200, width: 3840, height: 1280 };
        let ctrl = toolbar_position(phone, -195, desktop);
        assert!(ctrl.x >= -1920 && ctrl.y >= -200);
    }

    #[test]
    fn narrow_bezel_aligns_sdl_client_for_portrait() {
        let screen = ScreenRect { x: 225, y: 60, width: 400, height: 880 };
        let frame = expand_screen(screen).unwrap();
        assert_eq!(frame, FrameRect {
            x: 218, y: 53, width: 414, height: 894, inset_x: 7, inset_y: 7
        });
        assert_eq!(frame.x + frame.inset_x, screen.x);
        assert_eq!(frame.y + frame.inset_y, screen.y);
        assert_eq!(frame.width - 2 * frame.inset_x, screen.width);
        assert_eq!(frame.height - 2 * frame.inset_y, screen.height);
    }

    #[test]
    fn no_thick_bezel_on_tall_android_screen() {
        let screen = ScreenRect { x: -940, y: -120, width: 540, height: 1280 };
        let frame = expand_screen(screen).unwrap();
        assert!(frame.inset_x <= 11);
        assert!(frame.inset_y <= 11);
        assert_eq!(frame.x + frame.inset_x, screen.x);
        assert_eq!(frame.y + frame.inset_y, screen.y);
    }

    #[test]
    fn landscape_video_uses_rotated_svg_aperture_without_restarting_video() {
        let video = ScreenRect { x: -700, y: 150, width: 1280, height: 720 };
        assert!(is_landscape(video.width, video.height));
        let frame = expand_screen(video).unwrap();
        assert_eq!(frame.x + frame.inset_x, video.x);
        assert_eq!(frame.y + frame.inset_y, video.y);
        assert_eq!(frame.width - frame.inset_x * 2, video.width);
        assert_eq!(frame.height - frame.inset_y * 2, video.height);
        assert!(frame.inset_x <= 12);
        assert!(frame.inset_y <= 15);
        let (clip_x, clip_y) = native_clip_radius(video.width, video.height).unwrap();
        let (svg_x, svg_y) = svg_aperture_radius(video.width, video.height).unwrap();
        assert!((clip_x as f64) < svg_x && (clip_y as f64) < svg_y);
        let mapped_left = frame.width as f64 * INNER_X / LANDSCAPE_ARTBOARD_WIDTH;
        let mapped_top = frame.height as f64 * INNER_Y / LANDSCAPE_ARTBOARD_HEIGHT;
        assert!((mapped_left - frame.inset_x as f64).abs() < 0.55);
        assert!((mapped_top - frame.inset_y as f64).abs() < 0.55);
    }

    #[test]
    fn restoring_portrait_after_landscape_keeps_screen_client_aligned() {
        for (w,h) in [(1280,720), (720,1280), (1920,1080), (450,960)] {
            let client=ScreenRect{x:33,y:-17,width:w,height:h};
            let frame=expand_screen(client).unwrap();
            assert_eq!(frame.x+frame.inset_x,client.x);
            assert_eq!(frame.y+frame.inset_y,client.y);
        }
    }

    #[test]
    fn rejects_invalid_dimensions() {
        assert!(expand_screen(ScreenRect { x: 0, y: 0, width: 0, height: 100 }).is_none());
        assert!(expand_screen(ScreenRect { x: 0, y: 0, width: 40, height: 120 }).is_none());
        assert!(expand_screen(ScreenRect { x: 0, y: 0, width: 20000, height: 21000 }).is_none());
    }

    #[test]
    fn aperture_matches_svg_to_within_half_a_pixel_before_rounding() {
        let screen = ScreenRect { x: -800, y: -20, width: 583, height: 1260 };
        let frame = expand_screen(screen).unwrap();
        let mapped_left = frame.width as f64 * INNER_X / ARTBOARD_WIDTH;
        let mapped_top = frame.height as f64 * INNER_Y / ARTBOARD_HEIGHT;
        assert!((mapped_left - frame.inset_x as f64).abs() < 0.5);
        assert!((mapped_top - frame.inset_y as f64).abs() < 0.5);
    }
}
