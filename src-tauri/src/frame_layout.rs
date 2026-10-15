//! Pixel-accurate placement for the phone-frame SVG, measured against the
//! *actual* scrcpy SDL client area in physical Windows desktop pixels.
//!
//! Asset: public/device-frame/iphone-black-portrait.svg
//! viewBox: 1000 x 2150; transparent aperture: x=18 y=18 w=964 h=2114.
//! The insets are independent in X/Y to accommodate different Android screen
//! ratios without leaving a thick black gap or drifting at high Windows DPI.
//! Landscape video is intentionally left untouched in this portrait-first skin.

pub const ARTBOARD_WIDTH: f64 = 1000.0;
pub const ARTBOARD_HEIGHT: f64 = 2150.0;
pub const INNER_X: f64 = 18.0;
pub const INNER_Y: f64 = 18.0;
pub const INNER_WIDTH: f64 = ARTBOARD_WIDTH - 2.0 * INNER_X;
pub const INNER_HEIGHT: f64 = ARTBOARD_HEIGHT - 2.0 * INNER_Y;

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
    // Unsupported landscape / unreasonably small or large Win32 client rects:
    // hide the decoration, but keep scrcpy itself running.
    if screen.width < 80 || screen.height < 80
        || screen.width >= screen.height
        || screen.width > 10000 || screen.height > 10000
    {
        return None;
    }

    // Solve the geometry equation exactly:
    //   inset_x / (video_width + 2*inset_x) = INNER_X / ARTBOARD_WIDTH
    // so the SVG transparent hole lies on the SDL client, even when the
    // Android device differs from the SVG's design aspect ratio.
    let inset_x = ((screen.width as f64) * INNER_X / INNER_WIDTH).round() as i32;
    let inset_y = ((screen.height as f64) * INNER_Y / INNER_HEIGHT).round() as i32;
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


pub const TOOLBAR_WIDTH: i32 = 48;
pub const TOOLBAR_HEIGHT: i32 = 146;
pub const TOOLBAR_GAP: i32 = 8;

/// Test-only geometry reference for the SVG's antialiased inner corner.
/// Native Win32 clipping has been removed because its pixel-staircase edges
/// leaked blue desktop colors through the independently rasterized SVG edge.
#[cfg(test)]
pub fn round_clip_radius(video_width: i32, video_height: i32) -> Option<(i32, i32)> {
    if video_width < 80 || video_height < 80 { return None; }
    let rx = ((video_width as f64) * 57.0 / INNER_WIDTH).round() as i32;
    let ry = ((video_height as f64) * 57.0 / INNER_HEIGHT).round() as i32;
    Some((rx.clamp(4, video_width / 3), ry.clamp(4, video_height / 3)))
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
    fn svg_aperture_corner_reference_matches_physical_pixels() {
        let (rx, ry) = round_clip_radius(540, 1200).unwrap();
        assert!(rx >= 30 && rx <= 35);
        assert!(ry >= 30 && ry <= 35);
        assert!(round_clip_radius(0, 100).is_none());
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
    fn horizontal_video_disables_the_portrait_skin_without_disabling_video() {
        assert!(expand_screen(ScreenRect { x: 0, y: 0, width: 900, height: 400 }).is_none());
        assert!(expand_screen(ScreenRect { x: 0, y: 0, width: 400, height: 400 }).is_none());
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
