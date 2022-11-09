use usb_device::bus::UsbBusAllocator;
use usb_device::device::{UsbDevice, UsbVidPid, UsbDeviceBuilder};
use usbd_dfu_rt::DfuRuntimeClass;
use usbd_hid::descriptor::{MouseReport, SerializedDescriptor};
use usbd_hid::hid_class::HIDClass;

use crate::hal::usb;
use crate::hal_ext::reboot;
use crate::keyboard::hid::{HidKeyboard, HidMouse, HidClass};
use super::sides::BoardSide;

type Bus = usb::UsbBusType;

/// USB resources and class implementations
pub struct Usb {
    pub dev: UsbDevice<'static, Bus>,
    pub keyboard: HidKeyboard<'static, Bus>,
    pub mouse: HidMouse<'static, Bus>,
    // this does not need to be share but it should be cleaner to have it here
    pub dfu: DfuRuntimeClass<reboot::DfuBootloader>,
}

impl Usb {
    pub fn new(bus: &'static UsbBusAllocator<Bus>, side: &BoardSide) -> Self {
        // Classes
        let keyboard = HidKeyboard::new(bus);
        let mouse = HidMouse::new(bus);
        // NOTE: Create it last or else the device won't enumerate on Windows. It seems that Windows
        // does not like having DFU interface with number 0 and will report invalid configuration
        // descriptor.
        let dfu = usbd_dfu_rt::DfuRuntimeClass::new(bus, reboot::DfuBootloader);

        // Device
        // TODO: follow guidelines from https://github.com/obdev/v-usb/blob/master/usbdrv/USB-IDs-for-free.txt
        // VID:PID recognised as Van Ooijen Technische Informatica:Keyboard
        let generic_keyboard = UsbVidPid(0x16c0, 0x27db);
        let dev = UsbDeviceBuilder::new(bus, generic_keyboard)
            .manufacturer("inscrib.io")
            .product(match side {
                BoardSide::Left => "ghanima keyboard (L)",
                BoardSide::Right => "ghanima keyboard (R)"
            })
            .serial_number(env!("CARGO_PKG_VERSION"))
            .composite_with_iads()
            .build();

        Self { dev, keyboard, mouse, dfu }
    }

    /// Periodic USB poll
    pub fn poll(&mut self) -> bool {
        self.dev.poll(&mut [self.keyboard.class(), self.mouse.class(), &mut self.dfu])
    }
}
