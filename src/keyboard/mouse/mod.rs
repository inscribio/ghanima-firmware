use usb_device::class_prelude::UsbBus;
use usbd_hid::{descriptor::MouseReport, hid_class::HIDClass};
use bitfield::bitfield;

use super::actions::{MouseAction, MouseButton, MouseMovement};

/// USB mouse emulation
pub struct Mouse {
    buttons: MouseButtons,
    movement: MovementButtons,
    x: AxisAccumulator<'static>,
    y: AxisAccumulator<'static>,
    // TODO: way to invert scrolling
    wheel: AxisAccumulator<'static>,
    pan: AxisAccumulator<'static>,
}

/// Speed profiles for mouse emulation
pub struct MouseSpeedProfiles {
    pub x: &'static SpeedProfile,
    pub y: &'static SpeedProfile,
    pub wheel: &'static SpeedProfile,
    pub pan: &'static SpeedProfile,
}

/// Constant acceleration mouse speed profile.
///
/// HID mouse uses i8 [-128, 127] displacement in single USB report.
/// To keep better resolution all values are u16 and `divider` is
/// used to scale down the resulting speed.
pub struct SpeedProfile {
    /// Controls output speed scaling
    pub divider: u16,
    /// Delay from the moment key is pressed to when start_speed is applied
    pub delay: u16,
    /// Time it takes to accelerate from `start_speed` to `max_speed`
    pub acceleration_time: u16,
    /// Initial speed value applied after `delay`
    pub start_speed: u16,
    /// Final speed reached after `delay + acceleration_time` since key press
    pub max_speed: u16,
}


// struct PlaneEmulaator<'a> {
//     x: AxisEmulator<'a>,
//     y: AxisEmulator<'a>,
// }

/// Movement emulator along single axis
struct AxisAccumulator<'a> {
    time: u16,
    accumulated: i32,
    profile: &'a SpeedProfile,
}

bitfield! {
    /// State of mouse buttons
    #[derive(Clone, Copy, PartialEq)]
    struct MouseButtons(u8);
    pub left, set_left: 0;
    pub right, set_right: 1;
    pub mid, set_mid: 2;
}

bitfield! {
    /// State of mouse direction button on the keyboard
    #[derive(Clone, Copy)]
    struct MovementButtons(u8);
    pub up, set_up: 0;
    pub down, set_down: 1;
    pub left, set_left: 2;
    pub right, set_right: 3;
    pub wheel_up, set_wheel_up: 4;
    pub wheel_down, set_wheel_down: 5;
    pub pan_left, set_pan_left: 6;
    pub pan_right, set_pan_right: 7;
}

impl Mouse {
    /// Instantiate with given speed profiles
    pub fn new(profiles: &'static MouseSpeedProfiles) -> Self {
        Self {
            buttons: MouseButtons(0),
            movement: MovementButtons(0),
            x: AxisAccumulator::new(profiles.x),
            y: AxisAccumulator::new(profiles.y),
            wheel: AxisAccumulator::new(profiles.wheel),
            pan: AxisAccumulator::new(profiles.pan),
        }
    }

    /// Handle mouse action key event
    pub fn handle_action(&mut self, action: &MouseAction, pressed: bool) {
        match action {
            MouseAction::Click(button) => {
                match button {
                    MouseButton::Left => self.buttons.set_left(pressed),
                    MouseButton::Mid => self.buttons.set_mid(pressed),
                    MouseButton::Right => self.buttons.set_right(pressed),
                };
            },
            MouseAction::Move(movement) => match movement {
                MouseMovement::Up => self.movement.set_up(pressed),
                MouseMovement::Down => self.movement.set_down(pressed),
                MouseMovement::Left => self.movement.set_left(pressed),
                MouseMovement::Right => self.movement.set_right(pressed),
                MouseMovement::WheelUp => self.movement.set_wheel_up(pressed),
                MouseMovement::WheelDown => self.movement.set_wheel_down(pressed),
                MouseMovement::PanLeft => self.movement.set_pan_left(pressed),
                MouseMovement::PanRight => self.movement.set_pan_right(pressed),
            },
            MouseAction::Sensitivity(_) => todo!(),
        }
    }

    /// Get direction multiplier depending on state of positive and negative button
    #[inline(always)]
    const fn direction(positive: bool, negative: bool) -> i32 {
        match (positive, negative) {
            (true, true) => 0,
            (true, false) => 1,
            (false, true) => -1,
            (false, false) => 0,
        }
    }

    /// Get state of 2D movement (reset_xy, dir_x, dir_y)
    #[inline(always)]
    const fn movement_state(up: bool, down: bool, left: bool, right: bool) -> (bool, i32, i32) {
        let reset_xy = !(up || down || left || right);
        let dir_x = Self::direction(right, left);
        let dir_y = Self::direction(down, up);
        (reset_xy, dir_x, dir_y)
    }

    /// Calculate x * sqrt(2) (181/256=0.70703125 vs 1/sqrt(2)=0.707106781)
    #[inline(always)]
    const fn mul_inv_sqrt2(val: i8) -> i8 {
        ((val as i32 * 181) / 256) as i8
    }

    /// Generate 2D speed value if we are moving in both directions
    #[inline(always)]
    const fn speed_2d(x: i8, y: i8) -> (i8, i8) {
        if x != 0 && y != 0 {
            (Self::mul_inv_sqrt2(x), Self::mul_inv_sqrt2(x))
        } else {
            (x, y)
        }
    }

    /// Advance time and accumulate state
    pub fn tick(&mut self) {
        let m = &self.movement;
        let (reset_xy, dir_x, dir_y) = Self::movement_state(m.up(), m.down(), m.left(), m.right());
        let (reset_scroll, dir_pan, dir_wheel) = Self::movement_state(
            m.wheel_up(), m.wheel_down(), m.pan_left(), m.pan_right());

        self.x.tick(reset_xy, dir_x);
        self.y.tick(reset_xy, dir_y);
        self.wheel.tick(reset_scroll, dir_wheel);
        self.pan.tick(reset_scroll, dir_pan);
    }

    /// Try to push mouse report to endpoint or keep current info for the next report.
    pub fn push_report<'a, B: UsbBus>(&mut self, hid: &HIDClass<'a, B>) -> bool {
        let x = self.x.accumulated();
        let y = self.y.accumulated();
        let wheel = self.wheel.accumulated();
        let pan = self.pan.accumulated();

        let (x, y) = Self::speed_2d(x, y);
        let (pan, wheel) = Self::speed_2d(pan, wheel);

        let report = MouseReport { buttons: self.buttons.0, x, y, wheel, pan };

        match hid.push_input(&report) {
            Ok(_len) => {
                self.x.consume();
                self.y.consume();
                self.wheel.consume();
                self.pan.consume();
                true
            }
            Err(e) => match e {
                usb_device::UsbError::WouldBlock => false,
                _ => Err(e).unwrap(),
            },
        }
    }
}

impl SpeedProfile {
    pub fn get_speed(&self, time: u16) -> u16 {
        if time < self.delay {
            0
        } else if (self.acceleration_time != 0) && (time < self.delay + self.acceleration_time) {
            let v0 = self.start_speed;
            let v1 = self.max_speed;
            let t0 = self.delay;
            let dt = self.acceleration_time;
            let speed = v0 as u32 + (v1 - v0) as u32 * (time - t0) as u32 / dt as u32;
            speed as u16
        } else {
            self.max_speed
        }
    }
}

impl<'a> AxisAccumulator<'a> {
    pub const fn new(profile: &'a SpeedProfile) -> Self {
        Self { profile, time: 0, accumulated: 0 }
    }

    fn div(&self) -> i32 {
        // Avoid division by 0, while also avoiding (div + 1)
        self.profile.divider.max(1) as i32
    }

    pub fn consume(&mut self) {
        let rounded = self.accumulated() as i32 * self.div();
        // Avoid loosing small accumulated values by only subtracting the consumed value
        if rounded.abs() > self.accumulated.abs() {
            #[cfg(test)]
            {
                use std::println;
                println!("Consuming to {} -> 0", self.accumulated);
            }
            self.accumulated = 0;
        } else {
            let a = self.accumulated;
            self.accumulated -= rounded;
            #[cfg(test)]
            {
                use std::println;
                println!("Consuming {} - {} = {}", a, rounded, self.accumulated);
            }
        }
    }

    pub fn accumulated(&self) -> i8 {
        (self.accumulated / self.div())
            .clamp(i8::MIN as i32, i8::MAX as i32) as i8
    }

    pub fn tick(&mut self, reset: bool, dir: i32) {
        if reset {
            self.time = 0;
        }

        // Accumulate
        let speed = dir * self.profile.get_speed(self.time) as i32;
        self.accumulated = self.accumulated.saturating_add(speed);

        self.time = self.time.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulator_basic() {
        let profile = SpeedProfile {
            divider: 1,
            delay: 0,
            acceleration_time: 2,
            start_speed: 10,
            max_speed: 30,
        };
        let mut acc = AxisAccumulator::new(&profile);
        assert_eq!(acc.accumulated(), 0);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 10);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 10 + 20);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 10 + 20 + 30);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 10 + 20 + 30 + 30);
        acc.consume();
        assert_eq!(acc.accumulated(), 0);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 30);
    }

    #[test]
    fn accumulator_backwards() {
        let profile = SpeedProfile {
            divider: 1,
            delay: 0,
            acceleration_time: 2,
            start_speed: 10,
            max_speed: 30,
        };
        let mut acc = AxisAccumulator::new(&profile);
        assert_eq!(acc.accumulated(), 0);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 10);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 10 + 20);
        acc.tick(false, -1);
        assert_eq!(acc.accumulated(), 10 + 20 - 30);
        acc.tick(false, -1);
        assert_eq!(acc.accumulated(), 10 + 20 - 30 - 30);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 10 + 20 - 30 - 30 + 30);
    }

    #[test]
    fn accumulator_minimum_delay() {
        let profile = SpeedProfile {
            divider: 1,
            delay: 2,
            acceleration_time: 2,
            start_speed: 10,
            max_speed: 30,
        };
        let mut acc = AxisAccumulator::new(&profile);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 0);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 0);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 10);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 10 + 20);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 10 + 20 + 30);
    }

    #[test]
    fn accumulator_saturates_at_i8_bounds() {
        let profile = SpeedProfile {
            divider: 1,
            delay: 0,
            acceleration_time: 2,
            start_speed: 10,
            max_speed: 30,
        };
        let mut acc = AxisAccumulator::new(&profile);
        for _ in 0..5 {
            acc.tick(false, 1);
        }
        assert_eq!(acc.accumulated(), 10 + 20 + 30 + 30 + 30);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 127);
    }

    #[test]
    fn accumulator_divider() {
        let profile = SpeedProfile {
            divider: 100,
            delay: 0,
            acceleration_time: 0,
            start_speed: 50,
            max_speed: 50,
        };
        let mut acc = AxisAccumulator::new(&profile);
        for _ in 0..10 {
            acc.tick(false, 1);
        }
        assert_eq!(acc.accumulated(), 10 / 2);
    }

    #[test]
    fn accumulator_divider_0_handled() {
        let profile = SpeedProfile {
            divider: 0,
            delay: 0,
            acceleration_time: 0,
            start_speed: 50,
            max_speed: 50,
        };
        let mut acc = AxisAccumulator::new(&profile);
        acc.tick(false, 1);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), 100);
    }

    #[test]
    fn accumulator_time_reset() {
        let profile = SpeedProfile {
            divider: 10,
            delay: 0,
            acceleration_time: 2,
            start_speed: 50,
            max_speed: 100,
        };
        let mut acc = AxisAccumulator::new(&profile);
        acc.tick(false, 1);
        acc.tick(false, 1);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), ((50_i32 + 75 + 100) / 10) as i8);
        acc.tick(true, 1);
        assert_eq!(acc.accumulated(), ((50_i32 + 75 + 100 + 50) / 10) as i8);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), ((50_i32 + 75 + 100 + 50 + 75) / 10) as i8);
    }

    #[test]
    fn accumulator_time_no_reset_on_dir_0() {
        let profile = SpeedProfile {
            divider: 10,
            delay: 0,
            acceleration_time: 2,
            start_speed: 50,
            max_speed: 100,
        };
        let mut acc = AxisAccumulator::new(&profile);
        acc.tick(false, 1);
        acc.tick(false, 1);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), ((50_i32 + 75 + 100) / 10) as i8);
        acc.tick(false, 0);
        assert_eq!(acc.accumulated(), ((50_i32 + 75 + 100) / 10) as i8);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated(), ((50_i32 + 75 + 100 + 100) / 10) as i8);
    }

    #[test]
    fn accumulator_divider_larger_than_speed() {
        let profile = SpeedProfile {
            divider: 100,
            delay: 0,
            acceleration_time: 0,
            start_speed: 0,
            max_speed: 30,
        };
        let seq = [
            0,  // 30
            0,  // 60
            0,  // 90
            1,  // 120 / 100 = 1 rem 20
            0,  // 50
            0,  // 80
            1,  // 110 / 100 = 1 rem 10
            0,  // 40
            0,  // 70
            1,  // 100 / 100 = 1 rem 0
            // then it repeats
            0, 0, 0, 1, 0, 0, 1, 0, 0, 1,
        ];
        let mut acc = AxisAccumulator::new(&profile);
        for (i, val) in seq.into_iter().enumerate() {
            acc.tick(false, 1);
            assert_eq!(acc.accumulated(), val, "At i = {}", i);
            acc.consume();
        }
    }
}
