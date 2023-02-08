use serde::{Serialize, Deserialize};
use serde_big_array::BigArray;
use postcard::experimental::max_size::MaxSize;
use keyberon::layout::Event;

use crate::utils::max;
use crate::{hal_ext::crc::Crc, bsp::LedColors};
use crate::ioqueue;
use super::role;
use super::leds::Leds;

/// Messages used in communication between keyboard halves
#[derive(Serialize, Deserialize, PartialEq)]
pub enum Message {
    /// Negotiation of roles of each half
    Role(role::Message),
    /// Raw key event transmitted to the half that is connected to USB from the other one
    #[serde(with = "EventDef")]
    Key(Event),
    /// Send LED colors from half connected to USB to the other on
    #[serde(with = "BigArray")]
    Leds(LedColors),
}

// Work around Event not implementing Serialize: https://serde.rs/remote-derive.html
#[derive(Serialize, Deserialize, MaxSize)]
#[serde(remote = "Event")]
enum EventDef {
    Press(u8, u8),
    Release(u8, u8),
}

// Manual implementation on the whole enum because we have foreign types in variants
// that don't implement MaxSize so we cannot even implement it for them.
impl MaxSize for Message {
    const POSTCARD_MAX_SIZE: usize = 1 + max(
        max(role::Message::POSTCARD_MAX_SIZE, EventDef::POSTCARD_MAX_SIZE),
        3 * 28,
    );
}

impl ioqueue::Packet for Message {
    type Checksum = Crc;
}

impl From<role::Message> for Message {
    fn from(msg: role::Message) -> Self {
        Message::Role(msg)
    }
}

impl From<Event> for Message {
    fn from(event: Event) -> Self {
        Message::Key(event)
    }
}

impl From<LedColors> for Message {
    fn from(colors: LedColors) -> Self {
        Message::Leds(colors)
    }
}

impl From<&Leds> for Message {
    fn from(leds: &Leds) -> Self {
        Message::Leds(leds.colors)
    }
}

#[cfg(test)]
mod tests {
    use rgb::RGB8;

    use super::*;
    use crate::ioqueue::packet::PacketSer;

    #[test]
    fn message_max_size() {
        let msgs = [
            Message::Role(role::Message::EstablishMaster),
            Message::Role(role::Message::ReleaseMaster),
            Message::Role(role::Message::Ack),
            Message::Key(Event::Press(10, 11)),
            Message::Key(Event::Release(10, 11)),
            Message::Leds(LedColors::default()),
        ];
        let mut buf = [0; 256];

        for msg in msgs {
            let len = postcard::to_slice(&msg, &mut buf).unwrap().len();
            assert!(len <= Message::POSTCARD_MAX_SIZE);
        }
    }

    fn verify_serialization(msg: Message, expected: &[u8]) {
        let mut buf = [0; 89];
        let mut checksum = Crc::new_mock();
        let mut buf = msg.to_slice(&mut checksum, &mut buf[..]).unwrap();
        let len = cobs::decode_in_place(&mut buf).unwrap();
        assert_eq!(&buf[..len], expected);
    }

    #[test]
    fn message_ser_key_press() {
        verify_serialization(Message::Key(Event::Press(5, 6)),
            // Message::Key, Event::Press, i, j, crc16_L, crc16_H, sentinel
            &[0x01, 0x00, 5, 6, 0x82, 0x8a]
        );
    }

    #[test]
    fn message_ser_key_release() {
        verify_serialization(Message::Key(Event::Release(7, 8)),
            &[0x01, 0x01, 7, 8, 0x53, 0xee]
        );
    }

    #[test]
    fn message_ser_role_establish_master() {
        verify_serialization(Message::Role(role::Message::EstablishMaster),
            // Message::Key, role::Message::*, crc16_L, crc16_H, sentinel
            &[0x00, 0x00, 0x01, 0xb0]
        );
    }

    #[test]
    fn message_ser_role_release_master() {
        verify_serialization(Message::Role(role::Message::ReleaseMaster),
            &[0x00, 0x01, 0xc0, 0x70]
        );
    }

    #[test]
    fn message_ser_role_ack() {
        verify_serialization(Message::Role(role::Message::Ack),
            &[0x00, 0x02, 0x80, 0x71]
        );
    }

    #[test]
    fn message_leds_update() {
        let msg = Message::Leds([
            RGB8::new( 0,  1,  2),
            RGB8::new( 3,  4,  5),
            RGB8::new( 6,  7,  8),
            RGB8::new( 9, 10, 11),
            RGB8::new(12, 13, 14),
            RGB8::new(15, 16, 17),
            RGB8::new(18, 19, 20),
            RGB8::new(21, 22, 23),
            RGB8::new(24, 25, 26),
            RGB8::new(27, 28, 29),
            RGB8::new(30, 31, 32),
            RGB8::new(33, 34, 35),
            RGB8::new(36, 37, 38),
            RGB8::new(39, 40, 41),
            RGB8::new(42, 43, 44),
            RGB8::new(45, 46, 47),
            RGB8::new(48, 49, 50),
            RGB8::new(51, 52, 53),
            RGB8::new(54, 55, 56),
            RGB8::new(57, 58, 59),
            RGB8::new(60, 61, 62),
            RGB8::new(63, 64, 65),
            RGB8::new(66, 67, 68),
            RGB8::new(69, 70, 71),
            RGB8::new(72, 73, 74),
            RGB8::new(75, 76, 77),
            RGB8::new(78, 79, 80),
            RGB8::new(81, 82, 83),
        ]);
        verify_serialization(msg, &[
            0x02,
            // LED colors: r0, g0, b0, r1, g1, b1, ...
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
            23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43,
            44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
            65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83,
            // crc16_L, crc16_H
            0xda, 0x88,
        ]);
    }
}
