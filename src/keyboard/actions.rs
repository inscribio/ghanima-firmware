#![allow(dead_code)]

use crate::utils::CircularIter;

/// Additional key actions
pub enum Action {
    /// Modify LED lightning
    Led(LedAction),
    /// Use mouse emulation
    Mouse(MouseAction),
}


/// Actions for LED lightning control
pub enum LedAction {
    /// Cycle through available LED configurations
    Cycle(Inc),
    /// Modify global brightness
    Brightness(Inc),
}


/// Actions related to mouse emulation
pub enum MouseAction {
    /// Key emulates a mouse key
    Click(MouseButton),
    /// Key performs mouse movement when held
    Move(MouseMovement),
    /// Key changes mouse sensitivity
    Sensitivity(Inc),
}

/// Emulate a mouse button
pub enum MouseButton {
    Left,
    Mid,
    Right,
}

/// Emulate mouse (or mouse wheel) movement
pub enum MouseMovement {
    Up,
    Down,
    Left,
    Right,
    WheelUp,
    WheelDown,
    PanLeft,
    PanRight,
}

/// Changing value of a variable with integer steps
pub enum Inc {
    /// Up/Increase/Next/Increment
    Up,
    /// Down/Decrease/Previous/Decrement
    Down,
}

impl Inc {
    pub fn update<'a, T>(&self, iter: &mut CircularIter<'a, T>) -> &'a T {
        match self {
            Inc::Up => iter.next().unwrap(),
            Inc::Down => iter.next_back().unwrap(),
        }
    }
}
