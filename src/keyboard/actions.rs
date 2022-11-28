pub use usbd_human_interface_device::page::Consumer as ConsumerKey;
pub use crate::utils::Inc;

/// Additional key actions
pub enum Action {
    /// Modify LED lightning
    Led(LedAction),
    /// Use mouse emulation
    Mouse(MouseAction),
    /// Send USB HID consumer page keys
    Consumer(ConsumerKey),
    /// Perform special firmware-related actions
    Firmware(FirmwareAction)
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

/// Special actions related to keyboard firmware
pub enum FirmwareAction {
    /// Allow host to request "jump to bootloader" to flash firmware
    ///
    /// If `bootload_strict` is set in config, then keyboard will refuse
    /// any DFU detach requests until this action is invoked. This is a
    /// security mechanism as some program could in theory try to flash new
    /// firmware without user's knowledge.
    AllowBootloader,
    /// Jump to bootloader manually
    // FIXME: currently device fails to enumerate after reset
    JumpToBootloader,
    /// Reset processor without jumping to bootloader
    Reboot,
}
