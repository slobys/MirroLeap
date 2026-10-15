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

#[cfg(test)]
mod tests {
    use super::*;

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
