use proc_macro2::{TokenStream, Ident, Span};
use quote::{quote, ToTokens, TokenStreamExt};
use serde::{Serialize, Deserialize};
use schemars::JsonSchema;

use crate::{impl_struct_to_tokens, impl_enum_to_tokens};

pub type LedConfigurations = Vec<LedConfig>;

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub struct LedConfig {
    default: LayerRules,
    layers: Vec<LayerRules>,
}

pub type LayerRules = Vec<LedRule>;

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub struct LedRule {
    keys: Keys,
    condition: Condition,
    pattern: Pattern,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum Keys {
    All,
    Rows(Vec<u8>),
    Cols(Vec<u8>),
    Keys(Vec<(u8, u8)>),
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum Condition {
    Always,
    Led(KeyboardLed),
    UsbOn(bool),
    Role(Role),
    Pressed(bool),
    KeyPressed(bool, (u8, u8)),
    Not(Box<Condition>),
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum Role {
    Master,
    Slave,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum KeyboardLed {
    NumLock,
    CapsLock,
    ScrollLock,
    Compose,
    Kana,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub struct Pattern {
    repeat: Repeat,
    transitions: Vec<Transition>,
    phase: Phase,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub struct Phase {
    x: f32,
    y: f32,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum Repeat {
    Once,
    Wrap,
    Reflect,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub struct Transition {
    color: RGB8,
    duration: u16,
    interpolation: Interpolation,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum Interpolation {
    Piecewise,
    Linear,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub struct RGB8(u8, u8, u8);

pub fn to_tokens(configs: &LedConfigurations) -> TokenStream {
    quote! {
        &[ #( #configs ),* ]
    }
}

impl_enum_to_tokens! {
    enum KeyboardLed: crate::keyboard::leds::KeyboardLed,
    enum Repeat: crate::keyboard::leds::Repeat,
    enum Interpolation: crate::keyboard::leds::Interpolation,
    enum Role: crate::keyboard::role::Role,
}

impl_struct_to_tokens! {
    struct LedRule: crate::keyboard::leds::LedRule { keys, condition, pattern, }
    struct Pattern: crate::keyboard::leds::Pattern { repeat, &[transitions], phase, }
    struct Transition: crate::keyboard::leds::Transition { color, duration, interpolation, }
    struct Phase: crate::keyboard::leds::Phase { x, y, }
}

impl ToTokens for LedConfig {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let defaults = &self.default;
        let layers = &self.layers;
        tokens.append_all(quote! {
            crate::keyboard::leds::LedConfig {
                default: &[ #( #defaults ),* ],
                layers: &[ #( &[ #( #layers ),* ] ),* ],
            }
        })
    }
}

impl ToTokens for Keys {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let leds = quote! { crate::keyboard::leds };
        tokens.append_all(match self {
            Keys::All => quote! { #leds::Keys::All },
            Keys::Rows(rows) => quote! { #leds::Keys::Rows(&[ #( #rows ),* ]) },
            Keys::Cols(cols) => quote! { #leds::Keys::Cols(&[ #( #cols ),* ]) },
            Keys::Keys(keys) => {
                let keys = keys.iter().map(|(r, c)| quote! { (#r, #c) });
                 quote! { #leds::Keys::Keys(&[ #( #keys ),* ]) }
            },
        })
    }
}

impl ToTokens for Condition {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let leds = quote! { crate::keyboard::leds };
        tokens.append_all(match self {
            Condition::Always => quote! { #leds::Condition::Always },
            Condition::Led(led) => quote! { #leds::Condition::Led(#led) },
            Condition::UsbOn(on) => quote! { #leds::Condition::UsbOn(#on) },
            Condition::Role(role) => quote! { #leds::Condition::Role(#role) },
            Condition::Pressed(pressed) => quote! { #leds::Condition::Pressed(#pressed) },
            Condition::KeyPressed(pressed, (row, col)) => quote! { #leds::Condition::KeyPressed(#pressed, (#row, #col)) },
            Condition::Not(cond) => quote! { #leds::Condition::Not(&#cond) },
        })
    }
}

impl ToTokens for RGB8 {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let r = &self.0;
        let g = &self.1;
        let b = &self.2;
        tokens.append_all(quote! {
            rgb::RGB8::new(#r, #g, #b)
        })
    }
}

#[cfg(test)]
pub mod tests {
    use crate::format::assert_tokens_eq;

    use super::*;

    pub fn example_json() -> serde_json::Value {
        serde_json::json!(
            [
                {
                    "default": [
                        {
                            "keys": "All",
                            "condition": "Always",
                            "pattern": {
                                "repeat": "Wrap",
                                "transitions": [
                                    {
                                        "color": [0, 0, 0],
                                        "duration": 1500,
                                        "interpolation": "Piecewise",
                                    },
                                    {
                                        "color": [255, 180, 0],
                                        "duration": 1000,
                                        "interpolation": "Linear",
                                    },
                                ],
                                "phase": {
                                    "x": 0.0,
                                    "y": 0.0,
                                },
                            },
                        },
                        {
                            "keys": {
                                "Rows": [0, 1, 3],
                            },
                            "condition": {
                                "Pressed": true,
                            },
                            "pattern": {
                                "repeat": "Once",
                                "transitions": [
                                    {
                                        "color": [255, 255, 255],
                                        "duration": 250,
                                        "interpolation": "Linear",
                                    },
                                    {
                                        "color": [0, 0, 0],
                                        "duration": 250,
                                        "interpolation": "Linear",
                                    },
                                ],
                                "phase": {
                                    "x": 0.0,
                                    "y": 0.0,
                                },
                            },
                        },
                    ],
                    "layers": [],
                }
            ]
        )
    }

    pub fn example_config() -> LedConfigurations {
        vec![
            LedConfig {
                default: vec![
                    LedRule {
                        keys: Keys::All,
                        condition: Condition::Always,
                        pattern: Pattern {
                            repeat: Repeat::Wrap,
                            transitions: vec![
                                Transition {
                                    color: RGB8(0, 0, 0),
                                    duration: 1500,
                                    interpolation: Interpolation::Piecewise,
                                },
                                Transition {
                                    color: RGB8(255, 180, 0),
                                    duration: 1000,
                                    interpolation: Interpolation::Linear,
                                }
                            ],
                            phase: Phase { x: 0.0, y: 0.0 }
                        }
                    },
                    LedRule {
                        keys: Keys::Rows(vec![0, 1, 3]),
                        condition: Condition::Pressed(true),
                        pattern: Pattern {
                            repeat: Repeat::Once,
                            transitions: vec![
                                Transition {
                                    color: RGB8(255, 255, 255),
                                    duration: 250,
                                    interpolation: Interpolation::Linear,
                                },
                                Transition {
                                    color: RGB8(0, 0, 0),
                                    duration: 250,
                                    interpolation: Interpolation::Linear,
                                }
                            ],
                            phase: Phase { x: 0.0, y: 0.0 }
                        }
                    }
                ],
                layers: vec![],
            }
        ]
    }

    pub fn example_code() -> TokenStream {
        quote! {
            &[
                crate::keyboard::leds::LedConfig {
                    default: &[
                        crate::keyboard::leds::LedRule {
                            keys: crate::keyboard::leds::Keys::All,
                            condition: crate::keyboard::leds::Condition::Always,
                            pattern: crate::keyboard::leds::Pattern {
                                repeat: crate::keyboard::leds::Repeat::Wrap,
                                transitions: &[
                                    crate::keyboard::leds::Transition {
                                        color: rgb::RGB8::new(0u8, 0u8, 0u8),
                                        duration: 1500u16,
                                        interpolation: crate::keyboard::leds::Interpolation::Piecewise,
                                    },
                                    crate::keyboard::leds::Transition {
                                        color: rgb::RGB8::new(255u8, 180u8, 0u8),
                                        duration: 1000u16,
                                        interpolation: crate::keyboard::leds::Interpolation::Linear,
                                    }
                                ],
                                phase: crate::keyboard::leds::Phase { x: 0f32, y: 0f32 }
                            }
                        },
                        crate::keyboard::leds::LedRule {
                            keys: crate::keyboard::leds::Keys::Rows(&[0u8, 1u8, 3u8]),
                            condition: crate::keyboard::leds::Condition::Pressed(true),
                            pattern: crate::keyboard::leds::Pattern {
                                repeat: crate::keyboard::leds::Repeat::Once,
                                transitions: &[
                                    crate::keyboard::leds::Transition {
                                        color: rgb::RGB8::new(255u8, 255u8, 255u8),
                                        duration: 250u16,
                                        interpolation: crate::keyboard::leds::Interpolation::Linear,
                                    },
                                    crate::keyboard::leds::Transition {
                                        color: rgb::RGB8::new(0u8, 0u8, 0u8),
                                        duration: 250u16,
                                        interpolation: crate::keyboard::leds::Interpolation::Linear,
                                    }
                                ],
                                phase: crate::keyboard::leds::Phase { x: 0f32, y: 0f32 }
                            }
                        }
                    ],
                    layers: &[],
                }
            ]
        }
    }

    #[test]
    fn deserialize() -> anyhow::Result<()> {
        let config: LedConfigurations = serde_json::from_value(example_json())?;
        assert_eq!(config, example_config());
        Ok(())
    }

    #[test]
    fn tokenize() {
        assert_tokens_eq(to_tokens(&example_config()), example_code())
    }
}
