use core::convert::Infallible;

use embedded_hal::digital::v2::InputPin;

use crate::utils::InfallibleResult;

const NCOLS: usize = 6;
const NROWS: usize = 5;

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

    /// Keyboard matrix coordinates have to be transformed to global representation
    pub fn transform_coordinates(&self, (row, col): (u8, u8)) -> (u8, u8) {
        match self {
            Self::Left => (row, col),
            Self::Right => (row, 2 * NCOLS as u8 - 1 - col),
        }
    }
}
