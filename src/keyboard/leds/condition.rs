use serde::{Serialize, Deserialize};

use crate::bsp::{NROWS, NCOLS};
use crate::bsp::sides::{BoardSide, PerSide};
use crate::keyboard::hid::KeyboardLeds;
use crate::keyboard::keys::PressedLedKeys;
use crate::keyboard::role::Role;
use super::{Keys, Condition, KeyboardLed};

/// Collection of keyboard state variables that can be used as conditions
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyboardState {
    pub leds: KeyboardLeds,
    pub usb_on: bool,
    pub role: Role,
    pub layer: u8,
    pub pressed: PerSide<PressedLedKeys>,
}

impl Condition {
    /// Determine if condition applies
    ///
    /// The key is represented by `led` number as returned by [`BoardSide::led_number`].
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
            Condition::UsbOn => state.usb_on,
            Condition::Role(role) => role == &state.role,
            Condition::Pressed => state.pressed[*side].is_pressed(led),
            Condition::KeyPressed(row, col) => {
                let checked_side = if side.has_coords((*row, *col)) {
                    *side
                } else {
                    side.other()
                };
                let local = BoardSide::coords_to_local((*row, *col));
                BoardSide::led_number(local)
                    // FIXME: not possible to trigger on joystick press
                    // nor on keys from other side
                    .map(|led| state.pressed[checked_side].is_pressed(led))
                    .unwrap_or(false)
            },
            Condition::Layer(layer) => state.layer == *layer,
            Condition::Not(c) => !c.applies(state, side, led),
            Condition::And(conds) => conds.iter().all(|c| c.applies(state, side, led)),
            Condition::Or(conds) => conds.iter().any(|c| c.applies(state, side, led)),
        }
    }
}

pub trait RuleKeys {
    /// Internal iterator over key coordinates
    fn for_each<F: FnMut(u8, u8)>(&self, f: F);
}

fn cols_for_row(row: u8) -> impl Iterator<Item = u8> {
    (0..(2 * NCOLS as u8)).into_iter()
        .filter(move |col| col_in_row(*col, row))
}

fn col_in_row(col: u8, row: u8) -> bool {
    let row_cols = BoardSide::n_cols(row);
    let n_all_cols = 2 * NCOLS as u8;
    col < row_cols || (col >= (n_all_cols - row_cols) && col < n_all_cols)
}

impl<'a> RuleKeys for Option<&'a Keys> {
    fn for_each<F: FnMut(u8, u8)>(&self, mut f: F) {
        // FIXME: any better implementation?
        match self {
            None => {
                for row in 0..(NROWS as u8) {
                    for col in cols_for_row(row) {
                        f(row, col);
                    }
                }
            },
            Some(Keys::Rows(rows)) => {
                for row in rows.iter().copied() {
                    for col in cols_for_row(row) {
                        f(row, col);
                    }
                }
            },
            Some(Keys::Cols(cols)) => {
                for row in 0..(NROWS as u8) {
                    for col in cols.iter().copied().filter(|c| col_in_row(*c, row)) {
                        f(row, col);
                    }
                }
            },
            Some(Keys::Keys(keys)) => {
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
    fn col_in_row_ok() {
        for col in 0..=11 {
            assert!(col_in_row(col, 0), "col = {}", col);
        }
        assert!(!col_in_row(12, 0));

        for col in (0..=3).into_iter().chain(8..=11) {
            assert!(col_in_row(col, 4), "col = {}", col);
        }
        for col in 4..=7 {
            assert!(!col_in_row(col, 4), "col = {}", col);
        }
        assert!(!col_in_row(12, 4));
    }

    fn test_keys_for_each(keys: Option<&Keys>, contains: &[(u8, u8)], not_contains: &[(u8, u8)]) {
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
            None,
            &[(0, 0), (0, 3), (0, 5), (0, 7), (2, 5), (2, 11), (3, 5), (4, 0), (4, 3), (4, 8), (4, 11)],
            &[(0, 12), (2, 12), (4, 4), (4, 5), (4, 6), (4, 7), (4, 12)],
        );
    }

    #[test]
    fn keys_rows() {
        static ROWS: &[u8] = &[2, 4];
        test_keys_for_each(
            Some(&Keys::Rows(ROWS)),
            &[(2, 0), (2, 5), (2, 6), (2, 11), (4, 0), (4, 3)],
            &[(0, 1), (0, 7), (3, 0), (3, 9), (4, 4)],
        );
    }

    #[test]
    fn keys_cols() {
        static COLS: &[u8] = &[0, 5, 8];
        test_keys_for_each(
            Some(&Keys::Cols(COLS)),
            &[(0, 0), (4, 0), (0, 5), (3, 5), (1, 8), (3, 8)],
            &[(0, 1), (0, 10), (2, 3), (4, 5), (4, 9)],
        );
    }

    #[test]
    fn keys_concrete() {
        static KEYS: &[(u8, u8)] = &[(0, 0), (1, 1), (2, 2), (3, 3), (3, 7)];
        test_keys_for_each(
            Some(&Keys::Keys(KEYS)),
            &[(0, 0), (1, 1), (2, 2), (3, 3), (3, 7)],
            &[(0, 1), (2, 1), (3, 8), (4, 4)],
        );
    }

    fn simple_keyboard_state(left: u32, right: u32) -> KeyboardState {
        KeyboardState {
            leds: KeyboardLeds(0),
            usb_on: true,
            role: Role::Master,
            layer: 0,
            pressed: PerSide {
                left: PressedLedKeys::from_raw(left),
                right: PressedLedKeys::from_raw(right)
            },
        }
    }

    #[test]
    fn condition_pressed() {
        let cond = Condition::Pressed;
        let state = simple_keyboard_state(0b0000_0010, 0);
        assert!(!cond.applies(&state, &BoardSide::Left, 0));
        assert!(cond.applies(&state, &BoardSide::Left, 1));
        assert!(!cond.applies(&state, &BoardSide::Left, 2));
    }

    #[test]
    fn condition_not() {
        let cond = Condition::Not(&Condition::Pressed);
        let state = simple_keyboard_state(0b0000_0010, 0);
        assert!(cond.applies(&state, &BoardSide::Left, 0));
        assert!(!cond.applies(&state, &BoardSide::Left, 1));
        assert!(cond.applies(&state, &BoardSide::Left, 2));
    }

    #[test]
    fn condition_and() {
        let cond = Condition::And(&[
            Condition::KeyPressed(0, 0), // led = (6 - 1) - 0 = 5
            Condition::KeyPressed(0, 3) // led = (6 - 1) - 3 = 2
        ]);
        assert!(cond.applies(&simple_keyboard_state(0b0010_0100, 0), &BoardSide::Left, 20));
        assert!(!cond.applies(&simple_keyboard_state(0b0000_0100, 0), &BoardSide::Left, 20));
        assert!(!cond.applies(&simple_keyboard_state(0b0010_0000, 0), &BoardSide::Left, 20));
        assert!(!cond.applies(&simple_keyboard_state(0b0000_0000, 0), &BoardSide::Left, 20));
    }

    #[test]
    fn condition_or() {
        let cond = Condition::Or(&[
            Condition::KeyPressed(0, 0), // led = (6 - 1) - 0 = 5
            Condition::KeyPressed(0, 3) // led = (6 - 1) - 3 = 2
        ]);
        assert!(cond.applies(&simple_keyboard_state(0b0010_0100, 0), &BoardSide::Left, 20));
        assert!(cond.applies(&simple_keyboard_state(0b0000_0100, 0), &BoardSide::Left, 20));
        assert!(cond.applies(&simple_keyboard_state(0b0010_0000, 0), &BoardSide::Left, 20));
        assert!(!cond.applies(&simple_keyboard_state(0b0000_0000, 0), &BoardSide::Left, 20));
    }

    #[test]
    fn condition_and_not() {
        let cond = Condition::And(&[
            Condition::Not(&Condition::KeyPressed(0, 0)), // led = (6 - 1) - 0 = 5
            Condition::KeyPressed(0, 3) // led = (6 - 1) - 3 = 2
        ]);
        assert!(!cond.applies(&simple_keyboard_state(0b0010_0100, 0), &BoardSide::Left, 20));
        assert!(cond.applies(&simple_keyboard_state(0b0000_0100, 0), &BoardSide::Left, 20));
        assert!(!cond.applies(&simple_keyboard_state(0b0010_0000, 0), &BoardSide::Left, 20));
        assert!(!cond.applies(&simple_keyboard_state(0b0000_0000, 0), &BoardSide::Left, 20));
    }
}
