// What QMK does: https://github.com/qmk/qmk_firmware/blob/master/docs/feature_rgblight.md
// * keys for H/S/V inc/dec
// * cycle through lightning modes: static, breathing, rainbow, swirl, snake, "knight rider",
//   christmas, static gradient, rgb test, twinkle
//
// We probably want:
// * global mode - something that affects lightning of all leds with spacial knowledge
// * per-led modes - way to overwrite global mode for one or more leds
// * colors/animations could be somehow separate? or just use
//
// Usage:
// * signalize Caps Lock and others using a single LED or globally -
//   e.g. Caps Lock changes color/animation of whole keyboard, num lock on top via single key
// * change color/animation
//
// Format in-code:
// list of lightning definitions -> list of per-layer settings with 1st element as a default ->
// -> list of rules -> rule: key-matcher (all, rows, cols, specific keys) and color-spec
//
// Color-spec can be:
// * static
// * animation
// * dynamic from source, e.g. value of keyboard LEDs (caps/num lock, etc.), value of encoder,
//   current layer number
//
// Separate concepts:
// * in-memory representation of led configurations
// * CustomAction variants for led control
// * web UI for configuring led colors
// * file format for translating UI config to in-memory config
// * lightning controller - something that scans the rules and decides on final lightning for
//   each led
// * lightning executor - something that takes current animation and generates current color
//   and if the animation was to run "once" stops on a static color
// Notes:
// * web UI should most likely have a format that stores all key information in one place (like
//   a big key config); but we could allow to define some things (like color transition patterns,
//   key matchers, etc.) separately so that we could make use of the fact that many variants use
//   `&'static` and we could avoid duplication (smaller code size)
// * in-memory has to split this because we have Layout for key functions and need some led color
//   config

#![allow(dead_code)]

/// Logic related to rule conditions
mod condition;
/// Pattern iteration and color generation logic
mod pattern;

pub use pattern::{PatternController, Leds};
pub use condition::{KeyboardState, KeyboardLedsState};

use rgb::RGB8;
use super::role::Role;

/// List of keyboard LED lightning configurations
///
/// Configurations that can be cycled through, but only one is active at a time.
pub type LedConfigurations = &'static [LedConfig];

/// Configuration of keyboard LED lightning
#[derive(Debug)]
pub struct LedConfig {
    pub default: LayerRules,
    pub layers: &'static [LayerRules],
}

/// List of LED pattern rules for a single layer
pub type LayerRules = &'static [LedRule];

/// Rule defining LED pattern for given keys if condition applies
#[derive(Debug)]
pub struct LedRule {
    /// Keys to which the rule applies
    pub keys: Keys,
    /// Condition required for the rule to be active
    pub condition: Condition,
    /// Color pattern used for a LED when the rule applies
    pub pattern: Pattern,
}

/// Defines which keys to match (rows/cols must be valid)
///
/// Note that joystick is not considered as a key, because it has no LED
/// associated.
#[derive(Debug)]
pub enum Keys {
    /// All keys on this layer
    All,
    /// All keys from given rows
    Rows(&'static [u8]),
    /// All keys from given columns
    Cols(&'static [u8]),
    /// Specific keys
    // FIXME: should work on global coordinates instead of side-local
    Keys(&'static [(u8, u8)]),
}

/// Condition for the rule to be used
#[derive(Debug, PartialEq)]
pub enum Condition {
    /// Always applies
    Always,
    /// Apply this rule if host PC specifies that given LED is on
    Led(KeyboardLed),
    /// Apply if USB is (dis-)connected
    UsbOn(bool),
    /// Apply if the keyboard half acts as a master
    Role(Role),
    /// Apply when this key has given pressed state
    Pressed(bool),
    /// Apply when given key has given pressed state
    KeyPressed(bool, (u8, u8)),
}

/// Standard keyboard LED
#[derive(Debug, PartialEq)]
pub enum KeyboardLed {
    NumLock,
    CapsLock,
    ScrollLock,
    Compose,
    Kana,
}

/// Defines lightning pattern
#[derive(Debug)]
pub struct Pattern {
    pub repeat: Repeat,
    pub transitions: &'static [Transition],
    pub phase: Phase,
}

/// Pattern phase shift depending on key position
// TODO: rethink
#[derive(Debug, PartialEq)]
pub struct Phase {
    pub x: f32,
    pub y: f32,
}

/// Defines how the pattern should be repeated
#[derive(Debug)]
pub enum Repeat {
    /// Run the pattern once, then stop
    Once,
    /// Runs start->end, start->end, ...
    Wrap,
    /// Runs start->end then back end->start
    Reflect,
}

/// Single color transition in a pattern
#[derive(Debug, PartialEq)]
pub struct Transition {
    /// "Destination" color
    pub color: RGB8,
    /// Duration in milliseconds (max duration ~65.5 seconds)
    ///
    /// Duration 0 means that this transition will never end, so it can be
    /// used to specify constant color.
    pub duration: u16,
    /// Color interpolation type
    pub interpolation: Interpolation,
}

/// Color interpolation behavior
#[derive(Debug, PartialEq)]
pub enum Interpolation {
    /// Instantly change from previous color to this one
    Piecewise,
    /// Interpolate between previous color and this one
    Linear,
}
