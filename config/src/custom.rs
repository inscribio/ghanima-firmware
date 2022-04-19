use proc_macro2::{TokenStream, Ident, Span};
use quote::{quote, ToTokens, TokenStreamExt};
use serde::{Serialize, Deserialize};
use schemars::JsonSchema;

use crate::{impl_enum_to_tokens, impl_enum_tuple_to_tokens};

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
#[schemars(rename = "CustomAction")]
pub enum Action {
    /// Modify LED lightning
    Led(LedAction),
    /// Use mouse emulation
    Mouse(MouseAction),
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum LedAction {
    /// Cycle through available LED configurations
    Cycle(Inc),
    /// Modify global brightness
    Brightness(Inc),
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum MouseAction {
    /// Key emulates a mouse key
    Click(MouseButton),
    /// Key performs mouse movement when held
    Move(MouseMovement),
    /// Key changes mouse sensitivity
    Sensitivity(Inc),
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum MouseButton {
    Left,
    Mid,
    Right,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
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

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum Inc {
    Up,
    Down,
}

impl_enum_to_tokens! {
    enum MouseButton: crate::keyboard::actions::MouseButton,
    enum MouseMovement: crate::keyboard::actions::MouseMovement,
    enum Inc: crate::utils::Inc,
}

impl_enum_tuple_to_tokens! {
    enum Action: crate::keyboard::actions::Action { Led(led), Mouse(mouse) }
    enum LedAction: crate::keyboard::actions::LedAction { Cycle(inc), Brightness(inc) }
    enum MouseAction: crate::keyboard::actions::MouseAction { Click(button), Move(movement), Sensitivity(inc) }
}

#[cfg(test)]
pub mod tests {
    use proc_macro2::TokenStream;
    use crate::format::assert_tokens_eq;
    use super::*;

    pub fn example_json() -> serde_json::Value {
        serde_json::json!({
            "Led": { "Cycle": "Up" }
        })
    }

    pub fn example_config() -> Action {
        Action::Led(LedAction::Cycle(Inc::Up))
    }

    pub fn example_code() -> TokenStream {
        quote! {
            crate::keyboard::actions::Action::Led(
                crate::keyboard::actions::LedAction::Cycle(
                    crate::utils::Inc::Up
                )
            )
        }
    }

    #[test]
    fn deserialize() -> anyhow::Result<()> {
        let v: Action = serde_json::from_value(example_json())?;
        assert_eq!(v, example_config());
        Ok(())
    }

    #[test]
    fn tokenize() {
        let q = example_config();
        assert_tokens_eq(quote! { #q }, example_code())
    }
}
