//! Layout and functions of keys on the keyboard

use keyberon::{
    action::{self, k, l, d, m, Action::*, HoldTapConfig},
    key_code::KeyCode::*,
    layout::{self, layout},
};
use rgb::RGB8;

use crate::keyboard::actions::{Action as CustomAction, MouseAction, MouseButton, MouseMovement, Inc};
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

const HOLDTAP_TIMEOUT: u16 = 180;
const HTC: HoldTapConfig = HoldTapConfig::Default;

macro_rules! ht {
    ($hold:expr, $tap:expr, $tout:expr) => {
        HoldTap {
            timeout: $tout,
            hold: &$hold,
            tap: &$tap,
            tap_hold_interval: 0,
            config: HTC,
        }
    };
    ($hold:expr, $tap:expr) => {
        ht!($hold, $tap, HOLDTAP_TIMEOUT)
    };
}

const L1_SPACE: Action = ht!(l(1), k(Space));
const L2_ENTER: Action = ht!(l(2), k(Enter));
const L3_SPACE: Action = ht!(l(3), k(Space));
const L3_ENTER: Action = ht!(l(3), k(Enter));
const LDEF: Action = d(0);
const LGAM: Action = d(4);

const LCTRL_ESC: Action = ht!(k(LCtrl), k(Escape));
const RCTRL_QUOTE: Action = ht!(k(RCtrl), k(Quote));

const CA_LEFT: Action = m(&[LCtrl, LAlt, Left]);
const CA_RIGHT: Action = m(&[LCtrl, LAlt, Right]);
const CA_UP: Action = m(&[LCtrl, LAlt, Up]);
const CA_DOWN: Action = m(&[LCtrl, LAlt, Down]);
const SG_LEFT: Action = m(&[LShift, LGui, Left]);
const SG_RIGHT: Action = m(&[LShift, LGui, Right]);
const SG_PGUP: Action = m(&[LGui, LShift, PgUp]);
const SG_PGDOWN: Action = m(&[LGui, LShift, PgDown]);

const PSCREEN_ALL: Action = k(PScreen);
const PSCREEN_WIN: Action = m(&[LAlt, PScreen]);
const PSCREEN_SEL: Action = m(&[LShift, PScreen]);

const PREVIOUS: Action = k(MediaPreviousSong);
const NEXT: Action = k(MediaNextSong);
const PLAYPAUSE: Action = k(MediaPlayPause);
const STOP: Action = k(MediaStop);
const MUTE: Action = k(MediaMute);
const VOL_UP: Action = k(MediaVolUp);
const VOL_DOWN: Action = k(MediaVolDown);

const M_L: Action = Action::Custom(CustomAction::Mouse(MouseAction::Click(MouseButton::Left)));
const M_R: Action = Action::Custom(CustomAction::Mouse(MouseAction::Click(MouseButton::Right)));
const M_M: Action = Action::Custom(CustomAction::Mouse(MouseAction::Click(MouseButton::Mid)));
const M_UP: Action = Action::Custom(CustomAction::Mouse(MouseAction::Move(MouseMovement::Up)));
const M_DOWN: Action = Action::Custom(CustomAction::Mouse(MouseAction::Move(MouseMovement::Down)));
const M_LEFT: Action = Action::Custom(CustomAction::Mouse(MouseAction::Move(MouseMovement::Left)));
const M_RIGHT: Action = Action::Custom(CustomAction::Mouse(MouseAction::Move(MouseMovement::Right)));
const M_S_UP: Action = Action::Custom(CustomAction::Mouse(MouseAction::Move(MouseMovement::WheelUp)));
const M_S_DOWN: Action = Action::Custom(CustomAction::Mouse(MouseAction::Move(MouseMovement::WheelDown)));
const M_PLUS: Action = Action::Custom(CustomAction::Mouse(MouseAction::Sensitivity(Inc::Up)));
const M_MINUS: Action = Action::Custom(CustomAction::Mouse(MouseAction::Sensitivity(Inc::Down)));

static LAYERS: Layers = layout! {
    { // Default
        [ '`'           1 2 3 4 5   6 7 8 9 0   '\\'          ]
        [ Tab           Q W E R T   Y U I O P   BSpace        ]
        [ {LCTRL_ESC}   A S D F G   H J K L ;   {RCTRL_QUOTE} ]
        [ LShift        Z X C V B   N M , . /   RShift        ]
        // [ Tab           {KQ} {KW} {KE} {KR} {KT}   {KY} {KU} {KI} {KO} {KP}   BSpace        ]
        // [ {LCTRL_ESC}   {KA} {KS} {KD} {KF} {KG}   {KH} {KJ} {KK} {KL} ;   {RCTRL_QUOTE} ]
        // [ LShift        {KZ} {KX} {KC} {KV} {KB}   {KN} {KM} , . /   RShift        ]
        [ n n
            LGui LAlt {L1_SPACE}
                Space
                Enter
            {L2_ENTER} RAlt LGui
        n n ]
    }
    { // Layer 1 (hold left)
        [ F12   F1      F2   F3   F4    F5       F6 F7         F8   F9  F10   F11    ]
        // FIXME: use [LAlt Q], but it fails if not being the last one
        [ t     t       Home Up   End   PgUp     t  '('        ')'  '_' +     Delete ]
        [ t     t       Left Down Right PgDown   t  '['        ']'  -   =     t      ]
        [ t     t       t    t    t     t        t  '{'        '}'  t   t     t      ]
        [ t     t       t    t    t     Delete   t  {L3_ENTER} LAlt t   t     t      ]
    }
    { // Layer 2 (hold right)
        [ Insert     t      t      t   t          t   t t     t   t   t   t ]
        [ t          *      Kp7    Kp8 Kp9        -   t '('   ')' '_' +   t ]
        [ CapsLock   /      Kp4    Kp5 Kp6        +   t '['   ']' -   =   t ]
        [ t          Kp0    Kp1    Kp2 Kp3        =   t Enter t   t   t   t ]
        [ t          t      t      t   {L3_SPACE} t   t t     t   t   t   t ]
    }
    { // Layer 3 (hold left->right or right->left)
        [ t   {LDEF}     {LGAM}     t           t          t             t          {M_MINUS}  {M_M}    {M_PLUS}  t               t ]
        [ t   {VOL_UP}   {SG_LEFT}  {CA_UP}     {SG_RIGHT} {SG_PGUP}     {M_S_UP}   {M_L}      {M_UP}   {M_R}     {PSCREEN_SEL}   t ]
        [ t   {VOL_DOWN} {CA_LEFT}  {CA_DOWN}   {CA_RIGHT} {SG_PGDOWN}   {M_S_DOWN} {M_LEFT}   {M_DOWN} {M_RIGHT} {PSCREEN_WIN}   t ]
        [ t   {MUTE}     {PREVIOUS} {PLAYPAUSE} {NEXT}     {STOP}        {M_L}      t          t        t         {PSCREEN_ALL}   t ]
        [ t   t          t          t           t          t             t          t          t        t         t               t ]
    }
    { // Default for gaming, etc.
        [ Escape   1 2 3 4 5   6 7 8 9 0   '\\'          ]
        [ Tab      Q W E R T   Y U I O P   BSpace        ]
        [ LCtrl    A S D F G   H J K L ;   {RCTRL_QUOTE} ]
        [ LShift   Z X C V B   N M , . /   RShift        ]
        [ n n
            LGui LAlt Space
                {L1_SPACE}
                {L2_ENTER}
            Enter RAlt LGui
        n n ]
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
