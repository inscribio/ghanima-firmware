use bitfield::{Bit, BitMut};
use static_assertions as sa;
use serde::{Deserialize, Serialize};
use crate::bsp::NLEDS;

/// Bit-set storing led states as bit-flags (in the order of LEDs on PCB)
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
pub struct LedsBitset(pub u32);

sa::const_assert!(NLEDS <= 32);

impl LedsBitset {
    pub const ALL: Self = Self((1 << NLEDS) - 1);
    pub const NONE: Self = Self(0);

    pub const fn with_all(value: bool) -> Self {
        if value { Self::ALL } else { Self::NONE }
    }

    pub fn is_all(&self) -> bool {
        self == &Self::ALL
    }

    pub fn is_none(&self) -> bool {
        self == &Self::NONE
    }

    pub fn get(&self, led: u8) -> bool {
        debug_assert!(led < NLEDS as u8);
        self.0.bit(led as usize)
    }

    pub fn set(&mut self, led: u8, value: bool) {
        self.0.set_bit(led as usize, value);
    }
}

impl core::ops::Not for LedsBitset {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(!self.0 & Self::ALL.0) // mask to valid leds
    }
}

impl core::ops::BitAnd for LedsBitset {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl core::ops::BitOr for LedsBitset {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}
