use bitfield::bitfield;

use crate::bsp::NROWS;
use crate::bsp::sides::BoardSide;
use crate::keyboard::keys::PressedLedKeys;
use crate::keyboard::role::Role;
use super::{Keys, Condition, KeyboardLed};

/// Collection of keyboard state variables that can be used as conditions
#[derive(Clone)]
pub struct KeyboardState {
    pub leds: KeyboardLedsState,
    pub usb_on: bool,
    pub role: Role,
    pub layer: u8,
    pub pressed: PressedLedKeys,
}

/// Used to keep track of "event flags" for
#[derive(Clone)]
pub struct KeyboardStateEvents(KeyboardState);

bitfield! {
    #[derive(Clone, Copy, Default, PartialEq)]
    pub struct KeyboardLedsState(u8);
    pub num_lock, set_num_lock: 0;
    pub caps_lock, set_caps_lock: 1;
    pub scroll_lock, set_scroll_lock: 2;
    pub compose, set_compose: 3;
    pub kana, set_kana: 4;
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
            Condition::Pressed(pressed) => state.pressed.is_pressed(led) == *pressed,
            Condition::KeyPressed(pressed, (row, col)) => {
                let coords = side.coords_to_local((*row, *col));
                BoardSide::led_number(coords)
                    // FIXME: not possible to trigger on joystick press
                    // nor on keys from other side
                    .map(|led| state.pressed.is_pressed(led) == *pressed)
                    .unwrap_or(false)
            },
        }
    }
}

impl Keys {
    /// Internal iterator over key coordinates
    pub fn for_each<F: FnMut(u8, u8)>(&self, mut f: F) {
        // FIXME: any better implementation?
        match self {
            Self::All => {
                for row in 0..(NROWS as u8) {
                    for col in 0..BoardSide::n_cols(row) {
                        f(row, col);
                    }
                }
            },
            Self::Rows(rows) => {
                for row in rows.iter().copied() {
                    for col in 0..BoardSide::n_cols(row) {
                        f(row, col);
                    }
                }
            },
            Self::Cols(cols) => {
                for row in 0..(NROWS as u8) {
                    let n_cols = BoardSide::n_cols(row);
                    for col in cols.iter().copied().filter(|c| c < &n_cols) {
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

impl keyberon::keyboard::Leds for KeyboardLedsState {
    fn num_lock(&mut self, status: bool) { self.set_num_lock(status); }
    fn caps_lock(&mut self, status: bool) { self.set_caps_lock(status); }
    fn scroll_lock(&mut self, status: bool) { self.set_scroll_lock(status); }
    fn compose(&mut self, status: bool) { self.set_compose(status); }
    fn kana(&mut self, status: bool) { self.set_kana(status); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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
            &[(0, 0), (0, 3), (0, 5), (2, 5), (3, 5), (4, 0), (4, 3)],
            &[(0, 6), (3, 6), (4, 4), (4, 5)],
        );
    }

    #[test]
    fn keys_rows() {
        static ROWS: &[u8] = &[2, 4];
        test_keys_for_each(
            Keys::Rows(ROWS),
            &[(2, 0), (2, 5), (4, 0), (4, 3)],
            &[(0, 1), (3, 0), (4, 4)],
        );
    }

    #[test]
    fn keys_cols() {
        static COLS: &[u8] = &[0, 5];
        test_keys_for_each(
            Keys::Cols(COLS),
            &[(0, 0), (4, 0), (0, 5), (3, 5)],
            &[(0, 1), (2, 3), (4, 5)],
        );
    }

    #[test]
    fn keys_concrete() {
        static KEYS: &[(u8, u8)] = &[(0, 0), (1, 1), (2, 2), (3, 3)];
        test_keys_for_each(
            Keys::Keys(KEYS),
            &[(0, 0), (1, 1), (2, 2), (3, 3)],
            &[(0, 1), (2, 1), (4, 4)],
        );
    }
}
