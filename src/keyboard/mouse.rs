use usb_device::class_prelude::UsbBus;
use usbd_hid::{descriptor::MouseReport, hid_class::HIDClass};
use bitfield::bitfield;

use super::actions::{MouseAction, MouseButton, MouseMovement};

/// USB mouse emulation
pub struct Mouse {
    buttons: MouseButtons,
    movement: MovementButtons,
    xy: PlaneAccumulator<'static>,
    scroll: PlaneAccumulator<'static>,
    joystick: Joystick<'static>,
}

/// Speed profiles for mouse emulation
pub struct MouseConfig {
    pub x: AxisConfig,
    pub y: AxisConfig,
    pub wheel: AxisConfig,
    pub pan: AxisConfig,
    pub joystick: JoystickConfig,
}

/// Configuration for single movement axis
pub struct AxisConfig {
    pub invert: bool,
    pub profile: &'static SpeedProfile,
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

/// Joystick configuration
// TODO: movement speed curve
pub struct JoystickConfig {
    /// Minimum reading value at which joystick movement is registered
    pub min: u16,
    /// Maximum value of joystick readings
    pub max: u16,
    /// Divider controlling the joystick speed
    pub divider: u16,
    /// Swap X with Y
    pub swap_axes: bool,
    /// Invert X axis direction
    pub invert_x: bool,
    /// Invert Y axis direction
    pub invert_y: bool,
}

/// Joystick data
struct Joystick<'a> {
    x: i16,
    y: i16,
    x_acc: DivAccumulator,
    y_acc: DivAccumulator,
    // TODO: set from joystick config, change via custom actions
    plane: Plane,
    config: &'a JoystickConfig,
}

// Movement plane
enum Plane {
    Xy,
    Scroll,
}

/// Movement emulation on a 2D plane
struct PlaneAccumulator<'a> {
    x: AxisAccumulator<'a>,
    y: AxisAccumulator<'a>,
    x_config: &'a AxisConfig,
    y_config: &'a AxisConfig,
}

/// Movement emulation along single axis
struct AxisAccumulator<'a> {
    time: u16,
    accumulated: DivAccumulator,
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
    pub const fn new(config: &'static MouseConfig) -> Self {
        Self {
            buttons: MouseButtons(0),
            movement: MovementButtons(0),
            xy: PlaneAccumulator::new(&config.x, &config.y),
            scroll: PlaneAccumulator::new(&config.pan, &config.wheel),
            joystick: Joystick::new(&config.joystick),
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

    /// Advance time and accumulate state
    pub fn tick(&mut self) {
        let m = &self.movement;
        self.xy.tick(m.up(), m.down(), m.left(), m.right());
        self.scroll.tick(m.wheel_up(), m.wheel_down(), m.pan_left(), m.pan_right());
        self.joystick.tick();
    }

    /// Store latest joystick readings
    pub fn update_joystick(&mut self, (x, y): (i16, i16)) {
        self.joystick.set(x, y);
    }

    fn get_speeds(&self) -> (i8, i8, i8, i8) {
        let (mut x, mut y) = self.xy.get();
        let (mut pan, mut wheel) = self.scroll.get();
        if self.joystick.active() {
            let (joy_x, joy_y) = (self.joystick.x_acc.get(), self.joystick.y_acc.get());
            let (px, py) = match self.joystick.plane {
                Plane::Xy => (&mut x, &mut y),
                Plane::Scroll => (&mut pan, &mut wheel),
            };
            *px = px.saturating_add(joy_x);
            *py = py.saturating_add(joy_y);
        }
        (x, y, pan, wheel)
    }

    /// Try to push mouse report to endpoint or keep current info for the next report.
    pub fn push_report<'a, B: UsbBus>(&mut self, hid: &HIDClass<'a, B>) -> bool {
        let (x, y, pan, wheel) = self.get_speeds();
        let report = MouseReport { buttons: self.buttons.0, x, y, wheel, pan };

        match hid.push_input(&report) {
            Ok(_len) => {
                self.xy.consume();
                self.scroll.consume();
                self.joystick.x_acc.consume();
                self.joystick.y_acc.consume();
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

impl<'a> PlaneAccumulator<'a> {
    pub const fn new(x: &'a AxisConfig, y: &'a AxisConfig) -> Self {
        Self {
            x: AxisAccumulator::new(x.profile),
            y: AxisAccumulator::new(y.profile),
            x_config: x,
            y_config: y,
        }
    }

    pub fn tick(&mut self, up: bool, down: bool, left: bool, right: bool) {
        let reset = !(up || down || left || right);
        let dir_x = Self::direction(right, left, self.x_config.invert);
        let dir_y = Self::direction(down, up, self.y_config.invert);
        self.x.tick(reset, dir_x);
        self.y.tick(reset, dir_y);
    }

    pub fn get(&self) -> (i8, i8) {
        let (x, y) = (self.x.accumulated.get(), self.y.accumulated.get());
        // Generate 2D speed value if we are moving in both directions
        if x != 0 && y != 0 {
            (Self::mul_inv_sqrt2(x), Self::mul_inv_sqrt2(x))
        } else {
            (x, y)
        }
    }

    pub fn consume(&mut self) {
        self.x.accumulated.consume();
        self.y.accumulated.consume();
    }

    /// Calculate x * sqrt(2) (181/256=0.70703125 vs 1/sqrt(2)=0.707106781)
    #[inline(always)]
    const fn mul_inv_sqrt2(val: i8) -> i8 {
        ((val as i32 * 181) / 256) as i8
    }

    /// Get direction multiplier depending on state of positive and negative button
    #[inline(always)]
    const fn direction(positive: bool, negative: bool, invert: bool) -> i32 {
        let (positive, negative) = if invert {
            (negative, positive)
        } else {
            (positive, negative)
        };
        match (positive, negative) {
            (true, true) => 0,
            (true, false) => 1,
            (false, true) => -1,
            (false, false) => 0,
        }
    }
}

/// Accumulate values to read at lower resolution depending on divider.
struct DivAccumulator {
    value: i32,
    divider: u16,
}

impl DivAccumulator {
    pub const fn new(divider: u16) -> Self {
        Self { value: 0, divider }
    }

    pub fn accumulate(&mut self, value: i32) {
        self.value = self.value.saturating_add(value);
    }

    pub fn get(&self) -> i8 {
        (self.value / self.div())
            .clamp(i8::MIN as i32, i8::MAX as i32) as i8
    }

    pub fn consume(&mut self) {
        let rounded = self.get() as i32 * self.div();
        // Avoid loosing small accumulated values by only subtracting the consumed value
        if rounded.abs() > self.value.abs() {
            self.value = 0;
        } else {
            self.value -= rounded;
        }
    }

    fn div(&self) -> i32 {
        // Avoid division by 0, while also avoiding (div + 1)
        self.divider.max(1) as i32
    }
}


impl<'a> AxisAccumulator<'a> {
    pub const fn new(profile: &'a SpeedProfile) -> Self {
        Self { profile, time: 0, accumulated: DivAccumulator::new(profile.divider) }
    }

    pub fn tick(&mut self, reset: bool, dir: i32) {
        if reset {
            self.time = 0;
        }

        // Accumulate
        let speed = dir * self.profile.get_speed(self.time) as i32;
        self.accumulated.accumulate(speed);

        self.time = self.time.saturating_add(1);
    }
}

impl<'a> Joystick<'a> {
    pub const fn new(config: &'a JoystickConfig) -> Self {
        Self {
            x: 0,
            y: 0,
            x_acc: DivAccumulator::new(config.divider),
            y_acc: DivAccumulator::new(config.divider),
            plane: Plane::Xy,
            config
        }
    }

    pub fn active(&self) -> bool {
        self.x.abs() as u16 >= self.config.min || self.y.abs() as u16 >= self.config.min
    }

    pub fn set(&mut self, x: i16, y: i16) {
        let x = if self.config.invert_x { -x } else { x };
        let y = if self.config.invert_y { -y } else { y };
        let (x, y) = if self.config.swap_axes {
            (y, x)
        } else {
            (x, y)
        };
        self.x = x;
        self.y = y;
    }

    pub fn tick(&mut self) {
        if !self.active() {
            return
        }
        let clamped = |val: i16| {
            (val.signum() * ((val.abs() as u16).min(self.config.max)) as i16) as i32
        };
        self.x_acc.accumulate(clamped(self.x));
        self.y_acc.accumulate(clamped(self.y));
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
        assert_eq!(acc.accumulated.get(), 0);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 10);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 10 + 20);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 10 + 20 + 30);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 10 + 20 + 30 + 30);
        acc.accumulated.consume();
        assert_eq!(acc.accumulated.get(), 0);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 30);
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
        assert_eq!(acc.accumulated.get(), 0);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 10);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 10 + 20);
        acc.tick(false, -1);
        assert_eq!(acc.accumulated.get(), 10 + 20 - 30);
        acc.tick(false, -1);
        assert_eq!(acc.accumulated.get(), 10 + 20 - 30 - 30);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 10 + 20 - 30 - 30 + 30);
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
        assert_eq!(acc.accumulated.get(), 0);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 0);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 10);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 10 + 20);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 10 + 20 + 30);
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
        assert_eq!(acc.accumulated.get(), 10 + 20 + 30 + 30 + 30);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), 127);
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
        assert_eq!(acc.accumulated.get(), 10 / 2);
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
        assert_eq!(acc.accumulated.get(), 100);
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
        assert_eq!(acc.accumulated.get(), ((50_i32 + 75 + 100) / 10) as i8);
        acc.tick(true, 1);
        assert_eq!(acc.accumulated.get(), ((50_i32 + 75 + 100 + 50) / 10) as i8);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), ((50_i32 + 75 + 100 + 50 + 75) / 10) as i8);
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
        assert_eq!(acc.accumulated.get(), ((50_i32 + 75 + 100) / 10) as i8);
        acc.tick(false, 0);
        assert_eq!(acc.accumulated.get(), ((50_i32 + 75 + 100) / 10) as i8);
        acc.tick(false, 1);
        assert_eq!(acc.accumulated.get(), ((50_i32 + 75 + 100 + 100) / 10) as i8);
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
            assert_eq!(acc.accumulated.get(), val, "At i = {}", i);
            acc.accumulated.consume();
        }
    }
}
