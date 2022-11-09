pub use crate::keyboard::hid::ConsumerKey;
pub use crate::utils::Inc;

/// Additional key actions
pub enum Action {
    /// Modify LED lightning
    Led(LedAction),
    /// Use mouse emulation
    Mouse(MouseAction),
    /// Send USB HID consumer page keys
    Consumer(ConsumerKey),
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
