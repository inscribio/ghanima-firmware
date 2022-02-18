//! Board support package
//!
//! Code that builds on top of MCU-specific HAL (hal and hal_ext) to implement
//! support for the board and the peripherals located on it.

pub mod debug;
pub mod joystick;
pub mod sides;
pub mod usb;
pub mod ws2812b;

use crate::hal::gpio;

pub const NCOLS: usize = 6;
pub const NCOLS_THUMB: usize = 4;
pub const NROWS: usize = 5;

pub type ColPin = gpio::Pin<gpio::Input<gpio::PullUp>>;
pub type RowPin = gpio::Pin<gpio::Output<gpio::PushPull>>;
