//! Board support package
//!
//! Code that builds on top of MCU-specific HAL (hal and hal_ext) to implement
//! support for the board and the peripherals located on it.

/// Low-level debugging via GPIO/UART
pub mod debug;
/// Analog joystick readings
pub mod joystick;
/// Definitions that depend on keyboard half side
pub mod sides;
/// USB classes
pub mod usb;
/// Driver for WS2812B RGB LEDs via SPI
pub mod ws2812b;

use crate::hal::gpio;

/// Number of columns keyboard half
pub const NCOLS: usize = 6;
/// Number of columns in the thumb cluster
pub const NCOLS_THUMB: usize = 4;
/// Number of key rows
pub const NROWS: usize = 5;

/// Type of GPIOs connected to key matrix columns
pub type ColPin = gpio::Pin<gpio::Input<gpio::PullUp>>;
/// Type of GPIOs connected to key matrix rows
pub type RowPin = gpio::Pin<gpio::Output<gpio::PushPull>>;
