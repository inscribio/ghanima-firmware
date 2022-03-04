use core::convert::Infallible;
use embedded_hal::digital::v2::InputPin;

use crate::utils::InfallibleResult;
use super::{NCOLS, NCOLS_THUMB, NROWS};

/// Side of a half of a split-keyboard
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BoardSide {
    Left,
    Right,
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

    fn coordinates_valid(&self, row: u8, col: u8) -> bool {
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

    /// Keyboard matrix coordinates have to be transformed to global representation
    pub fn transform_coordinates(&self, (row, col): (u8, u8)) -> (u8, u8) {
        let (row, col) = match self {
            Self::Left => (row, col),
            Self::Right => (row, 2 * NCOLS as u8 - 1 - col),
        };
        debug_assert!(self.coordinates_valid(row, col));
        (row, col)
    }

    /// Get relative key position for this side
    ///
    /// Returns key coordinates (X, Y) relative to the position of key in row=3 col=0
    /// (which has coordinates x=0, y=0). For the right half most keys will have negative
    /// X coordinate.
    pub fn key_position(&self, (row, col): (u8, u8)) -> (f32, f32) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_no_coordinates_translation() {
        let side = BoardSide::Left;
        assert_eq!(side.transform_coordinates((0, 0)), (0, 0));
        assert_eq!(side.transform_coordinates((1, 3)), (1, 3));
        assert_eq!(side.transform_coordinates((3, 5)), (3, 5));
        assert_eq!(side.transform_coordinates((4, 2)), (4, 2));
    }


    #[test]
    fn right_coordinates_translation() {
        let side = BoardSide::Right;
        assert_eq!(side.transform_coordinates((0, 0)), (0, 11));
        assert_eq!(side.transform_coordinates((1, 3)), (1, 8));
        assert_eq!(side.transform_coordinates((3, 5)), (3, 6));
        assert_eq!(side.transform_coordinates((4, 2)), (4, 9));
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
}
