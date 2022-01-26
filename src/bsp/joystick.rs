use embedded_hal::adc::OneShot;
use micromath::F32Ext;

use crate::hal;
use hal::{gpio::{Analog, gpioa}, adc};

// TODO: later in the code we're using dedicated methods
type GpioX = gpioa::PA1<Analog>;
type GpioY = gpioa::PA0<Analog>;

pub struct Joystick {
    adc: hal::adc::Adc,
    zero: (u16, u16),
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

        let mut joy = Self { adc, x, y, zero: (0, 0) };
        joy.calibrate_zero();

        joy
    }

    fn read_raw(&mut self) -> (u16, u16) {
        // Current HAL implementation cannot return any error.
        let x = self.adc.read(&mut self.x).unwrap();
        let y = self.adc.read(&mut self.y).unwrap();
        (x, y)
    }

    /// Re-calibrate joystick zero position
    // TODO: hard-code to avoid issues if starting with joystick in non-zero
    pub fn calibrate_zero(&mut self) {
        self.zero = self.read_raw();
    }

    pub fn read_xy(&mut self) -> (i16, i16) {
        let (x, y) = self.read_raw();
        let x = x as i16 - self.zero.0 as i16;
        let y = y as i16 - self.zero.1 as i16;
        // by default x grows to the left, y grows down
        (-x, -y)
    }

    // On left side:
    //   x: larger left, lower right
    //   y: larger up, lower down
    pub fn read_polar(&mut self) -> (f32, f32) {
        let (x, y) = self.read_xy();
        let (x, y) = (x as f32, y as f32);
        let r = (x.powi(2) + y.powi(2)).sqrt();
        let a = y.atan2_norm(x);
        (r, a)
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
        self.read_raw()
    }
}
