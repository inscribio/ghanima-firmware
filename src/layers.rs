//! Layout and functions of keys on the keyboard

use keyberon::{
    action::{self, k, Action::*, HoldTapConfig},
    key_code::KeyCode::*,
    layout::{self, layout},
};
use rgb::RGB8;

use crate::keyboard::Action as CustomAction;
use crate::keyboard::leds::*;

pub type Layout = layout::Layout<CustomAction>;
pub type Layers = layout::Layers<CustomAction>;
type Action = action::Action<CustomAction>;

/// Get keyboard layout
pub fn layout() -> Layout {
    Layout::new(LAYERS)
}

/// Get keyboard layout
pub fn led_configs() -> LedConfigurations {
    LEDS
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

const MAX: u8 = 150;

static LEDS: LedConfigurations = &[
    LedConfig {
        default: &[
            LedRule {
                keys: Keys::All,
                condition: Condition::Always,
                pattern: Pattern {
                    repeat: Repeat::Reflect,
                    transitions: &[
                        Transition {
                            color: RGB8::new(MAX, 0, 0),
                            duration: 1000,
                            interpolation: Interpolation::Linear,
                        },
                        Transition {
                            color: RGB8::new(0, MAX, 0),
                            duration: 1000,
                            interpolation: Interpolation::Linear,
                        },
                        Transition {
                            color: RGB8::new(0, 0, MAX),
                            duration: 1000,
                            interpolation: Interpolation::Linear,
                        },
                    ],
                    phase: Phase { x: 0.0, y: 0.0 },
                },
            },
            LedRule {
                keys: Keys::Rows(&[1, 3]),
                condition: Condition::Always,
                pattern: Pattern {
                    repeat: Repeat::Wrap,
                    transitions: &[
                        Transition {
                            color: RGB8::new(0, 0, 0),
                            duration: 500,
                            interpolation: Interpolation::Linear,
                        },
                        Transition {
                            color: RGB8::new(MAX, 0, MAX),
                            duration: 500,
                            interpolation: Interpolation::Linear,
                        },
                    ],
                    phase: Phase { x: 0.0, y: 0.0 },
                },
            },
            LedRule {
                keys: Keys::Cols(&[1, 4]),
                condition: Condition::Always,
                pattern: Pattern {
                    repeat: Repeat::Wrap,
                    transitions: &[
                        Transition {
                            color: RGB8::new(0, 0, 0),
                            duration: 3000,
                            interpolation: Interpolation::Linear,
                        },
                        Transition {
                            color: RGB8::new(0, MAX, MAX),
                            duration: 3000,
                            interpolation: Interpolation::Linear,
                        },
                    ],
                    phase: Phase { x: 0.0, y: 0.0 },
                },
            },
            LedRule {
                keys: Keys::Keys(&[(4, 3)]),
                condition: Condition::Always,
                pattern: Pattern {
                    repeat: Repeat::Wrap,
                    transitions: &[
                        Transition {
                            color: RGB8::new(MAX, 0, 0),
                            duration: 200,
                            interpolation: Interpolation::Linear,
                        },
                        Transition {
                            color: RGB8::new(MAX, MAX, 0),
                            duration: 200,
                            interpolation: Interpolation::Linear,
                        },
                        Transition {
                            color: RGB8::new(0, MAX, 0),
                            duration: 200,
                            interpolation: Interpolation::Linear,
                        },
                        Transition {
                            color: RGB8::new(0, MAX, MAX),
                            duration: 200,
                            interpolation: Interpolation::Linear,
                        },
                        Transition {
                            color: RGB8::new(0, 0, MAX),
                            duration: 200,
                            interpolation: Interpolation::Linear,
                        },
                        Transition {
                            color: RGB8::new(MAX, 0, MAX),
                            duration: 200,
                            interpolation: Interpolation::Linear,
                        },
                    ],
                    phase: Phase { x: 0.0, y: 0.0 },
                },
            },
            LedRule {
                keys: Keys::All,
                condition: Condition::Pressed(true),
                pattern: Pattern {
                    repeat: Repeat::Once,
                    transitions: &[
                        Transition {
                            color: RGB8::new(255, 255, 255),
                            duration: 300,
                            interpolation: Interpolation::Linear,
                        },
                        Transition {
                            color: RGB8::new(255, 255, 255),
                            duration: 100,
                            interpolation: Interpolation::Piecewise,
                        },
                        Transition {
                            color: RGB8::new(0, 0, 0),
                            duration: 300,
                            interpolation: Interpolation::Linear,
                        },
                    ],
                    phase: Phase { x: 0.0, y: 0.0 },
                },
            },
            LedRule {
                keys: Keys::Rows(&[0]),
                condition: Condition::KeyPressed(true, (3, 8)),
                pattern: Pattern {
                    repeat: Repeat::Once,
                    transitions: &[
                        Transition {
                            color: RGB8::new(MAX, 0, MAX),
                            duration: 300,
                            interpolation: Interpolation::Linear,
                        },
                        Transition {
                            color: RGB8::new(MAX, 0, MAX),
                            duration: 100,
                            interpolation: Interpolation::Piecewise,
                        },
                        Transition {
                            color: RGB8::new(0, 0, 0),
                            duration: 300,
                            interpolation: Interpolation::Linear,
                        },
                    ],
                    phase: Phase { x: 0.0, y: 0.0 },
                },
            },
            LedRule {
                keys: Keys::Keys(&[(3, 3)]),
                condition: Condition::Pressed(false),
                pattern: Pattern {
                    repeat: Repeat::Wrap,
                    transitions: &[
                        Transition {
                            color: RGB8::new(MAX, MAX, 0),
                            duration: 0,
                            interpolation: Interpolation::Piecewise,
                        },
                    ],
                    phase: Phase { x: 0.0, y: 0.0 },
                },
            },
        ],
        layers: &[
            &[],
        ],
    },
];
