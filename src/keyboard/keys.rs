use keyberon::{matrix, debounce, layout};

use crate::bsp::{NCOLS, NROWS, ColPin, RowPin, sides::BoardSide};
use crate::utils::InfallibleResult;

/// Keyboard key matrix scanner
pub struct Keys {
    matrix: matrix::Matrix<ColPin, RowPin, NCOLS, NROWS>,
    debouncer: debounce::Debouncer<matrix::PressedKeys<NCOLS, NROWS>>,
    side: BoardSide,
}

impl Keys {
    /// Initialize key matrix scanner with debouncing that requires `debounce_cnt` stable states
    pub fn new(
        side: BoardSide,
        cols: [ColPin; NCOLS],
        rows: [RowPin; NROWS],
        debounce_cnt: u16,
    ) -> Self {
        let initial = matrix::PressedKeys::default;
        Self {
            side,
            matrix: matrix::Matrix::new(cols, rows).infallible(),
            // TODO: could use better debouncing logic
            debouncer: debounce::Debouncer::new(initial(), initial(), debounce_cnt),
        }
    }

    /// Scan for key events; caller decides what to do with the events
    pub fn scan(&mut self) -> impl Iterator<Item = layout::Event> + '_ {
        let scan = self.matrix.get().infallible();
        self.debouncer.events(scan)
            .map(|e| {
                // Matrix produces local coordinates; make them global.
                e.transform(|i, j| self.side.transform_coordinates((i, j)))
            })
    }

    /// Get board side
    pub fn side(&self) -> &BoardSide {
        &self.side
    }
}
