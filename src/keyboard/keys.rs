use keyberon::{matrix, debounce, layout};

use crate::bsp::{NCOLS, NROWS, ColPin, RowPin, sides::BoardSide};
use crate::utils::InfallibleResult;
use super::leds::LedsBitset;

pub type PressedKeys = LedsBitset;

/// Keyboard key matrix scanner
pub struct Keys {
    matrix: matrix::Matrix<ColPin, RowPin, NCOLS, NROWS>,
    debouncer: debounce::Debouncer<[[bool; NCOLS]; NROWS]>,
    side: BoardSide,
    pressed: LedsBitset,
}

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
            pressed: Default::default(),
        }
    }

    /// Scan for key events; caller decides what to do with the events
    pub fn scan(&mut self) -> impl Iterator<Item = layout::Event> + '_ {
        let scan = self.matrix.get().infallible();
        self.debouncer.events(scan)
            .map(|e| {
                self.pressed.update_keys_on_event(e.clone());
                // Matrix produces local coordinates; make them global.
                e.transform(|i, j| self.side.coords_to_global((i, j)))
            })
    }

    /// Get board side
    pub fn side(&self) -> &BoardSide {
        &self.side
    }

    pub fn pressed(&self) -> PressedKeys {
        self.pressed
    }
}

impl PressedKeys {
    /// Update pressed keys from a layout event
    pub fn update_keys_on_event(&mut self, event: layout::Event) {
        let (row, col, state) = match event {
            layout::Event::Press(i, j) => (i, j, true),
            layout::Event::Release(i, j) => (i, j, false),
        };
        // Ignore joystick key
        if let Some(led) = BoardSide::led_number((row, col)) {
            self.set(led, state);
        }
    }

    pub fn is_pressed(&self, led_key: u8) -> bool {
        self.get(led_key)
    }
}
