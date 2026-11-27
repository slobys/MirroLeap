//! State and report encoding for a confirmed iOS device's Bluetooth keyboard and mouse.

use serde::Deserialize;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

pub(crate) const AXIS_MAX: u16 = 10_000;
pub(crate) const HID_AXIS_MAX: u32 = 32_767;
pub(crate) const BATCH_LIMIT: usize = 16;
const MAX_SWIPE_STEPS: i8 = 8;
const CLICK_TO_KEY_DELAY: Duration = Duration::from_millis(80);
const KEY_REPORT_INTERVAL: Duration = Duration::from_millis(8);

pub(crate) type Position = (u16, u16);

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Event {
    Pointer {
        x: u16,
        y: u16,
        buttons: u8,
        wheel: i8,
    },
    Scroll {
        x: u16,
        y: u16,
        wheel: i8,
        pan: i8,
        start: bool,
    },
    Key {
        usage: u8,
        down: bool,
    },
    Click {
        x: u16,
        y: u16,
        button: u8,
    },
    Release,
}

impl Event {
    pub(crate) fn valid(&self) -> bool {
        match *self {
            Self::Pointer {
                x,
                y,
                buttons,
                wheel,
            } => x <= AXIS_MAX && y <= AXIS_MAX && buttons <= 7 && wheel != i8::MIN,
            Self::Scroll {
                x, y, wheel, pan, ..
            } => {
                x <= AXIS_MAX
                    && y <= AXIS_MAX
                    && (-MAX_SWIPE_STEPS..=MAX_SWIPE_STEPS).contains(&wheel)
                    && (-MAX_SWIPE_STEPS..=MAX_SWIPE_STEPS).contains(&pan)
                    && ((wheel != 0) ^ (pan != 0))
            }
            Self::Key { usage, .. } => matches!(usage, 4..=0x65 | 0xe0..=0xe7),
            Self::Click { x, y, button } => {
                x <= AXIS_MAX && y <= AXIS_MAX && matches!(button, 1 | 2 | 4)
            }
            Self::Release => true,
        }
    }
}

#[derive(Default)]
pub(crate) struct State {
    modifiers: u8,
    keys: BTreeSet<u8>,
    keyboard_ready_at: Option<Instant>,
    keyboard_cleanup_needed: bool,
    pub(crate) pointer: Option<Position>,
    buttons: u8,
}

impl State {
    pub(crate) fn keyboard(&mut self, usage: u8, down: bool) -> Option<[u8; 8]> {
        if usage >= 0xe0 {
            let mask = 1 << (usage - 0xe0);
            let previous = self.modifiers;
            self.modifiers = (previous & !mask) | if down { mask } else { 0 };
            if self.modifiers == previous {
                return None;
            }
        } else if !(if down {
            self.keys.insert(usage)
        } else {
            self.keys.remove(&usage)
        }) {
            return None;
        }
        let mut report = [0; 8];
        report[0] = self.modifiers;
        if self.keys.len() > 6 {
            report[2..].fill(1); // HID ErrorRollOver
        } else {
            for (slot, usage) in report[2..].iter_mut().zip(&self.keys) {
                *slot = *usage;
            }
        }
        Some(report)
    }

    pub(crate) fn buttons(&self) -> u8 {
        self.buttons
    }

    pub(crate) fn record_buttons(&mut self, buttons: u8, delivered: bool) {
        self.buttons = if delivered {
            buttons
        } else {
            buttons | self.buttons
        };
    }

    pub(crate) fn release_keys(&mut self) -> bool {
        let pressed = self.modifiers != 0 || !self.keys.is_empty() || self.keyboard_cleanup_needed;
        self.modifiers = 0;
        self.keys.clear();
        pressed
    }

    pub(crate) fn note_primary_click(&mut self, now: Instant) {
        // The iOS app gains text focus after the mouse-up reaches it.
        self.keyboard_ready_at = Some(now + CLICK_TO_KEY_DELAY);
    }

    pub(crate) fn keyboard_ready_at(&self) -> Option<Instant> {
        self.keyboard_ready_at
    }

    pub(crate) fn record_keyboard_delivery(&mut self, delivered: bool, now: Instant) {
        self.keyboard_cleanup_needed = !delivered;
        self.keyboard_ready_at = self.keyboard_ready_at.max(Some(now + KEY_REPORT_INTERVAL));
    }
}

pub(crate) fn absolute_report((x, y): Position) -> [u8; 5] {
    let to_hid = |value| {
        ((u32::from(value) * HID_AXIS_MAX + u32::from(AXIS_MAX / 2)) / u32::from(AXIS_MAX)) as u16
    };
    let [xl, xh] = to_hid(x).to_le_bytes();
    let [yl, yh] = to_hid(y).to_le_bytes();
    [0, xl, xh, yl, yh]
}

pub(crate) fn relative_report(buttons: u8, x: i8, y: i8, (wheel, pan): (i8, i8)) -> [u8; 5] {
    [buttons, x as u8, y as u8, wheel as u8, pan as u8]
}

pub(crate) fn swipe_steps((wheel, pan): (i8, i8)) -> impl ExactSizeIterator<Item = (i8, i8)> {
    let steps = usize::from(wheel.unsigned_abs().max(pan.unsigned_abs()));
    std::iter::repeat_n((wheel.signum(), pan.signum()), steps)
}

pub(crate) fn drag_steps(
    from: Position,
    to: Position,
    (width, height): Position,
) -> impl Iterator<Item = (i8, i8)> {
    let pixels = |value, size| {
        (i32::from(value) * i32::from(size) + i32::from(AXIS_MAX / 2)) / i32::from(AXIS_MAX)
    };
    let x = pixels(to.0, width) - pixels(from.0, width);
    let y = pixels(to.1, height) - pixels(from.1, height);
    let steps = (x.abs().max(y.abs()) + 126) / 127;
    (0..steps).map(move |i| {
        let step = |axis| (axis * (i + 1) / steps - axis * i / steps) as i8;
        (step(x), step(y))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_and_button_state_use_separate_reports() {
        let mut state = State::default();
        assert_eq!(absolute_report((5000, 10_000)), [0, 0, 0x40, 0xff, 0x7f]);
        assert_eq!(relative_report(1, 0, 0, (0, 0)), [1, 0, 0, 0, 0]);
        assert_eq!(relative_report(0, 0, 0, (0, 0)), [0; 5]);
        assert_eq!(relative_report(0, 0, 0, (-3, 4)), [0, 0, 0, 253, 4]);
        state.record_buttons(1, true);
        state.record_buttons(0, false);
        assert_eq!(state.buttons(), 1);
        state.record_buttons(0, true);
        assert_eq!(state.buttons(), 0);
    }

    #[test]
    fn coalesced_swipe_keeps_its_distance_in_single_steps() {
        assert_eq!(swipe_steps((4, 0)).collect::<Vec<_>>(), [(1, 0); 4]);
        assert_eq!(swipe_steps((0, -3)).collect::<Vec<_>>(), [(0, -1); 3]);
        assert!(!Event::Scroll {
            x: 0,
            y: 0,
            wheel: 9,
            pan: 0,
            start: true
        }
        .valid());
    }

    #[test]
    fn large_drag_deltas_split_without_losing_distance() {
        let steps: Vec<_> = drag_steps((1000, 9000), (9000, 1000), (800, 600)).collect();
        assert_eq!(steps.iter().map(|step| i32::from(step.0)).sum::<i32>(), 640);
        assert_eq!(
            steps.iter().map(|step| i32::from(step.1)).sum::<i32>(),
            -480
        );
        assert!(steps.iter().all(|&(x, y)| x > 0 && y < 0 && y != -128));
        assert_eq!(drag_steps((0, 0), (0, 0), (500, 500)).next(), None);
    }

    #[test]
    fn keyboard_releases_held_modifiers_on_leave() {
        let mut state = State::default();
        assert_eq!(state.keyboard(0xe1, true), Some([2, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!(state.keyboard(4, true), Some([2, 0, 4, 0, 0, 0, 0, 0]));
        assert_eq!(state.keyboard(4, true), None);
        assert!(state.release_keys());
        assert!(!state.release_keys());
        assert_eq!(state.keyboard(4, true), Some([0, 0, 4, 0, 0, 0, 0, 0]));
    }

    #[test]
    fn first_key_waits_for_click_focus_then_rapid_reports_keep_their_spacing() {
        let mut state = State::default();
        let clicked = Instant::now();
        assert_eq!(state.keyboard_ready_at(), None);
        state.note_primary_click(clicked);
        let focused = clicked + CLICK_TO_KEY_DELAY;
        assert_eq!(state.keyboard_ready_at(), Some(focused));
        state.record_keyboard_delivery(true, focused);
        assert_eq!(state.keyboard_ready_at(), Some(focused + KEY_REPORT_INTERVAL));
    }

    #[test]
    fn failed_key_up_still_requests_a_keyboard_release() {
        let mut state = State::default();
        state.keyboard(4, true);
        state.keyboard(4, false);
        state.record_keyboard_delivery(false, Instant::now());
        assert!(state.release_keys());
        state.record_keyboard_delivery(true, Instant::now());
        assert!(!state.release_keys());
    }
}
