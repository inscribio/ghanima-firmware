use proc_macro2::{TokenStream, Ident, Span, Punct, Spacing};
use quote::{quote, ToTokens, TokenStreamExt};
use serde::{Serialize, Deserialize};
use schemars::JsonSchema;

use super::impl_enum_to_tokens;

pub type Layers<T> = Vec<Vec<Vec<Act<T>>>>;

pub fn to_tokens<T: ToTokens>(layers: &Layers<T>) -> TokenStream {
    quote! {
        [ #([ #([ #(#layers),* ]),* ]),* ]
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
// HACK: rename to desired name and use a different name for the enum, or else schemars will use
// the default algorithm for generics leading to Action<Action> becoming "Action_for_Action";
// only rename will not work due to: `let schema_is_renamed = *type_name != schema_base_name`
#[schemars(rename = "Action")]
pub enum Act<T: ToTokens> {
    NoOp,
    Trans,
    KeyCode(KeyCode),
    MultipleKeyCodes(Vec<KeyCode>),
    MultipleActions(Vec<Act<T>>),
    Layer(usize),
    DefaultLayer(usize),
    HoldTap {
        timeout: u16,
        hold: Box<Act<T>>,
        tap: Box<Act<T>>,
        config: HoldTapConfig,
        tap_hold_interval: u16,
    },
    Custom(T),
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum HoldTapConfig {
    Default,
    HoldOnOtherKeyPress,
    PermissiveHold,
    // This should take a function pointer as parameter, but it makes no sense for representing
    // these in JSON. Instead it should be possible to pass a function name and assume that it
    // is already defined somewhere in the firmware.
    Custom(String),
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
}

impl<T: ToTokens> ToTokens for Act<T> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let act = quote! { keyberon::action::Action };
        let t = match self {
            Act::NoOp => quote! { #act::NoOp },
            Act::Trans => quote! { #act::Trans },
            Act::KeyCode(keycode) => {
                quote! { #act::KeyCode(#keycode) }
            },
            Act::MultipleKeyCodes(keycodes) => {
                quote! { #act::MultipleKeyCodes( &[ #( #keycodes ),* ].as_slice() ) }
            },
            Act::MultipleActions(actions) => {
                quote! { #act::MultipleActions( &[ #( #actions ),* ].as_slice() ) }
            },
            Act::Layer(layer) => quote! { #act::Layer(#layer) },
            Act::DefaultLayer(layer) => quote! { #act::DefaultLayer(#layer) },
            Act::HoldTap { timeout, hold, tap, config, tap_hold_interval } => {
                quote! {
                    #act::HoldTap(&keyberon::action::HoldTapAction {
                        timeout: #timeout,
                        hold: #hold,
                        tap: #tap,
                        config: #config,
                        tap_hold_interval: #tap_hold_interval,
                    })
                }
            },
            Act::Custom(custom) => quote! { #act::Custom(#custom) }
        };
        tokens.append_all(t);
    }
}

impl ToTokens for HoldTapConfig {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let c = quote! { keyberon::action::HoldTapConfig };
        println!("tokens: {:#?}", quote! { #c::Default });
        let t = match self {
            HoldTapConfig::Default => quote! { #c::Default },
            HoldTapConfig::HoldOnOtherKeyPress => quote! { #c::HoldOnOtherKeyPress },
            HoldTapConfig::PermissiveHold => quote! { #c::PermissiveHold },
            HoldTapConfig::Custom(s) => {
                let mut f = TokenStream::new();
                s.split("::")
                    .map(|i| Ident::new(i, Span::call_site()))
                    .enumerate()
                    .for_each(|(i, ident)| {
                        if i != 0 {
                            // Add ::
                            f.append(Punct::new(':', Spacing::Joint));
                            f.append(Punct::new(':', Spacing::Alone));
                        }
                        f.append_all(quote! { #ident });
                    });
                quote! { #c::Custom(#f) }
            },
        };
        tokens.append_all(t);
    }
}

#[cfg(test)]
pub mod tests {
    use crate::format::assert_tokens_eq;
    use crate::custom;

    use super::*;

    #[test]
    fn hold_tap_config_custom_fn() {
        let custom = HoldTapConfig::Custom("crate::module::function".to_string());
        assert_tokens_eq(quote! { #custom }, quote! {
            keyberon::action::HoldTapConfig::Custom(crate::module::function)
        });
    }

    #[test]
    fn hold_tap_config_custom_no_colons() {
        let custom = HoldTapConfig::Custom("myfunction".to_string());
        assert_tokens_eq(quote! { #custom }, quote! {
            keyberon::action::HoldTapConfig::Custom(myfunction)
        });
    }

    pub fn example_json() -> serde_json::Value {
        serde_json::json!([
            [
                [
                    "NoOp",
                    "Trans",
                    { "KeyCode": "Kb2" },
                    { "MultipleKeyCodes": ["LCtrl", "C"] },
                    {
                        "MultipleActions": [
                            { "KeyCode": "Q" },
                            { "Layer": 2 }
                        ]
                    },
                    { "Layer": 3 },
                    { "DefaultLayer": 2 },
                    {
                        "HoldTap": {
                            "timeout": 180,
                            "hold": { "Layer": 2 },
                            "tap": { "KeyCode": "Space" },
                            "config": "Default",
                            "tap_hold_interval": 100
                        }
                    },
                    { "Custom": { "Mouse": { "Move": "WheelDown" } } }
                ]
            ]
        ])
    }

    pub fn example_config() -> Layers<custom::Action> {
        vec![
            vec![
                vec![
                    Act::NoOp,
                    Act::Trans,
                    Act::KeyCode(KeyCode::Kb2),
                    Act::MultipleKeyCodes(
                        vec![
                            KeyCode::LCtrl,
                            KeyCode::C,
                        ],
                    ),
                    Act::MultipleActions(
                        vec![
                            Act::KeyCode(KeyCode::Q),
                            Act::Layer(2),
                        ],
                    ),
                    Act::Layer(3),
                    Act::DefaultLayer(2),
                    Act::HoldTap {
                        timeout: 180,
                        hold: Box::new(Act::Layer(2)),
                        tap: Box::new(Act::KeyCode(KeyCode::Space)),
                        config: HoldTapConfig::Default,
                        tap_hold_interval: 100,
                    },
                    Act::Custom(custom::Action::Mouse(custom::MouseAction::Move(custom::MouseMovement::WheelDown))),
                ],
            ],
        ]
    }

    pub fn example_code() -> TokenStream {
        quote! {
            [
                [
                    [
                        keyberon::action::Action::NoOp,
                        keyberon::action::Action::Trans,
                        keyberon::action::Action::KeyCode(keyberon::key_code::KeyCode::Kb2),
                        keyberon::action::Action::MultipleKeyCodes(&[
                            keyberon::key_code::KeyCode::LCtrl,
                            keyberon::key_code::KeyCode::C,
                        ].as_slice()),
                        keyberon::action::Action::MultipleActions(&[
                            keyberon::action::Action::KeyCode(keyberon::key_code::KeyCode::Q),
                            keyberon::action::Action::Layer(2usize),
                        ].as_slice()),
                        keyberon::action::Action::Layer(3usize),
                        keyberon::action::Action::DefaultLayer(2usize),
                        keyberon::action::Action::HoldTap(&keyberon::action::HoldTapAction {
                            timeout: 180u16,
                            hold: keyberon::action::Action::Layer(2usize),
                            tap: keyberon::action::Action::KeyCode(keyberon::key_code::KeyCode::Space),
                            config: keyberon::action::HoldTapConfig::Default,
                            tap_hold_interval: 100u16,
                        }),
                        keyberon::action::Action::Custom(
                            crate::keyboard::actions::Action::Mouse(
                                crate::keyboard::actions::MouseAction::Move(
                                    crate::keyboard::actions::MouseMovement::WheelDown
                                )
                            )
                        ),
                    ]
                ]
             ]
        }
    }

    #[test]
    fn deserialize() -> anyhow::Result<()> {
        let layers: Layers<custom::Action> = serde_json::from_value(example_json())?;
        assert_eq!(layers, example_config());
        Ok(())
    }

    #[test]
    fn tokenize() {
        assert_tokens_eq(to_tokens(&example_config()), example_code())
    }
}
