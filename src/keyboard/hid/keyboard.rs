use bitfield::bitfield;
use keyberon::key_code::KeyCode;
use packed_struct::PackedStruct as _;
use serde::{Serialize, Deserialize};
use usbd_human_interface_device::device::keyboard::KeyboardLedsReport;

bitfield! {
    /// State of HID keyboard LEDs
    #[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
    pub struct KeyboardLeds(u8);
    pub num_lock, set_num_lock: 0;
    pub caps_lock, set_caps_lock: 1;
    pub scroll_lock, set_scroll_lock: 2;
    pub compose, set_compose: 3;
    pub kana, set_kana: 4;
}

impl From<KeyboardLedsReport> for KeyboardLeds {
    fn from(leds: KeyboardLedsReport) -> Self {
        let bytes: [u8; 1] = leds.pack().map_err(|_| ()).unwrap();
        KeyboardLeds(bytes[0])
    }
}

/// Key code iterator adapter from keyberon to usbd_human_interface_device
pub struct KeyboardIter<I>(I);

impl<I> Iterator for KeyboardIter<I>
    where I: Iterator<Item = KeyCode>
{
    type Item = usbd_human_interface_device::page::Keyboard;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
            .map(|kc| (kc as u8).into())
    }
}

/// Extension trait for adapting keyberon key code iterator to usbd_human_interface_device
pub trait KeyCodeIterExt: Sized {
    fn as_page(self) -> KeyboardIter<Self>;
}

impl<I> KeyCodeIterExt for I
    where I: Iterator<Item = KeyCode>
{
    fn as_page(self) -> KeyboardIter<Self> {
        KeyboardIter(self)
    }
}
