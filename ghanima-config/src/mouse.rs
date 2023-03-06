use quote::{quote, ToTokens, TokenStreamExt};
use serde::{Serialize, Deserialize};
use schemars::JsonSchema;

use crate::impl_struct_to_tokens;

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub struct MouseConfig {
    x: AxisConfig,
    y: AxisConfig,
    wheel: AxisConfig,
    pan: AxisConfig,
    joystick: JoystickConfig,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub struct AxisConfig {
    invert: bool,
    // TODO: optimize for size by detecting same profiles and extracting to separate variable
    profile: SpeedProfile,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub struct SpeedProfile {
    divider: u16,
    delay: u16,
    acceleration_time: u16,
    start_speed: u16,
    max_speed: u16,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub struct JoystickConfig {
    min: u16,
    max: u16,
    divider: u16,
    swap_axes: bool,
    invert_x: bool,
    invert_y: bool,
}

impl_struct_to_tokens! {
    struct MouseConfig: crate::keyboard::mouse::MouseConfig { x, y, wheel, pan, joystick, }
    struct AxisConfig: crate::keyboard::mouse::AxisConfig { invert, &profile, }
    struct SpeedProfile: crate::keyboard::mouse::SpeedProfile { divider, delay, acceleration_time, start_speed, max_speed, }
    struct JoystickConfig: crate::keyboard::mouse::JoystickConfig { min, max, divider, swap_axes, invert_x, invert_y, }
}

#[cfg(test)]
pub mod tests {
    use proc_macro2::TokenStream;
    use crate::format::assert_tokens_eq;
    use super::*;

    pub fn example_json() -> serde_json::Value {
        serde_json::json!({
                "x": {
                "invert": false,
                "profile": {
                "divider": 10000,
                "delay": 50,
                "acceleration_time": 750,
                "start_speed": 5000,
                "max_speed": 15000,
            },
            },
                "y": {
                "invert": false,
                "profile": {
                "divider": 10000,
                "delay": 50,
                "acceleration_time": 750,
                "start_speed": 5000,
                "max_speed": 15000,
            },
            },
                "wheel": {
                "invert": true,
                "profile": {
                "divider": 1000,
                "delay": 50,
                "acceleration_time": 750,
                "start_speed": 25,
                "max_speed": 50,
            },
            },
                "pan": {
                "invert": false,
                "profile": {
                "divider": 1000,
                "delay": 50,
                "acceleration_time": 750,
                "start_speed": 25,
                "max_speed": 50,
            },
            },
                "joystick": {
                "min": 175,
                "max": 4000,
                "divider": 800,
                "swap_axes": false,
                "invert_x": false,
                "invert_y": true,
            },
        })
    }

    pub fn example_config() -> MouseConfig {
        MouseConfig {
            x: AxisConfig {
                invert: false,
                profile: SpeedProfile {
                    divider: 10000,
                    delay: 50,
                    acceleration_time: 750,
                    start_speed: 5000,
                    max_speed: 15000,
                }
            },
            y: AxisConfig {
                invert: false,
                profile: SpeedProfile {
                    divider: 10000,
                    delay: 50,
                    acceleration_time: 750,
                    start_speed: 5000,
                    max_speed: 15000,
                }
            },
            wheel: AxisConfig {
                invert: true,
                profile: SpeedProfile {
                    divider: 1000,
                    delay: 50,
                    acceleration_time: 750,
                    start_speed: 25,
                    max_speed: 50,
                }
            },
            pan: AxisConfig {
                invert: false,
                profile: SpeedProfile {
                    divider: 1000,
                    delay: 50,
                    acceleration_time: 750,
                    start_speed: 25,
                    max_speed: 50,
                }
            },
            joystick: JoystickConfig {
                min: 175,
                max: 4000,
                divider: 800,
                swap_axes: false,
                invert_x: false,
                invert_y: true,
            }
        }
    }

    pub fn example_code() -> TokenStream {
        quote! {
            crate::keyboard::mouse::MouseConfig {
                x: crate::keyboard::mouse::AxisConfig {
                    invert: false,
                    profile: &crate::keyboard::mouse::SpeedProfile {
                        divider: 10000u16,
                        delay: 50u16,
                        acceleration_time: 750u16,
                        start_speed: 5000u16,
                        max_speed: 15000u16,
                    }
                },
                y: crate::keyboard::mouse::AxisConfig {
                    invert: false,
                    profile: &crate::keyboard::mouse::SpeedProfile {
                        divider: 10000u16,
                        delay: 50u16,
                        acceleration_time: 750u16,
                        start_speed: 5000u16,
                        max_speed: 15000u16,
                    }
                },
                wheel: crate::keyboard::mouse::AxisConfig {
                    invert: true,
                    profile: &crate::keyboard::mouse::SpeedProfile {
                        divider: 1000u16,
                        delay: 50u16,
                        acceleration_time: 750u16,
                        start_speed: 25u16,
                        max_speed: 50u16,
                    }
                },
                pan: crate::keyboard::mouse::AxisConfig {
                    invert: false,
                    profile: &crate::keyboard::mouse::SpeedProfile {
                        divider: 1000u16,
                        delay: 50u16,
                        acceleration_time: 750u16,
                        start_speed: 25u16,
                        max_speed: 50u16,
                    }
                },
                joystick: crate::keyboard::mouse::JoystickConfig {
                    min: 175u16,
                    max: 4000u16,
                    divider: 800u16,
                    swap_axes: false,
                    invert_x: false,
                    invert_y: true,
                }
            }
        }
    }

    #[test]
    fn deserialize() -> anyhow::Result<()> {
        let mouse: MouseConfig = serde_json::from_value(example_json())?;
        assert_eq!(mouse, example_config());
        Ok(())
    }

    #[test]
    fn tokenize() {
        let mouse = example_config();
        assert_tokens_eq(quote! { #mouse }, example_code())
    }
}
