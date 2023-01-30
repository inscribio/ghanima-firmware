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

use rtic::Mutex;
use keyberon::layout::{self, Event};
use serde::{Serialize, Deserialize};

use usb_device::UsbError;
use usb_device::device::UsbDeviceState;
use usbd_human_interface_device::UsbHidError;
use crate::bsp::sides::{BoardSide, PerSide};
use crate::bsp::usb::Usb;
use crate::bsp::{NCOLS, NROWS};
use crate::ioqueue;
use crate::utils::OptionChanges as _;
use role::Role;
use actions::{Action, LedAction, Inc};
use keyberon::layout::CustomEvent;
use keys::PressedLedKeys;
use hid::KeyCodeIterExt as _;

pub use keys::Keys;
pub use leds::{LedController, LedOutput, KeyboardState};

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
    state: Option<KeyboardState>,
    prev_usb_state: UsbDeviceState,
    pressed: PerSide<PressedLedKeys>,
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
    state: Option<KeyboardState>,
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
    pub fn new(keys: keys::Keys, config: &KeyboardConfig<L>) -> Self {
        let side = *keys.side();
        let fsm = role::Fsm::with(side, config.timeout);
        let layout = layout::Layout::new(config.layers);
        let mouse = mouse::Mouse::new(config.mouse);
        let prev_state = KeyboardState {
            leds: hid::KeyboardLeds(0),
            usb_on: false,
            role: fsm.role(),
            layer: layout.current_layer() as u8,
            pressed: Default::default(),
        };
        let pressed = Default::default();
        let keyboard_reports = hid::HidReportQueue::new();
        let consumer_reports = hid::HidReportQueue::new();
        Self {
            keys,
            fsm,
            layout,
            mouse,
            state: None,
            pressed,
            keyboard_reports,
            consumer_reports,
            prev_usb_state: UsbDeviceState::Default,
        }
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
    pub fn tick<TX, RX>(
        &mut self,
        (mut tx, mut rx): (impl Mutex<T=TX>, impl Mutex<T=RX>),
        mut usb: impl Mutex<T=&'static mut Usb>,
    ) -> LedsUpdate
    where
        TX: ioqueue::TransmitQueue<msg::Message>,
        RX: ioqueue::ReceiveQueue<msg::Message>,
    {
        // Retrieve USB state
        let (usb_state, keyboard_leds) = usb.lock(|usb| (usb.dev.state(), usb.keyboard_leds()));
        let prev_usb_state = self.prev_usb_state;
        self.prev_usb_state = usb_state;

        // First update USB state in FSM
        if let Some(msg) = self.fsm.usb_state(usb_state == UsbDeviceState::Configured) {
            tx.lock(|tx| tx.push(msg.into()));
        }

        // Store LEDs updates from master
        let mut leds_update = None;

        // Process RX data
        let mut was_key_event = false;  // check events as any key should trigger usb wakeup from suspend
        while let Some(msg) = rx.lock(|rx| rx.get()) {
            match msg {
                msg::Message::Role(msg) => {
                    defmt::info!("Got role::Message: {}", msg);
                    if let Some(msg) =  self.fsm.on_rx(msg) {
                        tx.lock(|tx| tx.push(msg.into()));
                    }
                },
                msg::Message::Key(event) => {
                    was_key_event = true;
                    match event {
                        Event::Press(i, j) => defmt::info!("Got KeyPress({=u8}, {=u8})", i, j),
                        Event::Release(i, j) => defmt::info!("Got KeyRelease({=u8}, {=u8})", i, j),
                    }
                    // Update pressed keys for the other half
                    self.pressed[self.keys.side().other()].update(&event.transform(|i, j| {
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
        if let Some(msg) = self.fsm.tick() {
            tx.lock(|tx| tx.push(msg.into()));
        }

        // Scan keys and push all events
        for event in self.keys.scan() {
            was_key_event = true;
            match self.fsm.role() {
                // Master should handle keyboard logic
                Role::Master => self.layout.event(event),
                // Slave should only send key events to master
                Role::Slave => {
                    let (i, j) = event.coord();
                    defmt::info!("Send Key({=u8}, {=u8})", i, j);
                    tx.lock(|tx| tx.push(event.into()));
                },
            }
        }

        // Update pressed keys state after scan
        self.pressed[*self.keys.side()] = self.keys.pressed();

        // Process USB wake up FIXME: assumes keyboard tick is 1 kHz
        usb.lock(|usb| usb.wake_up_update(was_key_event, 9));

        if self.fsm.role() == Role::Slave {
            // Slave just uses the LED update from master

            // Update state if we received it
            let state_update = leds_update.as_ref()
                .and_then(|update| update.state.as_ref())
                .and_then(|state| self.state.if_changed(state));

            // Send state if changed, others copied from received update
            LedsUpdate {
                state: state_update.cloned(),
                config: leds_update.as_ref().and_then(|u| u.config),
                brightness: leds_update.and_then(|u| u .brightness),
            }
        } else {
            // Master keeps track of the actual keyboard state

            let state = leds::KeyboardState {
                leds: keyboard_leds,
                usb_on: usb_state == UsbDeviceState::Configured,
                role: self.fsm.role(),
                layer: {
                    debug_assert!(self.layout.current_layer() <= u8::MAX as usize);
                    self.layout.current_layer() as u8
                },
                pressed: self.pressed.clone(),
            };

            // Collect state
            let mut update = LedsUpdate {
                state: self.state.if_changed(&state).cloned(),
                config: None,
                brightness: None,
            };

            // TODO: auto-enable NumLock by checking leds state
            // Advance keyboard time
            let custom = self.layout.tick();
            // self.keyboard_reports.push(self.layout.keycodes().collect());
            if let Some((action, pressed)) = custom.transposed() {
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
                        usb.lock(|usb| {
                            let bus = usb.dev.bus();
                            let dfu_boot = usb.dfu.ops_mut();
                            match fw {
                                actions::FirmwareAction::AllowBootloader => dfu_boot.set_allowed(true),
                                actions::FirmwareAction::JumpToBootloader => dfu_boot.reboot(true, Some(bus)),
                                actions::FirmwareAction::Reboot => dfu_boot.reboot(false, Some(bus)),
                            }
                        });
                    }
                };

            }

            // Advance mouse emulation time
            self.mouse.tick();

            // Advance usbd-human-interface-device keyboard time FIXME: assumes 1 kHz
            usb.lock(|usb| {
                let keyboard: &hid::KeyboardInterface<'_, _> = usb.hid.interface();
                keyboard.tick().ok();
            });

            // Push next report
            self.keyboard_reports.push(hid::KeyboardReport::new(self.layout.keycodes().as_page()));

            // Push USB reports
            if usb_state == UsbDeviceState::Configured {
                usb.lock(|usb| {
                    let keyboard: &hid::KeyboardInterface<'_, _> = usb.hid.interface();
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
                });
            } else {
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
            if update.any_change() {
                // defmt::info!("Send Leds(left={=u32}, right={=u32})",
                //     update.state.clone().unwrap().pressed.left.get_raw(),
                //     update.state.clone().unwrap().pressed.right.get_raw(),
                // );
                tx.lock(|tx| tx.push(update.clone().into()));
            }

            update
        }
    }

    /// Set new joystick reading values
    pub fn update_joystick(&mut self, xy: (i16, i16)) {
        self.mouse.update_joystick(xy);
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
    pub fn any_change(&self) -> bool {
         self.state.is_some() || self.config.is_some() || self.brightness.is_some()
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
