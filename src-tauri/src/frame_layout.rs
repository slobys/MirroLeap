//! Geometry for the click-through device shell. Calculations use native
//! physical screen pixels; the browser skin uses the matching 4/108 vw inset.

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
    pub inset: i32,
}
pub fn expand_screen(screen: ScreenRect) -> Option<FrameRect> {
    if screen.width < 80 || screen.height < 80 || screen.width > 10000 || screen.height > 10000 {
        return None;
    }
    let inset = ((screen.width as f64) * 0.04).round() as i32;
    let width = screen.width.checked_add(inset.checked_mul(2)?)?;
    let height = screen.height.checked_add(inset.checked_mul(2)?)?;
    Some(FrameRect {
        x: screen.x.checked_sub(inset)?,
        y: screen.y.checked_sub(inset)?,
        width,
        height,
        inset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_portrait_phone_in_physical_pixels() {
        let screen = ScreenRect { x: 225, y: 60, width: 400, height: 880 };
        let frame = expand_screen(screen).unwrap();
        assert_eq!(frame, FrameRect { x: 209, y: 44, width: 432, height: 912, inset: 16 });
        assert_eq!(frame.x + frame.inset, screen.x);
        assert_eq!(frame.y + frame.inset, screen.y);
    }

    #[test]
    fn follows_landscape_and_negative_monitor_positions() {
        let screen = ScreenRect { x: -900, y: -250, width: 900, height: 400 };
        let frame = expand_screen(screen).unwrap();
        assert_eq!(frame.x, -936);
        assert_eq!(frame.y, -286);
        assert_eq!(frame.width, 972);
        assert_eq!(frame.height, 472);
    }

    #[test]
    fn avoids_invalid_tiny_or_huge_windows() {
        assert!(expand_screen(ScreenRect { x: 0, y: 0, width: 0, height: 100 }).is_none());
        assert!(expand_screen(ScreenRect { x: 0, y: 0, width: 40, height: 120 }).is_none());
        assert!(expand_screen(ScreenRect { x: 0, y: 0, width: 20000, height: 120 }).is_none());
    }
}
