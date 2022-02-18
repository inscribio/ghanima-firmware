use core::convert::Infallible;
use embedded_hal::digital::v2::InputPin;

use crate::utils::InfallibleResult;
use super::{NCOLS, NCOLS_THUMB, NROWS};

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
