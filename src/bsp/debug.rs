use core::{self, mem::MaybeUninit};
use static_assertions as sa;

use crate::{hal, utils::InfallibleResult};
use hal::{gpio, prelude::*};

type Uart = hal::pac::USART2;
type Tx = gpio::gpioa::PA2<gpio::Alternate<gpio::AF1>>;
type Rx = gpio::gpioa::PA3<gpio::Alternate<gpio::AF1>>;
type Serial = hal::serial::Serial<Uart, Tx, Rx>;

type TxPin = gpio::gpioa::PA2<gpio::Output<gpio::PushPull>>;
type RxPin = gpio::gpioa::PA3<gpio::Output<gpio::PushPull>>;
type Pin = gpio::Pin<gpio::Output<gpio::PushPull>>;

/// UART pins that can be used as UART or as two PushPull GPIOs
pub struct DebugPins {
    serial: Serial,
    mode: Mode,
}

#[derive(PartialEq)]
enum Mode {
    Serial,
    Gpio,
}

impl DebugPins {
    /// Initialize debug pins
    pub fn new(uart: Uart, (tx, rx): (Tx, Rx), rcc: &mut hal::rcc::Rcc) -> Self {
        let serial = Serial::usart2(uart, (tx, rx), 115_200.bps(), rcc);
        Self { serial, mode: Mode::Serial }
    }

    #[inline(always)]
    fn reconfigure(&self, mode: Mode) {
        use Mode::*;

        let gpio = unsafe { &*hal::pac::GPIOA::ptr() };
        match (&self.mode, mode) {
            (Serial, Serial) | (Gpio, Gpio) => {},
            (Serial, Gpio) => {
                // into push-pull output (push-pull is default so don't change)
                gpio.moder.modify(|_, w| w.moder2().output().moder3().output());
            },
            (Gpio, Serial) => {
                // into alternate (number has already been configured by Serial)
                gpio.moder.modify(|_, w| w.moder2().alternate().moder3().alternate());
            },
        }
    }

    /// Use UART
    #[inline(always)]
    pub fn as_serial<F, T>(&mut self, f: F) -> T
    where
        F: FnOnce(&mut Serial) -> T,
    {
        self.reconfigure(Mode::Serial);
        f(&mut self.serial)
    }

    /// Use UART pins as GPIOs in push-pull output mode
    #[inline(always)]
    pub fn as_gpio<F, T>(&mut self, f: F) -> T
    where
        F: FnOnce(Pin, Pin) -> T,
    {
        self.reconfigure(Mode::Gpio);

        // Get the 0-sized pin structures, as there is no data this should be safe?
        sa::const_assert_eq!(core::mem::size_of::<TxPin>(), 0);
        sa::const_assert_eq!(core::mem::size_of::<RxPin>(), 0);
        let (tx, rx): (TxPin, RxPin) = unsafe {
            (MaybeUninit::uninit().assume_init(), MaybeUninit::uninit().assume_init())
        };

        // Downgrade to prevent usage of any of the into_*() methods
        let (tx, rx) = (tx.downgrade(), rx.downgrade());

        f(tx, rx)
    }

    /// Switch to GPIO mode and set TX pin value
    #[inline(always)]
    pub fn set_tx(&mut self, value: bool) {
        self.as_gpio(|mut tx, _| Self::set_pin_value(&mut tx, value))
    }

    /// Switch to GPIO mode and set RX pin value
    #[inline(always)]
    pub fn set_rx(&mut self, value: bool) {
        self.as_gpio(|_, mut rx| Self::set_pin_value(&mut rx, value))
    }

    /// Run given callback with TX pin set high
    #[inline(always)]
    pub fn with_tx_high<F, T>(&mut self, f: F) -> T
    where
        F: FnOnce() -> T
    {
        self.as_gpio(|tx, _| Self::with_pin_value(tx, true, f))
    }

    /// Run given callback with TX pin set low
    #[inline(always)]
    pub fn with_tx_low<F, T>(&mut self, f: F) -> T
    where
        F: FnOnce() -> T
    {
        self.as_gpio(|tx, _| Self::with_pin_value(tx, false, f))
    }

    /// Run given callback with RX pin set high
    #[inline(always)]
    pub fn with_rx_high<F, T>(&mut self, f: F) -> T
    where
        F: FnOnce() -> T
    {
        self.as_gpio(|_, rx| Self::with_pin_value(rx, true, f))
    }

    /// Run given callback with RX pin set low
    #[inline(always)]
    pub fn with_rx_low<F, T>(&mut self, f: F) -> T
    where
        F: FnOnce() -> T
    {
        self.as_gpio(|_, rx| Self::with_pin_value(rx, false, f))
    }

    /// Run code with given pin set to a concrete value (e.g. to measure with logic analyzer)
    #[inline(always)]
    fn with_pin_value<F, T>(mut pin: Pin, value: bool, f: F) -> T
    where
        F: FnOnce() -> T
    {
        let current = pin.is_set_high().infallible();
        if current == value {
            Self::set_pin_value(&mut pin, !value);
            cortex_m::asm::nop();
        }
        Self::set_pin_value(&mut pin, value);
        let result = f();
        Self::set_pin_value(&mut pin, !value);
        result
    }

    #[inline(always)]
    fn set_pin_value(pin: &mut Pin, value: bool) {
        if value {
            pin.set_high().infallible()
        } else {
            pin.set_low().infallible()
        };
    }
}

/// Helper macro for debugging a set of MCU registers
///
/// Uses [`defmt::println`] to pretty-print 32-bit registers in binary format splitting
/// nibbles/bytes using underscores for easier reading. Use as:
///
/// ```no_run
/// # #[macro_use] extern crate ghanima;
/// # use stm32f0xx_hal as hal;
/// let dma = unsafe { &*hal::pac::DMA1::ptr() };
/// let usart = unsafe { &*hal::pac::USART1::ptr() };
/// debug_regs! {
///     USART1: isr cr1 cr2 cr3 brr,
///     DMA1: isr ch2.cr ch3.cr ch5.cr,
/// };
/// ```
// FIXME: find way to use interned strings (=istr), stringify!/concat! do not work
#[macro_export]
macro_rules! debug_regs {
    ( $( $periph:ident: $( $($reg:ident).+ )+ ),+ $(,)? ) => {
        $( debug_regs! { @periph $periph $($($reg).+)+ } )+
    };
    ( @periph $periph:ident $( $($reg:ident).+ )+ ) => {
        let periph = unsafe { &*hal::pac::$periph::ptr() };
        $(
            defmt::println!(
                "{0=str}: {1=28..32:08b}_{1=24..28:08b}__{1=20..24:08b}_{1=16..20:08b}__{1=12..16:08b}_{1=8..12:08b}__{1=4..8:08b}_{1=0..4:08b}",
                concat!(
                    stringify!($periph),
                    $(
                        ".",
                        stringify!($reg)
                    ),+
                ),
                periph. $($reg).+ .read().bits()
            );
        )+
    };
}
