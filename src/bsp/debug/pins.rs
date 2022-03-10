use core::{self, mem::MaybeUninit};

use crate::{hal, utils::InfallibleResult};
use hal::prelude::*;

use super::{types::*, get_tx, get_rx};

/// UART pins that can be used as UART or as two PushPull GPIOs
pub struct DebugPins {
    serial: Serial,
    mode: Mode,
}

/// [`DebugPins`] usable only as GPIO that can be used without mutable reference
///
/// This is mainly useful to avoid using locks when debugging via GPIOs.
/// Note that, because this interface can be used via shared references,
/// there is no guarantee that usage from other threads won't mess up GPIO
/// states set in the current thread.
pub struct DebugGpio(());

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

    pub fn into_gpio(self) -> DebugGpio {
        self.reconfigure(Mode::Gpio);
        DebugGpio(())
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

        // Downgrade to prevent usage of any of the into_*() methods
        let (tx, rx) = unsafe { (get_tx().downgrade(), get_rx().downgrade()) };

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

impl DebugGpio {
    fn tx(&self) -> Pin {
        let pin: TxPin = unsafe { MaybeUninit::uninit().assume_init() };
        pin.downgrade()
    }

    fn rx(&self) -> Pin {
        let pin: RxPin = unsafe { MaybeUninit::uninit().assume_init() };
        pin.downgrade()
    }

    /// Set TX pin value
    #[inline(always)]
    pub fn set_tx(&self, value: bool) {
        DebugPins::set_pin_value(&mut self.tx(), value);
    }

    /// Set RX pin value
    #[inline(always)]
    pub fn set_rx(&self, value: bool) {
        DebugPins::set_pin_value(&mut self.rx(), value);
    }

    /// Run given callback with TX pin set high
    #[inline(always)]
    pub fn with_tx_high<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T
    {
        DebugPins::with_pin_value(self.tx(), true, f)
    }

    /// Run given callback with TX pin set low
    #[inline(always)]
    pub fn with_tx_low<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T
    {
        DebugPins::with_pin_value(self.tx(), false, f)
    }

    /// Run given callback with RX pin set high
    #[inline(always)]
    pub fn with_rx_high<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T
    {
        DebugPins::with_pin_value(self.rx(), true, f)
    }

    /// Run given callback with RX pin set low
    #[inline(always)]
    pub fn with_rx_low<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T
    {
        DebugPins::with_pin_value(self.rx(), false, f)
    }
}
