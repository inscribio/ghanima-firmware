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
/// Number of "column-slots" in the thumb cluster
///
/// Note that joystick uses a separate column, but shares the "column-slot".
/// This count is the physical number of places where keys can exist, as
/// joystick replaces key (4, 0).
pub const NCOLS_THUMB: usize = 4;
/// Number of key rows
pub const NROWS: usize = 5;
/// Number of LEDs on each half (this is also the number of keys)
pub const NLEDS: usize = 28;

/// List of colors for all LEDs on a single half
pub type LedColors = [rgb::RGB8; NLEDS];

/// Type of GPIOs connected to key matrix columns
pub type ColPin = gpio::Pin<gpio::Input<gpio::PullUp>>;
/// Type of GPIOs connected to key matrix rows
pub type RowPin = gpio::Pin<gpio::Output<gpio::PushPull>>;

/// Perform blocking microsecond delay assuming 48 MHz CPU frequency
///
/// From measurements with logic analyzer:
/// * `cortex_m::asm::delay(1000)` takes 42 us
/// * `cortex_m::asm::delay(0)` takes 0.25 us
/// The above results have the overhead of `tasks::trace::run` (1.125 us) already
/// subtracted from the measured duration.
///
/// The theoretical time for 1000 cycle delay is 20.83 us.
#[inline(always)]
pub fn delay_us(us: u32) {
    const RATIO: u32 = 1000 / 42 + 1;
    cortex_m::asm::delay(us * RATIO);
}
