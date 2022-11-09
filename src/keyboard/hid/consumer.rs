use usb_device::class_prelude::*;
use usbd_hid::{hid_class::{HIDClass, ReportType}, descriptor::generator_prelude::*};

/// MediaKeyboardReport describes a report and descriptor that can be used to
/// send consumer control commands to the host.
///
/// This is commonly used for sending media player for keyboards with media player
/// keys, but can be used for all sorts of Consumer Page functionality.
///
/// Reference: <https://usb.org/sites/default/files/hut1_2.pdf>
///
#[gen_hid_descriptor(
    (collection = APPLICATION, usage_page = CONSUMER, usage = CONSUMER_CONTROL) = {
        (usage_page = CONSUMER, usage_min = 0x00, usage_max = 0x514) = {
            #[item_settings data,array,absolute,not_null] usage_id=input;
        };
    }
)]
#[derive(Default, Eq, PartialEq)]
pub struct ConsumerReport {
    pub usage_id: u16,
}

pub struct HidConsumer<'a, B: UsbBus> {
    hid: HIDClass<'a, B>,
}

impl<'a, B: UsbBus> HidConsumer<'a, B> {
    pub fn new(alloc: &'a UsbBusAllocator<B>) -> Self {
        Self {
            hid: HIDClass::new_ep_in_with_settings(alloc, ConsumerReport::desc(), 100, Self::settings()),
        }
    }

    const fn settings() -> usbd_hid::hid_class::HidClassSettings {
        use usbd_hid::hid_class::*;
        HidClassSettings {
            subclass: HidSubClass::NoSubClass,
            protocol: HidProtocol::Keyboard,
            config: ProtocolModeConfig::DefaultBehavior,
            locale: HidCountryCode::NotSupported,
        }
    }
}

impl<'a, B: UsbBus> super::HidClass<'a, B> for HidConsumer<'a, B> {
    type Report = ConsumerReport;

    fn class(&mut self) -> &mut usbd_hid::hid_class::HIDClass<'a, B> {
        &mut self.hid
    }
}

/// HID Consumer Usage Page keys that can be used in [`ConsumerReport`]
// Copy of the original because that one doesn't implement Clone
#[non_exhaustive]
#[derive(Debug, Copy, Clone)]
pub enum ConsumerKey {
    Zero = 0x00,
    Play = 0xB0,
    Pause = 0xB1,
    Record = 0xB2,
    NextTrack = 0xB5,
    PrevTrack = 0xB6,
    Stop = 0xB7,
    RandomPlay = 0xB9,
    Repeat = 0xBC,
    PlayPause = 0xCD,
    Mute = 0xE2,
    VolumeIncrement = 0xE9,
    VolumeDecrement = 0xEA,
}

impl From<ConsumerKey> for ConsumerReport {
    fn from(key: ConsumerKey) -> Self {
        ConsumerReport { usage_id: key as u16 }
    }
}
