use proc_macro2::{TokenStream, Ident, Span};
use quote::{quote, ToTokens, TokenStreamExt};
use serde::{Serialize, Deserialize};
use schemars::JsonSchema;

use super::impl_enum_to_tokens;

pub type Layers = Vec<Vec<Vec<Action>>>;

pub fn to_tokens(layers: &Layers) -> TokenStream {
    quote! {
        &[ #(&[ #(&[ #(#layers),* ]),* ]),* ]
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
#[serde(tag = "type")]
pub enum Action {
    NoOp,
    Trans,
    KeyCode { keycode: KeyCode },
    MultipleKeyCodes { keycodes: Vec<KeyCode> },
    MultipleActions { actions: Vec<Action> },
    Layer { layer: usize },
    DefaultLayer { layer: usize },
    HoldTap {
        timeout: u16,
        hold: Box<Action>,
        tap: Box<Action>,
        config: HoldTapConfig,
        tap_hold_interval: u16,
    },
    // TODO: custom actions
    // Custom(T),
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum HoldTapConfig {
    Default,
    HoldOnOtherKeyPress,
    PermissiveHold,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum KeyCode {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Kb1,
    Kb2,
    Kb3,
    Kb4,
    Kb5,
    Kb6,
    Kb7,
    Kb8,
    Kb9,
    Kb0,
    Enter,
    Escape,
    BSpace,
    Tab,
    Space,
    Minus,
    Equal,
    LBracket,
    RBracket,
    Bslash,
    NonUsHash,
    SColon,
    Quote,
    Grave,
    Comma,
    Dot,
    Slash,
    CapsLock,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    PScreen,
    ScrollLock,
    Pause,
    Insert,
    Home,
    PgUp,
    Delete,
    End,
    PgDown,
    Right,
    Left,
    Down,
    Up,
    NumLock,
    KpSlash,
    KpAsterisk,
    KpMinus,
    KpPlus,
    KpEnter,
    Kp1,
    Kp2,
    Kp3,
    Kp4,
    Kp5,
    Kp6,
    Kp7,
    Kp8,
    Kp9,
    Kp0,
    KpDot,
    NonUsBslash,
    Application,
    Power,
    KpEqual,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    Execute,
    Help,
    Menu,
    Select,
    Stop,
    Again,
    Undo,
    Cut,
    Copy,
    Paste,
    Find,
    Mute,
    VolUp,
    VolDown,
    LockingCapsLock,
    LockingNumLock,
    LockingScrollLock,
    KpComma,
    KpEqualSign,
    Intl1,
    Intl2,
    Intl3,
    Intl4,
    Intl5,
    Intl6,
    Intl7,
    Intl8,
    Intl9,
    Lang1,
    Lang2,
    Lang3,
    Lang4,
    Lang5,
    Lang6,
    Lang7,
    Lang8,
    Lang9,
    AltErase,
    SysReq,
    Cancel,
    Clear,
    Prior,
    Return,
    Separator,
    Out,
    Oper,
    ClearAgain,
    CrSel,
    ExSel,
    LCtrl,
    LShift,
    LAlt,
    LGui,
    RCtrl,
    RShift,
    RAlt,
    RGui,
    MediaPlayPause,
    MediaStopCD,
    MediaPreviousSong,
    MediaNextSong,
    MediaEjectCD,
    MediaVolUp,
    MediaVolDown,
    MediaMute,
    MediaWWW,
    MediaBack,
    MediaForward,
    MediaStop,
    MediaFind,
    MediaScrollUp,
    MediaScrollDown,
    MediaEdit,
    MediaSleep,
    MediaCoffee,
    MediaRefresh,
    MediaCalc,
}

impl_enum_to_tokens! {
    enum KeyCode: keyberon::key_code::KeyCode,
    enum HoldTapConfig: keyberon::action::HoldTapConfig,
}

impl ToTokens for Action {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let act = quote! { keyberon::action::Action };
        let t = match self {
            Action::NoOp => quote! { #act::NoOp },
            Action::Trans => quote! { #act::Trans },
            Action::KeyCode { keycode } => {
                quote! { #act::KeyCode(#keycode) }
            },
            Action::MultipleKeyCodes { keycodes } => {
                quote! { #act::MultipleKeyCodes( &[ #( #keycodes ),* ] ) }
            },
            Action::MultipleActions { actions } => {
                quote! { #act::MultipleActions( &[ #( #actions ),* ] ) }
            },
            Action::Layer { layer } => quote! { #act::Layer(#layer) },
            Action::DefaultLayer { layer } => quote! { #act::DefaultLayer(#layer) },
            Action::HoldTap { timeout, hold, tap, config, tap_hold_interval } => {
                quote! {
                    #act::HoldTap {
                        timeout: #timeout,
                        hold: &#hold,
                        tap: &#tap,
                        config: #config,
                        tap_hold_interval: #tap_hold_interval,
                    }
                }
            },
        };
        tokens.append_all(t);
    }
}

#[cfg(test)]
pub mod tests {
    use crate::format::assert_tokens_eq;

    use super::*;

    pub fn example_json() -> serde_json::Value {
        serde_json::json!([
            [
                [
                    {
                        "type": "NoOp"
                    },
                    {
                        "type": "Trans"
                    },
                    {
                        "type": "KeyCode",
                        "keycode": "Kb2"
                    },
                    {
                        "type": "MultipleKeyCodes",
                        "keycodes": ["LCtrl", "C"]
                    },
                    {
                        "type": "MultipleActions",
                        "actions": [
                        {
                            "type": "KeyCode",
                            "keycode": "Q"
                        },
                        {
                            "type": "Layer",
                            "layer": 2
                        }
                    ]
                    },
                    {
                        "type": "Layer",
                        "layer": 3
                    },
                    {
                        "type": "DefaultLayer",
                        "layer": 2
                    },
                    {
                        "type": "HoldTap",
                        "timeout": 180,
                        "hold": {
                            "type": "Layer",
                            "layer": 2
                        },
                        "tap": {
                            "type": "KeyCode",
                            "keycode": "Space"
                        },
                        "config": "Default",
                        "tap_hold_interval": 100
                    }
                ]
            ]
        ])
    }

    pub fn example_config() -> Layers {
        vec![
            vec![
                vec![
                    Action::NoOp,
                    Action::Trans,
                    Action::KeyCode { keycode: KeyCode::Kb2 },
                    Action::MultipleKeyCodes {
                        keycodes: vec![
                            KeyCode::LCtrl,
                            KeyCode::C,
                        ],
                    },
                    Action::MultipleActions {
                        actions: vec![
                            Action::KeyCode { keycode: KeyCode::Q },
                            Action::Layer { layer: 2 },
                        ],
                    },
                    Action::Layer { layer: 3 },
                    Action::DefaultLayer { layer: 2 },
                    Action::HoldTap {
                        timeout: 180,
                        hold: Box::new(Action::Layer { layer: 2 }),
                        tap: Box::new(Action::KeyCode { keycode: KeyCode::Space }),
                        config: HoldTapConfig::Default,
                        tap_hold_interval: 100,
                    },
                ],
            ],
        ]
    }

    pub fn example_code() -> TokenStream {
        quote! {
            &[
                &[
                    &[
                        keyberon::action::Action::NoOp,
                        keyberon::action::Action::Trans,
                        keyberon::action::Action::KeyCode(keyberon::key_code::KeyCode::Kb2),
                        keyberon::action::Action::MultipleKeyCodes(&[
                            keyberon::key_code::KeyCode::LCtrl,
                            keyberon::key_code::KeyCode::C,
                        ]),
                        keyberon::action::Action::MultipleActions(&[
                            keyberon::action::Action::KeyCode(keyberon::key_code::KeyCode::Q),
                            keyberon::action::Action::Layer(2usize),
                        ]),
                        keyberon::action::Action::Layer(3usize),
                        keyberon::action::Action::DefaultLayer(2usize),
                        keyberon::action::Action::HoldTap {
                            timeout: 180u16,
                            hold: &keyberon::action::Action::Layer(2usize),
                            tap: &keyberon::action::Action::KeyCode(keyberon::key_code::KeyCode::Space),
                            config: keyberon::action::HoldTapConfig::Default,
                            tap_hold_interval: 100u16,
                        }
                    ]
                ]
             ]
        }
    }

    #[test]
    fn deserialize() -> anyhow::Result<()> {
        let layers: Layers = serde_json::from_value(example_json())?;
        assert_eq!(layers, example_config());
        Ok(())
    }

    #[test]
    fn tokenize() {
        assert_tokens_eq(to_tokens(&example_config()), example_code())
    }
}
