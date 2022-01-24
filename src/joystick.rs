use embedded_hal::{digital::v2::InputPin, adc::OneShot};
use cortex_m::interrupt;
use crate::hal;
use crate::utils::InfallibleResult;
use hal::{gpio::{Analog, gpioa}, adc};

// TODO: later in the code we're using dedicated methods
type GpioX = gpioa::PA0<Analog>;
type GpioY = gpioa::PA1<Analog>;

pub struct Joystick {
    adc: hal::adc::Adc,
    x: GpioX,
    y: GpioY,
}

impl Joystick {
    pub fn new(adc: hal::pac::ADC, (x, y): (GpioX, GpioY), rcc: &mut hal::rcc::Rcc) -> Self {
        // Dedicated 14 MHz clock source is used. Conversion time is:
        // t_conv = (239.5 + 12.5) * (1/14e6) ~= 18 us
        let mut adc = hal::adc::Adc::new(adc, rcc);
        adc.set_align(adc::AdcAlign::Right);
        adc.set_precision(adc::AdcPrecision::B_12);
        adc.set_sample_time(adc::AdcSampleTime::T_239);

        Self { adc, x, y }
    }

    pub fn read(&mut self) -> (u16, u16) {
        // Current HAL implementation cannot return any error.
        let x = self.adc.read(&mut self.x).unwrap();
        let y = self.adc.read(&mut self.y).unwrap();
        (x, y)
    }

    /// Try to detect if the joystick is connected
    ///
    /// This is a bit hacky approach that temporarily enables  pull-up then pull-down,
    /// and compares the ADC read values. If there is a noticeable difference than we
    /// don't have joystick connected. With joystick connected pull-up/down are too
    /// weak to be visible.
    pub fn detect(&mut self) -> bool {
        const DELAY_CYCLES: u32 = 10;
        const MAX_DIFF: i16 = 300;

        let pull_up = self.detect_read(|w| w.pupdr0().pull_up().pupdr1().pull_up(), DELAY_CYCLES);
        let pull_down = self.detect_read(|w| w.pupdr0().pull_down().pupdr1().pull_down(), DELAY_CYCLES);
        // Restores state back to floating
        let floating = self.detect_read(|w| w.pupdr0().floating().pupdr1().floating(), DELAY_CYCLES);

        let detected = |_f, pu, pd| {
            (pu as i16 - pd as i16) < MAX_DIFF
        };

        defmt::debug!("Detecting X: f={=u16} pu={=u16} pd={=u16}", floating.0, pull_up.0, pull_down.0);
        defmt::debug!("Detecting Y: f={=u16} pu={=u16} pd={=u16}", floating.1, pull_up.1, pull_down.1);

        let x = detected(floating.0, pull_up.0, pull_down.0);
        let y = detected(floating.1, pull_up.1, pull_down.1);
        x || y
    }

    fn detect_read<F>(&mut self, f: F, delay: u32) -> (u16, u16)
    where
        F: FnOnce(&mut hal::pac::gpioa::pupdr::W) -> &mut hal::pac::gpioa::pupdr::W
    {
        let gpioa = unsafe { &*hal::pac::GPIOA::ptr() };
        gpioa.moder.modify(|_, w| w.moder0().input().moder1().input());
        gpioa.pupdr.modify(|_, w| f(w));
        cortex_m::asm::delay(delay);
        gpioa.moder.modify(|_, w| w.moder0().analog().moder1().analog());
        self.read()
    }
}
