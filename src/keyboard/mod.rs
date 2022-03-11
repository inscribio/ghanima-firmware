//! Main USB keyboard logic
//!
//! Implementation of split-keyboard logic based on the [`keyberon`] crate.
//! Contains firmware extensions such as communication between keyboard halves
//! and handling of custom events.

/// Special keyboard actions
pub mod actions;

mod event;
/// Keyboard matrix scanner with debouncing
mod keys;
/// Keyboard lightning control and configuration
pub mod leds;
/// Mouse emulation
pub mod mouse;
/// Role negotiation between keyboard halves
mod role;

use serde::{Serialize, Deserialize};
use keyberon::key_code::KbHidReport;
use keyberon::layout::{self, Event};
use usb_device::device::UsbDeviceState;

use crate::bsp::sides::BoardSide;
use crate::bsp::usb::Usb;
use crate::ioqueue;
use crate::hal_ext::crc::Crc;
use crate::utils::CircularIter;
use role::Role;
use leds::KeyboardState;
use actions::Action;
use event::CustomEventExt;

pub use keys::Keys;
pub use leds::Leds;

/// Transmitter of packets for communication between keyboard halves
pub type Transmitter<TX, const N: usize> = ioqueue::Transmitter<Message, TX, N>;
/// Receiver of packets for communication between keyboard halves
pub type Receiver<RX, const N: usize, const B: usize> = ioqueue::Receiver<Message, RX, N, B>;

/// Split keyboard logic
pub struct Keyboard {
    keys: keys::Keys,
    fsm: role::Fsm,
    layout: layout::Layout<Action>,
    mouse: mouse::Mouse,
}

/// Keyboard lightning control
pub struct KeyboardLeds {
    controller: leds::PatternController<'static>,
    configs: CircularIter<'static, leds::LedConfig>,
}

// TODO: use this in new()
// pub struct KeyboardConfig {
//     layers: layout::Layers<actions::Action>,
//     mouse: mouse::MouseConfig,
//     timeout: u32,
// }

/// Messages used in communication between keyboard halves
#[derive(Serialize, Deserialize, PartialEq)]
pub enum Message {
    /// Negotiation of roles of each half
    Role(role::Message),
    /// Raw key event transmitted to the half that is connected to USB from the other one
    #[serde(with = "EventDef")]
    Key(Event),
}

impl ioqueue::Packet for Message {
    type Checksum = Crc;
}

// Work around Event not implementing Serialize: https://serde.rs/remote-derive.html
#[derive(Serialize, Deserialize)]
#[serde(remote = "Event")]
enum EventDef {
    Press(u8, u8),
    Release(u8, u8),
}

impl Keyboard {
    /// Crate new keyboard with given layout and negotiation timeout specified in "ticks"
    /// (see [`Self::tick`])
    pub fn new(keys: keys::Keys, layout: layout::Layout<Action>, mouse: &'static mouse::MouseConfig, timeout_ticks: u32) -> Self {
        let side = *keys.side();
        Self {
            keys,
            fsm: role::Fsm::with(side, timeout_ticks),
            layout,
            mouse: mouse::Mouse::new(mouse)
        }
    }

    /// Get current role
    pub fn role(&self) -> Role {
        self.fsm.role()
    }

    /// Periodic keyboard events processing
    ///
    /// This should be called in a fixed period. Will handle communication between keyboard
    /// halves and resolve key events depending on keyboard layout. Requires information
    /// about current USB state (connected/not connected). Returns keyboard USB HID report
    /// with the keys that are currently pressed and [`KeyboardState`] for LED controller.
    pub fn tick<TX, RX>(&mut self, (tx, rx): (&mut TX, &mut RX), usb: &mut Usb<leds::KeyboardLedsState>) -> KeyboardState
    where
        TX: ioqueue::TransmitQueue<Message>,
        RX: ioqueue::ReceiveQueue<Message>,
    {
        let maybe_tx = |tx: &mut TX, msg: Option<role::Message>| {
            if let Some(msg) = msg {
                tx.push(Message::Role(msg));
            }
        };

        // First update USB state in FSM
        maybe_tx(tx, self.fsm.usb_state(usb.dev.state() == UsbDeviceState::Configured));

        // Process RX data
        while let Some(msg) = rx.get() {
            match msg {
                Message::Role(msg) => {
                    defmt::info!("Got role::Message: {}", msg);
                    maybe_tx(tx, self.fsm.on_rx(msg));
                },
                Message::Key(event) => {
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
                    tx.push(Message::Key(event));
                },
            }
        }

        // Only master should keep track of all the keyboard state
        if self.fsm.role() == Role::Master {
            // Advance keyboard time
            let custom = self.layout.tick();
            if let Some((action, pressed)) = custom.transposed() {
                self.handle_action(action, pressed);
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
        leds::KeyboardState {
            leds: *usb.keyboard_leds(),
            usb_on: usb.dev.state() == UsbDeviceState::Configured,
            role: self.fsm.role(),
            layer: self.layout.current_layer() as u8,
            pressed: self.keys.pressed(),
        }
    }

    pub fn update_joystick(&mut self, xy: (i16, i16)) {
        self.mouse.update_joystick(xy);
    }

    fn handle_action(&mut self, action: &Action, pressed: bool) {
        use actions::{LedAction};
        match action {
            Action::Led(led) => match led {
                LedAction::Cycle(_inc) => {
                    todo!()
                },
                LedAction::Brightness(_) => todo!(),
            },
            Action::Mouse(mouse) => self.mouse.handle_action(mouse, pressed),
        }
    }
}

impl KeyboardLeds {
    pub fn new(side: BoardSide, configs: leds::LedConfigurations) -> Self {
        let configs = CircularIter::new(configs);
        Self {
            controller: leds::PatternController::new(side, configs.current()),
            configs,
        }
    }

    /// Get the underlying pattern controller
    pub fn controller_mut(&mut self) -> &mut leds::PatternController<'static> {
        &mut self.controller
    }

    pub fn handle_action(&mut self, action: &actions::LedAction, press: bool) {
        // On release
        if !press {
            match action {
                actions::LedAction::Cycle(inc) => {
                    let new = inc.update(&mut self.configs);
                    self.controller.set_config(new);
                },
                actions::LedAction::Brightness(_) => todo!(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ioqueue::packet::PacketSer;

    fn verify_serialization(msg: Message, expected: &[u8]) {
        let mut buf = [0; 32];
        let mut checksum = Crc::new();
        let mut buf = msg.to_slice(&mut checksum, &mut buf[..]).unwrap();
        let len = postcard_cobs::decode_in_place(&mut buf).unwrap();
        assert_eq!(&buf[..len], expected);
    }

    #[test]
    fn message_ser_key_press() {
        verify_serialization(Message::Key(Event::Press(5, 6)),
            // Message::Key, Event::Press, i, j, crc16_L, crc16_H, sentinel
            &[0x01, 0x00, 5, 6, 0x82, 0x8a, 0x00]
        );
    }

    #[test]
    fn message_ser_key_release() {
        verify_serialization(Message::Key(Event::Release(7, 8)),
            &[0x01, 0x01, 7, 8, 0x53, 0xee, 0x00]
        );
    }

    #[test]
    fn message_ser_role_establish_master() {
        verify_serialization(Message::Role(role::Message::EstablishMaster),
            // Message::Key, role::Message::*, crc16_L, crc16_H, sentinel
            &[0x00, 0x00, 0x01, 0xb0, 0x00]
        );
    }

    #[test]
    fn message_ser_role_release_master() {
        verify_serialization(Message::Role(role::Message::ReleaseMaster),
            &[0x00, 0x01, 0xc0, 0x70, 0x00]
        );
    }

    #[test]
    fn message_ser_role_ack() {
        verify_serialization(Message::Role(role::Message::Ack),
            &[0x00, 0x02, 0x80, 0x71, 0x00]
        );
    }
}
