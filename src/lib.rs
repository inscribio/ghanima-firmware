//! Ghanima keyboard firmware
//!
//! Ghanima is an ergonomic split USB keyboard based on the [`keyberon`] firmware.
//! Additional features, beside regular keyboard functionality, include:
//!
//! * RGB LEDs under keys, with ability to control each individual LED
//! * Optional Joystick support that can act as USB HID mouse or be used as an
//!   encoder to control analog quantities, like e.g. system volume

#![no_std]

// Use std when running tests, see: https://stackoverflow.com/a/28186509
// Make sure to use different target when testing, e.g.
//   cargo test --target x86_64-unknown-linux-gnu
#[cfg(test)]
#[macro_use]
extern crate std;

use stm32f0xx_hal as hal;

pub mod bsp;
pub mod config;  // TODO: maybe remove from lib and only include in main?
pub mod hal_ext;
pub mod ioqueue;
pub mod keyboard;
pub mod utils;
