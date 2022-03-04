//! Layout and functions of keys on the keyboard

use keyberon::{
    action::{self, k, Action::*, HoldTapConfig},
    key_code::KeyCode::*,
    layout::{self, layout},
};

use crate::keyboard::Action as CustomAction;

pub type Layout = layout::Layout<CustomAction>;
pub type Layers = layout::Layers<CustomAction>;
type Action = action::Action<CustomAction>;

/// Get keyboard layout
pub fn layout() -> Layout {
    Layout::new(LAYERS)
}

macro_rules! ht {
    ($hold:expr, $tap:expr, $tout:expr) => {
        HoldTap {
            timeout: $tout,
            hold: &$hold,
            tap: &$tap,
            tap_hold_interval: 0,
            config: HoldTapConfig::Default,
        }
    };
    ($hold:expr, $tap:expr) => {
        ht!($hold, $tap, HOLDTAP_TIMEOUT)
    };
}

const HOLDTAP_TIMEOUT: u16 = 180;

const LCTRL_ESC: Action = ht!(k(LCtrl), k(Escape));
const RCTRL_QUOTE: Action = ht!(k(RCtrl), k(Quote));

static LAYERS: Layers = layout! {
    { // Default
        [ '`'           1 2 3 4 5   6 7 8 9 0   '\\'          ]
        [ Tab           Q W E R T   Y U I O P   BSpace        ]
        [ {LCTRL_ESC}   A S D F G   H J K L ;   {RCTRL_QUOTE} ]
        [ LShift        Z X C V B   N M , . /   RShift        ]
        [ A B C D 1 n n 2 I J K L ]
    }
};
