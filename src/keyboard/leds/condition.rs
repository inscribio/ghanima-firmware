use serde::{Serialize, Deserialize};

use crate::bsp::{NROWS, NCOLS};
use crate::bsp::sides::BoardSide;
use crate::keyboard::hid::KeyboardLeds;
use crate::keyboard::keys::PressedLedKeys;
use crate::keyboard::role::Role;
use super::{Keys, Condition, KeyboardLed, LedController};

/// Collection of keyboard state variables that can be used as conditions
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyboardState {
    pub leds: KeyboardLeds,
    pub usb_on: bool,
    pub role: Role,
    pub layer: u8,
    pub pressed_left: PressedLedKeys,
    pub pressed_right: PressedLedKeys,
}

/// Used to keep track of "event flags" for
#[derive(Clone)]
pub struct KeyboardStateEvents(KeyboardState);

impl KeyboardState {
    /// Apply LED controller state update
    pub fn update(self, time: u32, controller: &mut LedController) {
        controller.update_patterns(time, self);
    }

    /// Get pressed keys for given board side
    pub fn pressed(&self, side: &BoardSide) -> PressedLedKeys {
        match side {
            BoardSide::Left => self.pressed_left,
            BoardSide::Right => self.pressed_right,
        }
    }
}

impl Condition {
    pub fn applies(&self, state: &KeyboardState, side: &BoardSide, led: u8) -> bool {
        match self {
            Condition::Always => true,
            Condition::Led(led) => match led {
                KeyboardLed::NumLock => state.leds.num_lock(),
                KeyboardLed::CapsLock => state.leds.caps_lock(),
                KeyboardLed::ScrollLock => state.leds.scroll_lock(),
                KeyboardLed::Compose => state.leds.compose(),
                KeyboardLed::Kana => state.leds.kana(),
            },
            Condition::UsbOn(usb_on) => usb_on == &state.usb_on,
            Condition::Role(role) => role == &state.role,
            Condition::Pressed(pressed) => state.pressed(side).is_pressed(led) == *pressed,
            Condition::KeyPressed(pressed, global) => {
                let checked_side = if side.has_coords(*global) {
                    *side
                } else {
                    side.other()
                };
                let local = BoardSide::coords_to_local(*global);
                BoardSide::led_number(local)
                    // FIXME: not possible to trigger on joystick press
                    // nor on keys from other side
                    .map(|led| state.pressed(&checked_side).is_pressed(led) == *pressed)
                    .unwrap_or(false)
            },
            Condition::Not(c) => !c.applies(state, side, led),
        }
    }
}

impl Keys {
    fn cols_for_row(&self, row: u8) -> impl Iterator<Item = u8> {
        (0..(2 * NCOLS as u8)).into_iter()
            .filter(move |col| Self::col_in_row(*col, row))
    }

    fn col_in_row(col: u8, row: u8) -> bool {
        let row_cols = BoardSide::n_cols(row);
        let n_all_cols = 2 * NCOLS as u8;
        col < row_cols || (col >= (n_all_cols - row_cols) && col < n_all_cols)
    }

    /// Internal iterator over key coordinates
    pub fn for_each<F: FnMut(u8, u8)>(&self, mut f: F) {
        // FIXME: any better implementation?
        match self {
            Self::All => {
                for row in 0..(NROWS as u8) {
                    for col in self.cols_for_row(row) {
                        f(row, col);
                    }
                }
            },
            Self::Rows(rows) => {
                for row in rows.iter().copied() {
                    for col in self.cols_for_row(row) {
                        f(row, col);
                    }
                }
            },
            Self::Cols(cols) => {
                for row in 0..(NROWS as u8) {
                    for col in cols.iter().copied().filter(|c| Self::col_in_row(*c, row)) {
                        f(row, col);
                    }
                }
            },
            Self::Keys(keys) => {
                for (row, col) in keys.iter().copied() {
                    f(row, col)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn col_in_row() {
        for col in 0..=11 {
            assert!(Keys::col_in_row(col, 0), "col = {}", col);
        }
        assert!(!Keys::col_in_row(12, 0));

        for col in (0..=3).into_iter().chain(8..=11) {
            assert!(Keys::col_in_row(col, 4), "col = {}", col);
        }
        for col in 4..=7 {
            assert!(!Keys::col_in_row(col, 4), "col = {}", col);
        }
        assert!(!Keys::col_in_row(12, 4));
    }

    fn test_keys_for_each(keys: Keys, contains: &[(u8, u8)], not_contains: &[(u8, u8)]) {
        let mut set = HashSet::new();
        keys.for_each(|row, col| {
            let added = set.insert((row, col));
            assert!(added, "Key repeated: {} {}", row, col);
        });
        for coords in contains {
            assert!(set.contains(&coords), "Key not found: {:?}", coords);
        }
        for coords in not_contains {
            assert!(!set.contains(&coords), "Key found: {:?}", coords);
        }
    }

    #[test]
    fn keys_all() {
        test_keys_for_each(
            Keys::All,
            &[(0, 0), (0, 3), (0, 5), (0, 7), (2, 5), (2, 11), (3, 5), (4, 0), (4, 3), (4, 8), (4, 11)],
            &[(0, 12), (2, 12), (4, 4), (4, 5), (4, 6), (4, 7), (4, 12)],
        );
    }

    #[test]
    fn keys_rows() {
        static ROWS: &[u8] = &[2, 4];
        test_keys_for_each(
            Keys::Rows(ROWS),
            &[(2, 0), (2, 5), (2, 6), (2, 11), (4, 0), (4, 3)],
            &[(0, 1), (0, 7), (3, 0), (3, 9), (4, 4)],
        );
    }

    #[test]
    fn keys_cols() {
        static COLS: &[u8] = &[0, 5, 8];
        test_keys_for_each(
            Keys::Cols(COLS),
            &[(0, 0), (4, 0), (0, 5), (3, 5), (1, 8), (3, 8)],
            &[(0, 1), (0, 10), (2, 3), (4, 5), (4, 9)],
        );
    }

    #[test]
    fn keys_concrete() {
        static KEYS: &[(u8, u8)] = &[(0, 0), (1, 1), (2, 2), (3, 3), (3, 7)];
        test_keys_for_each(
            Keys::Keys(KEYS),
            &[(0, 0), (1, 1), (2, 2), (3, 3), (3, 7)],
            &[(0, 1), (2, 1), (3, 8), (4, 4)],
        );
    }
}
