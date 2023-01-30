use core::convert::Infallible;
use embedded_hal::digital::v2::InputPin;
use serde::{Serialize, Deserialize};

use crate::utils::InfallibleResult;
use super::{NCOLS, NCOLS_THUMB, NROWS};

/// Side of a half of a split-keyboard
#[derive(PartialEq, Eq, Clone, Copy)]
#[cfg_attr(test, derive(Debug))]
pub enum BoardSide {
    Left,
    Right,
}

/// Storage of data for both sides of keyboard with [`BoardSide`] indexing
#[derive(PartialEq, Clone, Default, Serialize, Deserialize)]
pub struct PerSide<T> {
    pub left: T,
    pub right: T,
}

impl BoardSide {
    /// Board side can be determined via pull-up/down on a pin
    pub fn get(pin: impl InputPin<Error = Infallible>) -> Self {
        if pin.is_high().infallible() {
            Self::Left
        } else {
            Self::Right
        }
    }

    /// Get the other side
    pub const fn other(&self) -> BoardSide {
        match self {
            BoardSide::Left => BoardSide::Right,
            BoardSide::Right => BoardSide::Left,
        }
    }

    const fn coordinates_valid(&self, row: u8, col: u8) -> bool {
        let (row, col) = ((row as usize), (col as usize));
        let row_valid = row < NROWS;
        let is_thumb = row == NROWS - 1;
        let ncols = if is_thumb { NCOLS_THUMB } else { NCOLS };
        let col_valid = match self {
            Self::Left => col < ncols,
            Self::Right => (col < 2 * NCOLS) && (col >= 2 * NCOLS - ncols),
        };
        row_valid && col_valid
    }

    const fn reflect_col(col: u8) -> u8 {
        2 * NCOLS as u8 - 1 - col
    }

    /// Transform local key coordinates to global coordinates
    pub const fn coords_to_global(&self, (row, col): (u8, u8)) -> (u8, u8) {
        let col = match self {
            Self::Left => col,
            Self::Right => Self::reflect_col(col),
        };
        debug_assert!(self.coordinates_valid(row, col));
        (row, col)
    }

    /// Transform global key coordinates to local coordinates
    pub const fn coords_to_local((row, col): (u8, u8)) -> (u8, u8) {
        let col =  if col >= NCOLS as u8 {
            Self::reflect_col(col)
        } else {
            col
        };
        (row, col)
    }

    pub const fn has_coords(&self, (_row, col): (u8, u8)) -> bool {
        match self {
            BoardSide::Left => col < NCOLS as u8,
            BoardSide::Right => col >= NCOLS as u8,
        }
    }

    pub const fn from_coords((row, col): (u8, u8)) -> Self {
        debug_assert!(BoardSide::Left.coordinates_valid(row, col) || BoardSide::Right.coordinates_valid(row, col));
        if BoardSide::Left.has_coords((row, col)) {
            BoardSide::Left
        } else {
            BoardSide::Right
        }
    }

    /// Get relative key position for this side
    ///
    /// Row and column must be valid, side-local key coordinates.
    /// Returns key coordinates (X, Y) relative to the position of key in row=3 col=0
    /// (which has coordinates x=0, y=0). For the right half most keys will have negative
    /// X coordinate.
    pub const fn key_position(&self, (row, col): (u8, u8)) -> (f32, f32) {
        match self {
            Self::Left => match (row, col) {
                (0, 0) => (  0.00,  57.15),
                (0, 1) => ( 19.05,  59.53),
                (0, 2) => ( 38.10,  69.06),
                (0, 3) => ( 57.15,  73.82),
                (0, 4) => ( 76.20,  69.06),
                (0, 5) => ( 95.25,  65.72),
                (1, 0) => (  0.00,  38.10),
                (1, 1) => ( 19.05,  40.48),
                (1, 2) => ( 38.10,  50.01),
                (1, 3) => ( 57.15,  54.77),
                (1, 4) => ( 76.20,  50.01),
                (1, 5) => ( 95.25,  46.67),
                (2, 0) => (  0.00,  19.05),
                (2, 1) => ( 19.05,  21.43),
                (2, 2) => ( 38.10,  30.96),
                (2, 3) => ( 57.15,  35.72),
                (2, 4) => ( 76.20,  30.96),
                (2, 5) => ( 95.25,  27.62),
                (3, 0) => (  0.00,   0.00),
                (3, 1) => ( 19.05,   2.38),
                (3, 2) => ( 38.10,  11.91),
                (3, 3) => ( 57.15,  16.67),
                (3, 4) => ( 76.20,  11.91),
                (3, 5) => ( 95.25,   8.57),
                (4, 0) => ( 68.07, -10.10),
                (4, 1) => ( 88.95, -11.94),
                (4, 2) => (108.50, -19.48),
                (4, 3) => (125.20, -32.14),
                _ => unreachable!(),
            },
            Self::Right => match (row, col) {
                (0, 0) => (   0.00,  57.15),
                (0, 1) => ( -19.05,  59.53),
                (0, 2) => ( -38.10,  69.06),
                (0, 3) => ( -57.15,  73.82),
                (0, 4) => ( -76.20,  69.06),
                (0, 5) => ( -95.25,  65.72),
                (1, 0) => (   0.00,  38.10),
                (1, 1) => ( -19.05,  40.48),
                (1, 2) => ( -38.10,  50.01),
                (1, 3) => ( -57.15,  54.77),
                (1, 4) => ( -76.20,  50.01),
                (1, 5) => ( -95.25,  46.67),
                (2, 0) => (   0.00,  19.05),
                (2, 1) => ( -19.05,  21.43),
                (2, 2) => ( -38.10,  30.96),
                (2, 3) => ( -57.15,  35.72),
                (2, 4) => ( -76.20,  30.96),
                (2, 5) => ( -95.25,  27.62),
                (3, 0) => (   0.00,   0.00),
                (3, 1) => ( -19.05,   2.38),
                (3, 2) => ( -38.10,  11.91),
                (3, 3) => ( -57.15,  16.67),
                (3, 4) => ( -76.20,  11.91),
                (3, 5) => ( -95.25,   8.57),
                (4, 0) => ( -68.07, -10.10),
                (4, 1) => ( -88.95, -11.94),
                (4, 2) => (-108.50, -19.48),
                (4, 3) => (-125.20, -32.14),
                _ => unreachable!(),
            },
        }
    }

    /// Get number of "column-slots" in given row (see [`NCOLS_THUMB`])
    pub const fn n_cols(row: u8) -> u8 {
        let is_thumb = row == (NROWS as u8 - 1);
        if is_thumb { NCOLS_THUMB as u8 } else { NCOLS as u8 }
    }

    /// Get RGB LED position (number in the chain) for a given key
    ///
    /// Row and column must be valid, side-local key coordinates.
    pub const fn led_number((row, col): (u8, u8)) -> Option<u8> {
        // Special case for joystick which has no LED
        if row == 4 && col == 4 {
            None
        } else {
            // Both sides are routed in the same way
            // LED numbers in odd rows increase with column, and decrease in even rows
            let rows_before = row * NCOLS as u8;
            let this_row = if row % 2 == 0 {
                (Self::n_cols(row) - 1) - col
            } else {
                col
            };
            Some(rows_before + this_row)
        }
    }

    /// Get side-local key coordinates for given RGB LED
    pub const fn led_coords(led: u8) -> (u8, u8) {
        let row = led / (NCOLS as u8);
        let led_rem = led % (NCOLS as u8);
        let col = if row % 2 == 0 {
            (Self::n_cols(row) - 1) - led_rem
        } else {
            led_rem
        };
        (row, col)
    }

}

impl<T> PerSide<T> {
    pub const fn as_array(&self) -> [&T; 2] {
        [&self.left, &self.right]
    }

    pub fn as_array_mut(&mut self) -> [&mut T; 2] {
        [&mut self.left, &mut self.right]
    }
}

impl<T> core::ops::Index<BoardSide> for PerSide<T> {
    type Output = T;

    fn index(&self, index: BoardSide) -> &Self::Output {
        match index {
            BoardSide::Left => &self.left,
            BoardSide::Right => &self.right,
        }
    }
}

impl<T> core::ops::IndexMut<BoardSide> for PerSide<T> {
    fn index_mut(&mut self, index: BoardSide) -> &mut Self::Output {
        match index {
            BoardSide::Left => &mut self.left,
            BoardSide::Right => &mut self.right,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_to_local() {
        assert_eq!(BoardSide::coords_to_local((1, 3)), (1, 3));
        assert_eq!(BoardSide::coords_to_local((1, 8)), (1, 3));
        assert_eq!(BoardSide::coords_to_local((1, 4)), (1, 4));
        assert_eq!(BoardSide::coords_to_local((1, 7)), (1, 4));
    }

    fn test_coordinates_translation(side: BoardSide, coords: &[((u8, u8), (u8, u8))]) {
        for (local, global) in coords {
            assert_eq!(side.coords_to_global(*local), *global, "L{:?} -> G{:?}", local, global);
            assert_eq!(BoardSide::coords_to_local(*global), *local, "G{:?} -> L{:?}", global, local);
        }
    }

    #[test]
    fn left_no_coordinates_translation() {
        test_coordinates_translation(BoardSide::Left, &[
            ((0, 0), (0, 0)),
            ((1, 3), (1, 3)),
            ((3, 5), (3, 5)),
            ((4, 2), (4, 2)),
        ]);
    }


    #[test]
    fn right_coordinates_translation() {
        test_coordinates_translation(BoardSide::Right, &[
            ((0, 0), (0, 11)),
            ((1, 3), (1, 8)),
            ((3, 5), (3, 6)),
            ((4, 2), (4, 9)),
        ]);
    }

    type Range = std::ops::RangeInclusive<u8>;

    fn valid_coordinates(side: &BoardSide, rows: Range, cols: Range, valid: bool) {
        for row in rows {
            for col in cols.clone() {
                let result = side.coordinates_valid(row, col);
                assert_eq!(result, valid,
                   "{:?}, row={}, col={} => valid={} vs expected {}",
                   side, row, col, result, valid
               );
            }
        }
    }

    #[test]
    fn valid_coordinates_left() {
        let side = BoardSide::Left;
        // main
        valid_coordinates(&side, 0..=3, 0..=5, true);
        valid_coordinates(&side, 0..=3, 6..=12, false);
        // thumb
        valid_coordinates(&side, 4..=4, 0..=3, true);
        valid_coordinates(&side, 4..=4, 4..=12, false);
        // out of range
        valid_coordinates(&side, 5..=6, 0..=12, false);
    }

    #[test]
    fn valid_coordinates_right() {
        let side = BoardSide::Right;
        // main
        valid_coordinates(&side, 0..=3, 0..=5, false);
        valid_coordinates(&side, 0..=3, 6..=11, true);
        valid_coordinates(&side, 0..=3, 12..=12, false);
        // thumb
        valid_coordinates(&side, 4..=4, 0..=7, false);
        valid_coordinates(&side, 4..=4, 8..=11, true);
        valid_coordinates(&side, 4..=4, 12..=12, false);
        // out of range
        valid_coordinates(&side, 5..=6, 0..=12, false);
    }

    #[test]
    fn led_number_coords_conversion() {
        let verify = |coords: (u8, u8), led: u8| {
            assert_eq!(BoardSide::led_number(coords), Some(led), "Wrong conversion from coordinates to led number");
            assert_eq!(BoardSide::led_coords(led), coords, "Wrong conversion from led number to coordinates");
        };
        verify((0, 5), 0);
        verify((0, 2), 3);
        verify((0, 0), 5);
        verify((1, 0), 6);
        verify((1, 1), 7);
        verify((1, 5), 11);
        verify((2, 4), 13);
        verify((2, 0), 17);
        verify((3, 1), 19);
        verify((4, 3), 24);
        verify((4, 2), 25);
        verify((4, 0), 27);
        // Special case: joystick key shares LED with key (4, 0)
        assert_eq!(BoardSide::led_number((4, 4)), None);
    }

    #[test]
    fn side_from_coords() {
        for row in 0..=3 {
            for col in 0..=5 {
                assert_eq!(BoardSide::from_coords((row, col)), BoardSide::Left);
            }
            for col in 6..=11 {
                assert_eq!(BoardSide::from_coords((row, col)), BoardSide::Right);
            }
        }
        for col in 0..=3 {
            assert_eq!(BoardSide::from_coords((4, col)), BoardSide::Left);
        }
        for col in 8..=11 {
            assert_eq!(BoardSide::from_coords((4, col)), BoardSide::Right);
        }
    }
}
