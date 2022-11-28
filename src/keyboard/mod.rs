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

use usb_device::UsbError;
use usb_device::device::UsbDeviceState;
use usbd_human_interface_device::UsbHidError;
use crate::bsp::sides::BoardSide;
use crate::bsp::usb::Usb;
use crate::bsp::{NCOLS, NROWS};
use crate::ioqueue;
use role::Role;
use actions::{Action, LedAction, Inc};
use keyberon::layout::CustomEvent;
use keys::PressedLedKeys;
use hid::KeyCodeIterExt as _;

pub use keys::Keys;
pub use leds::{LedController, KeyboardState};

/// Transmitter of packets for communication between keyboard halves
pub type Transmitter<TX, const N: usize> = ioqueue::Transmitter<msg::Message, TX, N>;
/// Receiver of packets for communication between keyboard halves
pub type Receiver<RX, const N: usize, const B: usize> = ioqueue::Receiver<msg::Message, RX, N, B>;

/// Split keyboard logic
pub struct Keyboard<const L: usize> {
    keys: keys::Keys,
    fsm: role::Fsm,
    layout: layout::Layout<{ 2 * NCOLS }, NROWS, L, Action>,
    mouse: mouse::Mouse,
    prev_update: LedsUpdate,
    prev_usb_state: UsbDeviceState,
    pressed_other: PressedLedKeys,
    keyboard_reports: hid::HidReportQueue<hid::KeyboardReport, 8>,
    consumer_reports: hid::HidReportQueue<hid::ConsumerReport, 1>,
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
    /// Do not jump to bootloader until FirmwareAction::AllowBootloader is pressed
    pub bootload_strict: bool,
}

/// Deferred update of LED controller state
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct LedsUpdate {
    state: KeyboardState,
    config: Option<Inc>,
    brightness: Option<BrightnessUpdate>,
}

/// Deferred update of LED controller state
#[derive(Clone, Serialize, Deserialize, PartialEq)]
enum BrightnessUpdate {
    Up,
    Down,
    Disable,
    Enable,
}

impl From<Inc> for BrightnessUpdate {
    fn from(inc: Inc) -> Self {
        match inc {
            Inc::Up => Self::Up,
            Inc::Down => Self::Down,
        }
    }
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
        let prev_update = LedsUpdate {
            state: prev_state,
            config: None,
            brightness: None,
        };
        let pressed_other = Default::default();
        let keyboard_reports = hid::HidReportQueue::new();
        let consumer_reports = hid::HidReportQueue::new();
        let keyboard = Self {
            keys,
            fsm,
            layout,
            mouse,
            prev_update,
            pressed_other,
            keyboard_reports,
            consumer_reports,
            prev_usb_state: UsbDeviceState::Default,
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

        // Retrieve USB state
        let usb_state = usb.dev.state();
        let prev_usb_state = self.prev_usb_state;
        self.prev_usb_state = usb_state;

        // First update USB state in FSM
        maybe_tx(tx, self.fsm.usb_state(usb_state == UsbDeviceState::Configured));

        // Store LEDs updates from master
        let mut leds_update = None;

        // Process RX data
        let mut was_event = false;
        while let Some(msg) = rx.get() {
            match msg {
                msg::Message::Role(msg) => {
                    defmt::info!("Got role::Message: {}", msg);
                    maybe_tx(tx, self.fsm.on_rx(msg));
                },
                msg::Message::Key(event) => {
                    was_event = true;
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
            was_event = true;
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

        // Process USB wake up FIXME: assumes keyboard tick is 1 kHz
        usb.wake_up_update(was_event, 9);

        if self.fsm.role() == Role::Slave {
            // Slave just uses the LED update from master

            // Update previous state
            if let Some(update) = leds_update.as_ref() {
                self.prev_update = update.clone();
            }

            // Return the new update or use state from previous one
            leds_update.unwrap_or(LedsUpdate {
                state: self.prev_update.state.clone(),
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
                    leds: usb.keyboard_leds(),
                    usb_on: usb_state == UsbDeviceState::Configured,
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
            // self.keyboard_reports.push(self.layout.keycodes().collect());
            if let Some((action, pressed)) = custom.transposed() {
                self.handle_action(action, pressed, &mut update, usb);
            }

            // Advance mouse emulation time
            self.mouse.tick();

            // Advance usbd-human-interface-device keyboard time FIXME: assumes 1 kHz
            let keyboard: &hid::KeyboardInterface<'_, _> = usb.hid.interface();
            keyboard.tick().ok();

            // Push next report
            self.keyboard_reports.push(hid::KeyboardReport::new(self.layout.keycodes().as_page()));

            // Push USB reports
            if self.fsm.role() == Role::Master && usb_state == UsbDeviceState::Configured {
                let consumer: &hid::ConsumerInterface<'_, _> = usb.hid.interface();
                let mouse: &hid::MouseInterface<'_, _> = usb.hid.interface();

                self.keyboard_reports.send(|r| keyboard.write_report(r)
                    .or_else(|e| match e {
                        UsbHidError::WouldBlock => Err(UsbError::WouldBlock),
                        UsbHidError::Duplicate => Ok(()),
                        UsbHidError::UsbError(e) => Err(e),
                        UsbHidError::SerializationError => Err(UsbError::ParseError),
                    })
                    .map(|_| 1));

                self.consumer_reports.send(|r| consumer.write_report(r));

                // Try to push USB mouse report
                self.mouse.push_report(|r| {
                    match mouse.write_report(r) {
                        Ok(_) => true,
                        Err(e) => match e {
                            UsbHidError::WouldBlock | UsbHidError::UsbError(UsbError::WouldBlock) => false,
                            UsbHidError::Duplicate => false,
                            _ => panic!("Unexpected UsbHidError"),
                        },
                    }
                });
            } else if usb_state != UsbDeviceState::Configured {
                self.keyboard_reports.clear();
                self.consumer_reports.clear();
            }

            // Disable LEDs when entering suspend mode
            match (prev_usb_state, usb_state) {
                (UsbDeviceState::Suspend, UsbDeviceState::Suspend) => {},
                (_, UsbDeviceState::Suspend) => update.brightness = Some(BrightnessUpdate::Disable),
                (UsbDeviceState::Suspend, _) => update.brightness = Some(BrightnessUpdate::Enable),
                _ => {},
            }

            // Transfer LED updates
            // TODO: the other half still uses it's own configuration so this won't be correct if
            // each half has different firmware loaded
            // TODO: need to synchronize time between halves!
            if update.any_change(&self.prev_update) {
                defmt::info!("Send Leds(left={=u32}, right={=u32})",
                    update.state.pressed_left.get_raw(),
                    update.state.pressed_right.get_raw(),
                );
                tx.push(msg::Message::Leds(update.clone()));
                self.prev_update = update.clone();
            }

            update
        }
    }

    /// Set new joystick reading values
    pub fn update_joystick(&mut self, xy: (i16, i16)) {
        self.mouse.update_joystick(xy);
    }

    fn handle_action(&mut self, action: &Action, pressed: bool, update: &mut LedsUpdate, usb: &mut Usb) {
        match action {
            Action::Led(led) => if !pressed {  // only on release
                match led {
                    LedAction::Cycle(inc) => update.config = Some((*inc).into()),
                    LedAction::Brightness(inc) => update.brightness = Some((*inc).into()),
                }
            },
            Action::Mouse(mouse) => self.mouse.handle_action(&mouse, pressed),
            Action::Consumer(key) => {
                let mut report = hid::ConsumerReport::default();
                if pressed {
                    report.codes[0] = (*key).into()
                }
                self.consumer_reports.push(report);
            },
            Action::Firmware(fw) => if pressed {
                let bus = usb.dev.bus();
                let dfu_boot = usb.dfu.ops_mut();
                match fw {
                    actions::FirmwareAction::AllowBootloader => dfu_boot.set_allowed(true),
                    actions::FirmwareAction::JumpToBootloader => dfu_boot.reboot(true, Some(bus)),
                    actions::FirmwareAction::Reboot => dfu_boot.reboot(false, Some(bus)),
                }
            }
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
                BrightnessUpdate::Up => leds.brightness().saturating_add(Self::BRIGHTNESS_INC),
                BrightnessUpdate::Down => leds.brightness().saturating_sub(Self::BRIGHTNESS_INC),
                BrightnessUpdate::Disable => 0,
                BrightnessUpdate::Enable => LedController::INITIAL_BRIGHTNESS,
            };
            leds.set_brightness(new);
        }
        leds.update_patterns(time, self.state);
    }

    /// Determine this update is meaningful (there is any change)
    pub fn any_change(&self, previous: &Self) -> bool {
        self.config.is_some() || self.brightness.is_some() || self.state != previous.state
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
