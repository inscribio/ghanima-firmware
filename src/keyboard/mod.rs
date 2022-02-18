use core::convert::Infallible;

use serde::{Serialize, Deserialize};
use keyberon::key_code::KbHidReport;
use keyberon::layout::{self, Event};

use crate::io;
use crate::hal_ext::crc::Crc;

pub use keys::Keys;
pub use fsm::Fsm;

pub mod keys;
pub mod fsm;

pub type Transmitter<TX, const N: usize> = io::Transmitter<Message, TX, N>;
pub type Receiver<RX, const N: usize, const B: usize> = io::Receiver<Message, RX, N, B>;

pub struct Keyboard<TX, RX, ACT: 'static = Infallible> {
    pub keys: keys::Keys,
    pub fsm: fsm::Fsm<TX, RX>,
    pub layout: layout::Layout<ACT>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum Message {
    EstablishMaster,
    ReleaseMaster,
    Ack,
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

impl<TX, RX, ACT> Keyboard<TX, RX, ACT>
where
    TX: io::TransmitQueue<Message>,
    RX: io::ReceiveQueue<Message>,
    ACT: 'static
{
    pub fn tick(&mut self, tx: &mut TX, rx: &mut RX, time: u32) -> KbHidReport {
        // Advance IO FSM, may push events to layout
        self.fsm.tick(tx, rx, &mut self.layout, time);

        // Scan keys and push all events
        for event in self.keys.scan() {
            if self.fsm.should_handle_events() {
                // Master should handle keyboard logic
                self.layout.event(event)
            } else {
                let (i, j) = event.coord();
                defmt::info!("Send Key({=u8}, {=u8})", i, j);
                // Slave should only send key events to master
                tx.push(Message::Key(event));
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
