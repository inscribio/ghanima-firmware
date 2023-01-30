use serde::{Serialize, Deserialize};
use keyberon::layout::Event;

use crate::hal_ext::crc::Crc;
use crate::ioqueue;
use super::{role, LedsUpdate};

/// Messages used in communication between keyboard halves
#[derive(Serialize, Deserialize, PartialEq)]
pub enum Message {
    /// Negotiation of roles of each half
    Role(role::Message),
    /// Raw key event transmitted to the half that is connected to USB from the other one
    #[serde(with = "EventDef")]
    Key(Event),
    /// Update LEDs state
    Leds(LedsUpdate),
}

// Work around Event not implementing Serialize: https://serde.rs/remote-derive.html
#[derive(Serialize, Deserialize)]
#[serde(remote = "Event")]
enum EventDef {
    Press(u8, u8),
    Release(u8, u8),
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

impl From<LedsUpdate> for Message {
    fn from(update: LedsUpdate) -> Self {
        Message::Leds(update)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bsp::sides::PerSide;
    use crate::keyboard::BrightnessUpdate;
    use crate::keyboard::keys::PressedLedKeys;
    use crate::keyboard::leds::KeyboardState;
    use crate::keyboard::hid::KeyboardLeds;
    use crate::ioqueue::packet::PacketSer;

    fn verify_serialization(msg: Message, expected: &[u8]) {
        let mut buf = [0; 32];
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
    fn verify_leds_update() {
        verify_serialization(Message::Leds(LedsUpdate {
            state: Some(KeyboardState {
                leds: KeyboardLeds(0b01010),
                usb_on: true,
                role: role::Role::Master,
                layer: 2,
                pressed: PerSide {
                    left: PressedLedKeys::new_raw(0b0000_0000000000000000000000011001),
                    right: PressedLedKeys::new_raw(0b00001100000000000000000000000011),
                },
            }),
            config: None,
            brightness: Some(BrightnessUpdate::Down),
        }),
            // Message::Leds
            &[0x02,
                // option Some
                1,
                // leds, usb_on, role, layer
                0b01010, 1, 0, 2,
                // pressed_left (varint(u32))
                0b00011001,
                // pressed_right (varint(u32))
                0b1_0000011, 0b1_0000000, 0b1_0000000, 0b01100000,
                // config
                0x00,
                // brightness
                0x01, 0x01,
                // crc16_L, crc16_H
                0xb2, 0xcd,
            ]
        );
    }
}
