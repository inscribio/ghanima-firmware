//! Main USB keyboard logic
//!
//! Implementation of split-keyboard logic based on the [`keyberon`] crate.
//! Contains firmware extensions such as communication between keyboard halves
//! and handling of custom events.

/// Special keyboard actions
pub mod actions;
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

use keyberon::key_code::KbHidReport;
use keyberon::layout::{self, Event};
use usb_device::device::UsbDeviceState;

use crate::bsp::usb::Usb;
use crate::ioqueue;
use crate::utils::CircularIter;
use role::Role;
use leds::KeyboardState;
use actions::Action;
use keyberon::layout::CustomEvent;

pub use keys::Keys;
pub use leds::LedController;

/// Transmitter of packets for communication between keyboard halves
pub type Transmitter<TX, const N: usize> = ioqueue::Transmitter<msg::Message, TX, N>;
/// Receiver of packets for communication between keyboard halves
pub type Receiver<RX, const N: usize, const B: usize> = ioqueue::Receiver<msg::Message, RX, N, B>;

/// Split keyboard logic
pub struct Keyboard {
    keys: keys::Keys,
    fsm: role::Fsm,
    layout: layout::Layout<Action>,
    mouse: mouse::Mouse,
    led_configs: CircularIter<'static, leds::LedConfig>,
}

/// Keyboard configuration
pub struct KeyboardConfig {
    /// Keyboard layers configuration
    pub layers: layout::Layers<actions::Action>,
    /// Configuration of mouse emulation
    pub mouse: &'static mouse::MouseConfig,
    /// Configuration of RGB LED lightning
    pub leds: leds::LedConfigurations,
    /// Timeout for polling the other half about role negotiation
    pub timeout: u32,
}

/// Deferred update of LED controller state
pub struct LedsUpdate {
    state: KeyboardState,
    new_config: Option<&'static leds::LedConfig>,
    new_brightness: Option<u8>,
}

impl Keyboard {
    /// Crate new keyboard with given layout and negotiation timeout specified in "ticks"
    /// (see [`Self::tick`])
    pub fn new(keys: keys::Keys, config: &KeyboardConfig) -> (Self, LedController) {
        let side = *keys.side();
        let led_configs = CircularIter::new(config.leds);
        let leds = LedController::new(side, led_configs.current());
        let keyboard = Self {
            keys,
            fsm: role::Fsm::with(side, config.timeout),
            layout: layout::Layout::new(config.layers),
            mouse: mouse::Mouse::new(config.mouse),
            led_configs,
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
    pub fn tick<TX, RX>(&mut self, (tx, rx): (&mut TX, &mut RX), usb: &mut Usb<leds::KeyboardLedsState>) -> LedsUpdate
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
                    // Only master cares for key presses from the other half
                    if self.fsm.role() == Role::Master {
                        self.layout.event(event);
                    }
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

        let mut led_config = None;

        // Only master should keep track of all the keyboard state
        if self.fsm.role() == Role::Master {
            // Advance keyboard time
            let custom = self.layout.tick();
            if let Some((action, pressed)) = custom.transposed() {
                led_config = self.handle_action(action, pressed);
            }

            // Advance mouse emulation time
            self.mouse.tick();

            // Push USB reports
            if self.fsm.role() == Role::Master && usb.dev.state() == UsbDeviceState::Configured {
                // TODO: auto-enable NumLock by checking leds state
                let kb_report: KbHidReport = self.layout.keycodes().collect();
                let modified = usb.keyboard.device_mut().set_keyboard_report(kb_report.clone());
                // Only write to the endpoint if report has changed
                if modified {
                    // Keyboard HID report is just a set of keys being pressed, so just ignore this
                    // report if we were not able to push it because the last one hasn't been read yet.
                    // If USB host polls so rarely then there's no point in queueing anything, USB host
                    // will just miss some keys (seems unlikely as a key would have to be pressed for a
                    // very short time).
                    // TODO: we could add a small queue to debounce USB hosts with unpredictable lags
                    usb.keyboard.write(kb_report.as_bytes())
                        .expect("Bug in class implementation");
                }

                // Try to push USB mouse report
                self.mouse.push_report(&usb.mouse);
            }
        }

        // Collect keyboard state
        // TODO: send LED commands to second half
        let state = leds::KeyboardState {
            leds: *usb.keyboard_leds(),
            usb_on: usb.dev.state() == UsbDeviceState::Configured,
            role: self.fsm.role(),
            layer: self.layout.current_layer() as u8,
            pressed: self.keys.pressed(),
        };

        LedsUpdate {
            state,
            new_config: led_config,
            new_brightness: None
        }
    }

    /// Set new joystick reading values
    pub fn update_joystick(&mut self, xy: (i16, i16)) {
        self.mouse.update_joystick(xy);
    }

    fn handle_action(&mut self, action: &Action, pressed: bool) -> Option<&'static leds::LedConfig> {
        use actions::LedAction;
        match action {
            Action::Led(led) => if !pressed {  // only on release
                match led {
                    LedAction::Cycle(inc) => return Some(inc.update(&mut self.led_configs)),
                    LedAction::Brightness(_) => todo!(),
                }
            },
            Action::Mouse(mouse) => self.mouse.handle_action(mouse, pressed),
        };
        None
    }
}

impl LedsUpdate {
    /// Perform LED controller update
    pub fn apply(self, time: u32, leds: &mut LedController) {
        if let Some(config) = self.new_config {
            leds.set_config(config);
        }
        // if let Some(brightness) = self.new_brightness {
        //     leds.set_brightness(brightness);
        // }
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
