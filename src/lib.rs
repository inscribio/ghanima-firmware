#![no_std]

// Use std when running tests, see: https://stackoverflow.com/a/28186509
// Make sure to use different target when testing, e.g.
//   cargo test --target x86_64-unknown-linux-gnu
#[cfg(test)]
#[macro_use]
extern crate std;

use stm32f0xx_hal as hal;

pub mod bsp;
pub mod hal_ext;
pub mod utils;
