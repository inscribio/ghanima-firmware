use usb_device::class_prelude::*;
use usbd_hid::{hid_class::{HIDClass, ReportType}, descriptor::generator_prelude::*};

/// MouseReport describes a report and its companion descriptor than can be used
/// to send mouse movements and button presses to a host.
#[gen_hid_descriptor(
    (collection = APPLICATION, usage_page = GENERIC_DESKTOP, usage = MOUSE) = {
        (collection = PHYSICAL, usage = POINTER) = {
            (usage_page = BUTTON, usage_min = BUTTON_1, usage_max = BUTTON_8) = {
                #[packed_bits 8] #[item_settings data,variable,absolute] buttons=input;
            };
            (usage_page = GENERIC_DESKTOP,) = {
                (usage = X,) = {
                    #[item_settings data,variable,relative] x=input;
                };
                (usage = Y,) = {
                    #[item_settings data,variable,relative] y=input;
                };
                (usage = WHEEL,) = {
                    #[item_settings data,variable,relative] wheel=input;
                };
            };
            (usage_page = CONSUMER,) = {
                (usage = AC_PAN,) = {
                    #[item_settings data,variable,relative] pan=input;
                };
            };
        };
    }
)]
#[derive(Default, Eq, PartialEq)]
pub struct MouseReport {
    pub buttons: u8,
    pub x: i8,
    pub y: i8,
    pub wheel: i8, // Scroll down (negative) or up (positive) this many units
    pub pan: i8,   // Scroll left (negative) or right (positive) this many units
}

pub struct HidMouse<'a, B: UsbBus> {
    hid: HIDClass<'a, B>,
}

impl<'a, B: UsbBus> HidMouse<'a, B> {
    pub fn new(alloc: &'a UsbBusAllocator<B>) -> Self {
        Self {
            hid: HIDClass::new_ep_in_with_settings(alloc, MouseReport::desc(), 10, Self::settings()),
        }
    }

    const fn settings() -> usbd_hid::hid_class::HidClassSettings {
        use usbd_hid::hid_class::*;
        HidClassSettings {
            subclass: HidSubClass::NoSubClass,
            protocol: HidProtocol::Mouse,
            config: ProtocolModeConfig::DefaultBehavior,
            locale: HidCountryCode::NotSupported,
        }
    }
}

impl<'a, B: UsbBus> super::HidClass<'a, B> for HidMouse<'a, B> {
    type Report = MouseReport;

    fn class(&mut self) -> &mut usbd_hid::hid_class::HIDClass<'a, B> {
        &mut self.hid
    }
}
