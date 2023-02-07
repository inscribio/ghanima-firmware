use serde::{Serialize, Deserialize};

use crate::bsp::{NROWS, NCOLS, NLEDS};
use crate::bsp::sides::{BoardSide, PerSide};
use crate::keyboard::hid::KeyboardLeds;
use crate::keyboard::keys::PressedKeys;
use crate::keyboard::role::Role;
use super::{Keys, Condition, KeyboardLed};

/// Collection of keyboard state variables that can be used as conditions
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyboardState {
    pub leds: KeyboardLeds,
    pub usb_on: bool,
    pub role: Role,
    pub layer: u8,
    pub pressed: PerSide<PressedKeys>,
}

impl Condition {
    /// Determine leds mask to which the condition applies
    ///
    /// Most conditions apply independently of [`super::Keys`], i.e. they apply to all or to none
    /// based on keyboard state, but [`Condition::Pressed`] actually filters the keys. Instead of
    /// calling `applies(self, state, side, led)` in a loop it is much faster to call this method
    /// once returning keys (leds) mask and then to use the mask while iterating over keys (leds).
    pub fn applies_to(&self, state: &KeyboardState, side: &BoardSide) -> PressedKeys {
        match self {
            Condition::Always => PressedKeys::with_all(true),
            Condition::Led(led) => PressedKeys::with_all(match led {
                KeyboardLed::NumLock => state.leds.num_lock(),
                KeyboardLed::CapsLock => state.leds.caps_lock(),
                KeyboardLed::ScrollLock => state.leds.scroll_lock(),
                KeyboardLed::Compose => state.leds.compose(),
                KeyboardLed::Kana => state.leds.kana(),
            }),
            Condition::UsbOn => PressedKeys::with_all(state.usb_on),
            Condition::Role(role) => PressedKeys::with_all(role == &state.role),
            Condition::Pressed => state.pressed[*side],
            Condition::KeyPressed(row, col) => {
                let checked_side = if side.has_coords((*row, *col)) {
                    *side
                } else {
                    side.other()
                };
                let local = BoardSide::coords_to_local((*row, *col));
                let is_pressed =
                BoardSide::led_number(local)
                    // FIXME: not possible to trigger on joystick press
                    // nor on keys from other side
                    .map(|led| state.pressed[checked_side].is_pressed(led))
                    .unwrap_or(false);
                PressedKeys::with_all(is_pressed)
            },
            Condition::Layer(layer) => PressedKeys::with_all(state.layer == *layer),
            Condition::Not(c) => !c.applies_to(state, side),
            Condition::And(conds) => conds.iter()
                .fold(PressedKeys::with_all(true), |acc, c| acc & c.applies_to(state, side)),
            Condition::Or(conds) => conds.iter()
                .fold(PressedKeys::with_all(false), |acc, c| acc | c.applies_to(state, side)),
        }
    }
}

pub trait RuleKeys {
    /// Internal iterator over key coordinates
    fn for_each<F: FnMut(u8, u8)>(&self, f: F);

    /// Internal iterator over led coordinates
    fn for_each_led<F: FnMut(u8)>(&self, f: F);
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

const ROW_LEDS_LOOKUP: [&[u8]; NROWS] = [
    &[ 0,  1,  2,  3,  4,  5],
    &[ 6,  7,  8,  9, 10, 11],
    &[12, 13, 14, 15, 16, 17],
    &[18, 19, 20, 21, 22, 23],
    &[24, 25, 26, 27],
];

const COL_LEDS_LOOKUP: [&[u8]; NCOLS * 2] = [
    // left (0 - 5)
    &[5,  6, 17, 18, 27],
    &[4,  7, 16, 19, 26],
    &[3,  8, 15, 20, 25],
    &[2,  9, 14, 21, 24],
    &[1, 10, 13, 22],
    &[0, 11, 12, 23],
    // right (6 - 11)
    &[0, 11, 12, 23],
    &[1, 10, 13, 22],
    &[2,  9, 14, 21, 24],
    &[3,  8, 15, 20, 25],
    &[4,  7, 16, 19, 26],
    &[5,  6, 17, 18, 27],
];

impl<'a> RuleKeys for Option<&'a Keys> {
    /// Iterate over all key positions (global)
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

    /// Iterate over all led positions (so always local)
    fn for_each_led<F: FnMut(u8)>(&self, mut f: F) {
        match self {
            None => for led in 0..(NLEDS as u8) {
                f(led);
            },
            Some(Keys::Rows(rows)) => {
                for row in rows.iter().copied() {
                    if let Some(leds) = ROW_LEDS_LOOKUP.get(row as usize) {
                        for led in leds.iter().copied() {
                            f(led);
                        }
                    }
                }
            },
            Some(Keys::Cols(cols)) => {
                for col in cols.iter().copied() {
                    if let Some(leds) = COL_LEDS_LOOKUP.get(col as usize) {
                        for led in leds.iter().copied() {
                            f(led);
                        }
                    }
                }
            },
            Some(Keys::Keys(keys)) => {
                for (row, col) in keys.iter() {
                    let (row, col) = BoardSide::coords_to_local((*row, *col));
                    if let Some(led) = BoardSide::led_number((row, col)) {
                        f(led);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::keyboard::leds::LedsBitset;

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

        // Also verify for_each_led, here the coordinates are always local so we just check if any
        // side is in the set.
        keys.for_each_led(|led| {
            let coords = BoardSide::led_coords(led);
            let other = BoardSide::Right.coords_to_global(coords);
            assert!(set.contains(&coords) || set.contains(&other), "{coords:?}/{other:?} not in set {set:?}");
        });
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
                left: LedsBitset(left),
                right: LedsBitset(right)
            },
        }
    }

    #[test]
    fn condition_pressed() {
        let cond = Condition::Pressed;
        let state = simple_keyboard_state(0b0000_0010, 0);
        let leds = cond.applies_to(&state, &BoardSide::Left);
        assert!(!leds.is_pressed(0));
        assert!(leds.is_pressed(1));
        assert!(!leds.is_pressed(2));
        assert_eq!(leds.0, 0b10);
    }

    #[test]
    fn condition_not() {
        let cond = Condition::Not(&Condition::Pressed);
        let state = simple_keyboard_state(0b0000_0010, 0);
        let leds = cond.applies_to(&state, &BoardSide::Left);
        assert!(leds.is_pressed(0));
        assert!(!leds.is_pressed(1));
        assert!(leds.is_pressed(2));
        assert_eq!(leds.0, 0b1111_11111111_11111111_11111101);
    }

    #[test]
    fn condition_and() {
        let cond = Condition::And(&[
            Condition::KeyPressed(0, 0), // led = (6 - 1) - 0 = 5
            Condition::KeyPressed(0, 3) // led = (6 - 1) - 3 = 2
        ]);
        let expected = [
            (0b0010_0100, true),
            (0b0000_0100, false),
            (0b0010_0000, false),
            (0b0000_0000, false),
        ];
        // Same for all leds (so leds is ALL or NONE)
        for led in 0..28 {
            for (pressed, expect) in expected {
                let leds = cond.applies_to(&simple_keyboard_state(pressed, 0), &BoardSide::Left);
                assert_eq!(leds.is_pressed(led), expect, "At led={led}, pressed={pressed:08b}");
            }
        }
    }

    #[test]
    fn condition_or() {
        let cond = Condition::Or(&[
            Condition::KeyPressed(0, 0), // led = (6 - 1) - 0 = 5
            Condition::KeyPressed(0, 3) // led = (6 - 1) - 3 = 2
        ]);
        let expected = [
            (0b0010_0100, true),
            (0b0000_0100, true),
            (0b0010_0000, true),
            (0b0000_0000, false),
        ];
        for led in 0..28 {
            for (pressed, expect) in expected {
                let leds = cond.applies_to(&simple_keyboard_state(pressed, 0), &BoardSide::Left);
                assert_eq!(leds.is_pressed(led), expect, "At led={led}, pressed={pressed:08b}");
            }
        }
    }

    #[test]
    fn condition_and_not() {
        let cond = Condition::And(&[
            Condition::Not(&Condition::KeyPressed(0, 0)), // led = (6 - 1) - 0 = 5
            Condition::KeyPressed(0, 3) // led = (6 - 1) - 3 = 2
        ]);
        let expected = [
            (0b0010_0100, false),
            (0b0000_0100, true),
            (0b0010_0000, false),
            (0b0000_0000, false),
        ];
        for led in 0..28 {
            for (pressed, expect) in expected {
                let leds = cond.applies_to(&simple_keyboard_state(pressed, 0), &BoardSide::Left);
                assert_eq!(leds.is_pressed(led), expect, "At led={led}, pressed={pressed:08b}");
            }
        }
    }
}
