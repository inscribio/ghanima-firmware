//! Main USB keyboard logic
//!
//! Implementation of split-keyboard logic based on the [`keyberon`] crate.
//! Contains firmware extensions such as communication between keyboard halves
//! and handling of custom events.

/// Special keyboard actions
pub mod actions;
/// Keyboard related USB HID classes
pub mod hid;
/// Keyboard matrix scanner with debouncing
mod keys;
/// Keyboard lightning control and configuration
pub mod leds;
/// Mouse emulation
pub mod mouse;
/// Messages sent between keyboard halves
mod msg;
/// Role negotiation between keyboard halves
mod role;

use keyberon::layout::{self, Event};
use serde::{Serialize, Deserialize};

use usb_device::device::UsbDeviceState;
use crate::bsp::sides::BoardSide;
use crate::bsp::usb::Usb;
use crate::bsp::{NCOLS, NROWS};
use crate::ioqueue;
use role::Role;
use leds::KeyboardState;
use actions::{Action, LedAction, Inc};
use keyberon::layout::CustomEvent;
use keys::PressedLedKeys;
use hid::{KeyboardReport, HidReportQueue};

pub use keys::Keys;
pub use leds::LedController;


/// Transmitter of packets for communication between keyboard halves
pub type Transmitter<TX, const N: usize> = ioqueue::Transmitter<msg::Message, TX, N>;
/// Receiver of packets for communication between keyboard halves
pub type Receiver<RX, const N: usize, const B: usize> = ioqueue::Receiver<msg::Message, RX, N, B>;

/// Split keyboard logic
pub struct Keyboard<const L: usize> {
    keys: keys::Keys,
    fsm: role::Fsm,
    layout: layout::Layout<{ 2 * NCOLS}, NROWS, L, Action>,
    mouse: mouse::Mouse,
    prev_state: KeyboardState,
    pressed_other: PressedLedKeys,
    keyboard_reports: HidReportQueue<KeyboardReport, 8>,
}

/// Keyboard configuration
pub struct KeyboardConfig<const L: usize> {
    /// Keyboard layers configuration
    pub layers: &'static layout::Layers<{ 2 * NCOLS}, NROWS, L, actions::Action>,
    /// Configuration of mouse emulation
    pub mouse: &'static mouse::MouseConfig,
    /// Configuration of RGB LED lightning
    pub leds: leds::LedConfigurations,
    /// Timeout for polling the other half about role negotiation
    pub timeout: u32,
}

/// Deferred update of LED controller state
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct LedsUpdate {
    state: KeyboardState,
    config: Option<Inc>,
    brightness: Option<Inc>,
}

impl<const L: usize> Keyboard<L> {
    /// Crate new keyboard with given layout and negotiation timeout specified in "ticks"
    /// (see [`Self::tick`])
    pub fn new(keys: keys::Keys, config: &KeyboardConfig<L>) -> (Self, LedController) {
        let side = *keys.side();
        let leds = LedController::new(side, &config.leds);
        let fsm = role::Fsm::with(side, config.timeout);
        let layout = layout::Layout::new(config.layers);
        let mouse = mouse::Mouse::new(config.mouse);
        let prev_state = KeyboardState {
            leds: hid::KeyboardLeds(0),
            usb_on: false,
            role: fsm.role(),
            layer: layout.current_layer() as u8,
            pressed_left: Default::default(),
            pressed_right: Default::default(),
        };
        let pressed_other = Default::default();
        let keyboard_reports = HidReportQueue::new();
        let keyboard = Self {
            keys,
            fsm,
            layout,
            mouse,
            prev_state,
            pressed_other,
            keyboard_reports,
        };
        (keyboard, leds)
    }

    /// Get current role
    pub fn role(&self) -> Role {
        self.fsm.role()
    }

    /// Periodic keyboard events processing
    ///
    /// This should be called in a fixed period to update internal state, handle communication
    /// between keyboard halves and resolve key events depending on keyboard layout. Returns
    /// [`KeyboardState`] to be passed to the LED controller - possibly a lower priority task.
    pub fn tick<TX, RX>(&mut self, (tx, rx): (&mut TX, &mut RX), usb: &mut Usb) -> LedsUpdate
    where
        TX: ioqueue::TransmitQueue<msg::Message>,
        RX: ioqueue::ReceiveQueue<msg::Message>,
    {
        let maybe_tx = |tx: &mut TX, msg: Option<role::Message>| {
            if let Some(msg) = msg {
                tx.push(msg::Message::Role(msg));
            }
        };

        // First update USB state in FSM
        maybe_tx(tx, self.fsm.usb_state(usb.dev.state() == UsbDeviceState::Configured));

        // Store LEDs updates from master
        let mut leds_update = None;

        // Process RX data
        while let Some(msg) = rx.get() {
            match msg {
                msg::Message::Role(msg) => {
                    defmt::info!("Got role::Message: {}", msg);
                    maybe_tx(tx, self.fsm.on_rx(msg));
                },
                msg::Message::Key(event) => {
                    match event {
                        Event::Press(i, j) => defmt::info!("Got KeyPress({=u8}, {=u8})", i, j),
                        Event::Release(i, j) => defmt::info!("Got KeyRelease({=u8}, {=u8})", i, j),
                    }
                    // Update pressed keys for the other half
                    self.pressed_other.update(&event.transform(|i, j| {
                        BoardSide::coords_to_local((i, j))
                    }));
                    // Only master uses key events from the other half
                    if self.fsm.role() == Role::Master {
                        self.layout.event(event);
                    }
                },
                msg::Message::Leds(leds) => if self.fsm.role() == Role::Slave {
                    leds_update = Some(leds);
                },
            }
        }

        // Advance FSM time, process timeouts
        maybe_tx(tx, self.fsm.tick());

        // Scan keys and push all events
        for event in self.keys.scan() {
            match self.fsm.role() {
                // Master should handle keyboard logic
                Role::Master => self.layout.event(event),
                // Slave should only send key events to master
                Role::Slave => {
                    let (i, j) = event.coord();
                    defmt::info!("Send Key({=u8}, {=u8})", i, j);
                    tx.push(msg::Message::Key(event));
                },
            }
        }

        if self.fsm.role() == Role::Slave {
            // Slave just uses the LED update from master

            // Update previous state
            if let Some(update) = leds_update.as_ref() {
                self.prev_state = update.state.clone();
            }

            // Return the new update or use state from previous one
            leds_update.unwrap_or(LedsUpdate {
                state: self.prev_state.clone(),
                config: None,
                brightness: None,
            })
        } else {
            // Master keeps track of the actual keyboard state

            // Get pressed keys state for each side
            let (pressed_left, pressed_right) = match self.keys.side() {
                BoardSide::Left => (self.keys.pressed(), self.pressed_other),
                BoardSide::Right => (self.pressed_other, self.keys.pressed()),
            };
            // Collect state
            let mut update = LedsUpdate {
                state: leds::KeyboardState {
                    leds: usb.keyboard.leds().0, // pulls data from HidClass
                    usb_on: usb.dev.state() == UsbDeviceState::Configured,
                    role: self.fsm.role(),
                    layer: self.layout.current_layer() as u8,
                    pressed_left,
                    pressed_right,
                },
                config: None,
                brightness: None,
            };

            // TODO: auto-enable NumLock by checking leds state
            // Advance keyboard time
            let custom = self.layout.tick();
            self.keyboard_reports.push(self.layout.keycodes().collect());
            if let Some((action, pressed)) = custom.transposed() {
                self.handle_action(action, pressed, &mut update);
            }

            // Advance mouse emulation time
            self.mouse.tick();

            // Push USB reports
            if self.fsm.role() == Role::Master && usb.dev.state() == UsbDeviceState::Configured {
                self.keyboard_reports.send(|r| usb.keyboard.push_keyboard_report(r));

                // Try to push USB mouse report
                self.mouse.push_report(&usb.mouse);
            }

            // Transfer LED updates
            // TODO: the other half still uses it's own configuration so this won't be correct if
            // each half has different firmware loaded
            // TODO: need to synchronize time between halves!
            if update.config.is_some() || update.brightness.is_some() || update.state != self.prev_state {
                defmt::info!("Send Leds(left={=u32}, right={=u32})",
                    update.state.pressed_left.get_raw(),
                    update.state.pressed_right.get_raw(),
                );
                tx.push(msg::Message::Leds(update.clone()));
                self.prev_state = update.state.clone();
            }

            update
        }
    }

    /// Set new joystick reading values
    pub fn update_joystick(&mut self, xy: (i16, i16)) {
        self.mouse.update_joystick(xy);
    }

    fn handle_action(&mut self, action: &Action, pressed: bool, update: &mut LedsUpdate) {
        match action {
            Action::Led(led) => if !pressed {  // only on release
                match led {
                    LedAction::Cycle(inc) => update.config = Some(*inc),
                    LedAction::Brightness(inc) => update.brightness = Some(*inc),
                }
            },
            Action::Mouse(mouse) => self.mouse.handle_action(mouse, pressed),
        };
    }
}

impl LedsUpdate {
    const BRIGHTNESS_LEVELS: u8 = 8;
    const BRIGHTNESS_INC: u8 = u8::MAX / Self::BRIGHTNESS_LEVELS;

    /// Perform LED controller update
    pub fn apply(self, time: u32, leds: &mut LedController) {
        if let Some(inc) = self.config {
            leds.cycle_config(inc);
        }
        if let Some(inc) = self.brightness {
            let new = match inc {
                Inc::Up => leds.brightness().saturating_add(Self::BRIGHTNESS_INC),
                Inc::Down => leds.brightness().saturating_sub(Self::BRIGHTNESS_INC),
            };
            leds.set_brightness(new);
        }
        leds.update_patterns(time, self.state);
    }
}

/// Extension trait for [`CustomEvent`]
pub trait CustomEventExt<T: 'static> {
    /// Convert NoEvent into None, else return Some(T, pressed)
    fn transposed(self) -> Option<(&'static T, bool)>;
}

impl<T> CustomEventExt<T> for CustomEvent<T> {
    fn transposed(self) -> Option<(&'static T, bool)> {
        match self {
            CustomEvent::NoEvent => None,
            CustomEvent::Press(act) => Some((act, true)),
            CustomEvent::Release(act) => Some((act, false)),
        }
    }
}
