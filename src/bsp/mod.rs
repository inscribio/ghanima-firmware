//! Board support package
//!
//! Code that builds on top of MCU-specific HAL (hal and hal_ext) to implement
//! support for the board and the peripherals located on it.

pub mod debug;
pub mod joystick;
pub mod sides;
pub mod usb;
pub mod ws2812b;
