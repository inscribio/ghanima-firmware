mod keys;
mod role;

use core::convert::Infallible;

use serde::{Serialize, Deserialize};
use keyberon::key_code::KbHidReport;
use keyberon::layout::{self, Event};

use crate::io;
use crate::hal_ext::crc::Crc;
use role::Role;

pub use keys::Keys;

pub type Transmitter<TX, const N: usize> = io::Transmitter<Message, TX, N>;
pub type Receiver<RX, const N: usize, const B: usize> = io::Receiver<Message, RX, N, B>;

pub struct Keyboard<ACT: 'static = Infallible> {
    keys: keys::Keys,
    fsm: role::Fsm,
    layout: layout::Layout<ACT>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum Message {
    Role(role::Message),
    #[serde(with = "EventDef")]
    Key(Event),
}

impl io::Packet for Message {
    type Checksum = Crc;
}

// Work around Event not implementing Serialize: https://serde.rs/remote-derive.html
#[derive(Serialize, Deserialize)]
#[serde(remote = "Event")]
enum EventDef {
    Press(u8, u8),
    Release(u8, u8),
}

impl<ACT: 'static> Keyboard<ACT> {
    pub fn new(keys: keys::Keys, layout: layout::Layout<ACT>, timeout_ticks: u32) -> Self {
        let side = *keys.side();
        Self {
            keys,
            fsm: role::Fsm::with(side, timeout_ticks),
            layout
        }
    }

    pub fn tick<TX, RX>(&mut self, tx: &mut TX, rx: &mut RX, usb_on: bool) -> KbHidReport
        where
        TX: io::TransmitQueue<Message>,
        RX: io::ReceiveQueue<Message>,
    {
        let maybe_tx = |tx: &mut TX, msg: Option<role::Message>| {
            if let Some(msg) = msg {
                tx.push(Message::Role(msg));
            }
        };

        // First update USB state in FSM
        maybe_tx(tx, self.fsm.usb_state(usb_on));

        // Process RX data
        while let Some(msg) = rx.get() {
            match msg {
                Message::Role(msg) => {
                    defmt::info!("Got role::Message: {}", msg);
                    maybe_tx(tx, self.fsm.on_rx(msg));
                },
                Message::Key(event) => {
                    match &event {
                        &Event::Press(i, j) => defmt::info!("Got KeyPress({=u8}, {=u8})", i, j),
                        &Event::Release(i, j) => defmt::info!("Got KeyRelease({=u8}, {=u8})", i, j),
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

        // Advance keyboard time
        let custom = self.layout.tick();
        match custom {
            layout::CustomEvent::NoEvent => {},
            _ => todo!("Handle custom events"),
        }

        // Generate USB report
        // TODO: auto-enable NumLock by checking leds state
        self.layout.keycodes().collect()
    }
}
