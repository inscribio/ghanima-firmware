use embedded_hal::adc::OneShot;
use micromath::F32Ext;

use crate::hal;
use hal::{gpio::{Analog, gpioa}, adc};

// TODO: later in the code we're using dedicated methods
type GpioX = gpioa::PA1<Analog>;
type GpioY = gpioa::PA0<Analog>;

/// Two-axis joystick read via ADC
pub struct Joystick {
    adc: hal::adc::Adc,
    zero: (u16, u16),
    x: GpioX,
    y: GpioY,
}

impl Joystick {
    /// Initialize and calibrate joystick's ADC
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

    fn offset_xy((x, y): (u16, u16), zero: (u16, u16)) -> (i16, i16) {
        let x = x as i16 - zero.0 as i16;
        let y = y as i16 - zero.1 as i16;
        (x, y)
    }

    /// Get XY coordinates centered around the calibrated zero point
    pub fn read_xy(&mut self) -> (i16, i16) {
        let (x, y) = Self::offset_xy(self.read_raw(), self.zero);
        // by default x grows to the left, y grows down
        (-x, -y)
    }

    fn to_polar((x, y): (i16, i16)) -> (f32, f32) {
        let (x, y) = (x as f32, y as f32);
        let r = (x.powi(2) + y.powi(2)).sqrt();
        let a = y.atan2_norm(x);
        (r, a)
    }

    // On left side:
    //   x: larger left, lower right
    //   y: larger up, lower down
    /// Read joystick position as polar coordinates (R, 𝛗) with 𝛗 normalized to [0, 4) (quadrant)
    pub fn read_polar(&mut self) -> (f32, f32) {
        Self::to_polar(self.read_xy())
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

#[cfg(test)]
mod tests {
    use super::*;
    use assert_float_eq::*;

    #[test]
    fn coordinate_offset() {
        let zero = (2100, 2200);
        let xy = |x, y| Joystick::offset_xy((x, y), zero);
        assert_eq!(xy(2300, 2200), (200, 0));
        assert_eq!(xy(1000, 2000), (-1100, -200));
        assert_eq!(xy(0, 4095), (-2100, 1895));
    }

    // transform angle [-pi, pi] to rot [0, 4]
    fn angle2rot(angle: f32) -> f32 {
        if angle >= 0.0 {
            angle / std::f32::consts::FRAC_PI_2
        } else {
            angle / std::f32::consts::FRAC_PI_2 + 4.0
        }
    }

    #[test]
    fn angle2rot_conversion() {
        assert_float_absolute_eq!(angle2rot(0.0), 0.0);
        assert_float_absolute_eq!(angle2rot(std::f32::consts::FRAC_PI_2), 1.0);
        assert_float_absolute_eq!(angle2rot(std::f32::consts::PI), 2.0);
        assert_float_absolute_eq!(angle2rot(-std::f32::consts::PI), 2.0);
        assert_float_absolute_eq!(angle2rot(-std::f32::consts::FRAC_PI_2), 3.0);
    }

    #[test]
    fn polar_coordinates() {
        let assert = |got: (f32, f32), expected: (f32, f32)| {
            assert_float_relative_eq!(got.0, expected.0);
            assert_float_relative_eq!(got.1, angle2rot(expected.1), 0.002);
        };
        assert(Joystick::to_polar((20, 300)), (300.66592756745814, 1.5042281630190728));
        assert(Joystick::to_polar((-10, 10)), (14.142135623730951, 2.356194490192345));
        assert(Joystick::to_polar((-16, -2)), (16.1245154965971, -3.017237659043032));
        assert(Joystick::to_polar((2000, -1)), (2000.0002499999844, -0.0004999999583333395));
        assert_eq!(Joystick::to_polar((0, 0)).0, 0.0);
        assert!(Joystick::to_polar((0, 0)).1.is_nan());
    }
}
