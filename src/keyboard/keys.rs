use keyberon::{matrix, debounce, layout};
use serde::{Deserialize, Serialize};

use crate::bsp::{NCOLS, NROWS, NLEDS, ColPin, RowPin, sides::BoardSide};
use crate::utils::InfallibleResult;

type PressedKeys = [[bool; NCOLS]; NROWS];

/// Keyboard key matrix scanner
pub struct Keys {
    matrix: matrix::Matrix<ColPin, RowPin, NCOLS, NROWS>,
    debouncer: debounce::Debouncer<PressedKeys>,
    side: BoardSide,
    pressed: PressedLedKeys,
}

/// Bit-set storing key states as bit-flags in the order of LEDs
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct PressedLedKeys(u32);

impl Keys {
    /// Initialize key matrix scanner with debouncing that requires `debounce_cnt` stable states
    pub fn new(
        side: BoardSide,
        cols: [ColPin; NCOLS],
        rows: [RowPin; NROWS],
        debounce_cnt: u16,
    ) -> Self {
        let initial = Default::default;
        Self {
            side,
            matrix: matrix::Matrix::new(cols, rows).infallible(),
            // TODO: could use better debouncing logic
            debouncer: debounce::Debouncer::new(initial(), initial(), debounce_cnt),
            pressed: PressedLedKeys(0),
        }
    }

    /// Scan for key events; caller decides what to do with the events
    pub fn scan(&mut self) -> impl Iterator<Item = layout::Event> + '_ {
        let scan = self.matrix.get().infallible();
        self.debouncer.events(scan)
            .map(|e| {
                self.pressed.update(&e);
                // Matrix produces local coordinates; make them global.
                e.transform(|i, j| self.side.coords_to_global((i, j)))
            })
    }

    /// Get board side
    pub fn side(&self) -> &BoardSide {
        &self.side
    }

    pub fn pressed(&self) -> PressedLedKeys {
        self.pressed
    }
}

impl PressedLedKeys {
    /// Get pressed state of key above given LED
    pub fn is_pressed(&self, led: u8) -> bool {
        debug_assert!(led < NLEDS as u8);
        (self.0 & (1 << led)) != 0
    }

    pub fn update(&mut self, event: &layout::Event) {
        let (row, col, state) = match event {
            layout::Event::Press(i, j) => (i, j, true),
            layout::Event::Release(i, j) => (i, j, false),
        };
        // Ignore joystick key
        if let Some(led) = BoardSide::led_number((*row, *col)) {
            let bitmask = 1 << led;
            if state {
                self.0 |= bitmask;
            } else {
                self.0 &= !bitmask;
            }
        }
    }

    /// Get the raw internal state
    pub fn get_raw(&self) -> u32 {
        self.0
    }

    #[cfg(test)]
    pub fn new_raw(keys: u32) -> Self {
        Self(keys)
    }
}
