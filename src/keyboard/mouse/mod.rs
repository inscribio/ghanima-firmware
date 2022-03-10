use usbd_hid::descriptor::MouseReport;
use bitfield::bitfield;

use super::actions::{MouseAction, MouseButton, MouseMovement};

pub struct Mouse {
    buttons: MouseButtons,
    movement: MovementButtons,
    x: AxisEmulator<'static>,
    y: AxisEmulator<'static>,
    wheel: AxisEmulator<'static>,
    pan: AxisEmulator<'static>,
    buttons_changed: bool,
}

/// Speed profiles for mouse emulation
pub struct MouseSpeedProfiles<'a> {
    pub x: &'a SpeedProfile,
    pub y: &'a SpeedProfile,
    pub wheel: &'a SpeedProfile,
    pub pan: &'a SpeedProfile,
}

/// Mouse movement emulation speed profile
///
/// Mouse speed emulation based on `MouseKeysAccel` from
/// https://en.wikipedia.org/wiki/Mouse_keys.
pub struct SpeedProfile {
    /// Interval at which to generate motion events
    // TODO: remove it and work in abstract "ticks"
    pub interval: u32,
    /// Minimum key press time required to start motion
    pub min_delay: u32,
    /// Time (since `min_delay`) after which maximum speed is reached
    pub time_to_max: u32,
    /// Speed value that is initially applied after `min_delay`
    pub start_speed: u32,
    /// Speed reached after `min_delay + time_to_max`
    pub max_speed: u32,
    /// Acceleration curve control as defined for `MouseKeysAccel` profile
    pub curve_1000: Option<i32>,
}


/// Movement emulator along single axis
struct AxisEmulator<'a> {
    last: u32,
    accumulated: u32,
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
    pub fn new(profiles: &'static MouseSpeedProfiles<'static>) -> Self {
        Self {
            buttons: MouseButtons(0),
            movement: MovementButtons(0),
            x: AxisEmulator::new(profiles.x),
            y: AxisEmulator::new(profiles.y),
            wheel: AxisEmulator::new(profiles.wheel),
            pan: AxisEmulator::new(profiles.pan),
            buttons_changed: true,  // to initially send a report
        }
    }

    /// Handle mouse action key event
    pub fn handle_action(&mut self, action: MouseAction, pressed: bool) {
        match action {
            MouseAction::Click(button) => {
                let prev = self.buttons;
                match button {
                    MouseButton::Left => self.buttons.set_left(pressed),
                    MouseButton::Mid => self.buttons.set_mid(pressed),
                    MouseButton::Right => self.buttons.set_right(pressed),
                };
                if self.buttons != prev {
                    self.buttons_changed = true;
                }
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

    /// Generate next mouse report if one should be sent
    pub fn tick(&mut self, time_ms: u32) -> Option<MouseReport> {
        let m = &self.movement;
        let (reset_xy, dir_x, dir_y) = Self::movement_state(m.up(), m.down(), m.left(), m.right());
        let (reset_scroll, dir_pan, dir_wheel) = Self::movement_state(
            m.wheel_up(), m.wheel_down(), m.pan_left(), m.pan_right());

        let as_i8 = |speed: Option<i32>| {
            let speed = speed.unwrap_or(0);
            speed.clamp(i8::MIN as i32, i8::MAX as i32) as i8
        };

        let x = as_i8(self.x.get_speed(time_ms, dir_x, reset_xy));
        let y = as_i8(self.y.get_speed(time_ms, dir_y, reset_xy));
        let wheel = as_i8(self.wheel.get_speed(time_ms, dir_wheel, reset_scroll));
        let pan = as_i8(self.pan.get_speed(time_ms, dir_pan, reset_scroll));

        let (x, y) = Self::speed_2d(x, y);
        let (pan, wheel) = Self::speed_2d(pan, wheel);

        let any_movement = (x | y | wheel | pan) != 0;
        if self.buttons_changed || any_movement {
            self.buttons_changed = false;
            Some(MouseReport { buttons: self.buttons.0, x, y, wheel, pan })
        } else {
            None
        }
    }
}

impl SpeedProfile {
    pub fn get_speed(&self, time: u32) -> u32 {
        if time < self.min_delay {
            0
        } else if time < self.min_delay + self.time_to_max {
            let speed = self.start_speed as i32
                + ((self.max_speed - self.start_speed) * (time - self.min_delay)) as i32
                * (1000 + self.curve_1000.unwrap_or(0).clamp(-1000, 1000))
                / self.time_to_max as i32
                / 1000 as i32;
            speed as u32
        } else {
            self.max_speed
        }
    }
}

impl<'a> AxisEmulator<'a> {
    pub const fn new(profile: &'a SpeedProfile) -> Self {
        Self { last: 0, accumulated: 0, profile }
    }

    pub fn get_speed(&mut self, time: u32, dir: i32, reset: bool) -> Option<i32> {
        if time < self.last + self.profile.interval {
            return None;
        }

        self.accumulated += time - self.last;
        self.last = time;

        let speed = dir * self.profile.get_speed(self.accumulated) as i32;

        if reset {
            self.accumulated = 0;
        }

        Some(speed)
    }
}
