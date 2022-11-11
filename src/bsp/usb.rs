use pkg_version::{pkg_version_major, pkg_version_minor};
use static_assertions::const_assert;
use usb_device::bus::UsbBusAllocator;
use usb_device::device::{UsbDevice, UsbVidPid, UsbDeviceBuilder};
use usbd_dfu_rt::DfuRuntimeClass;

use crate::hal::usb;
use crate::hal_ext::reboot;
use crate::keyboard::hid::{HidKeyboard, HidMouse, HidConsumer, HidClass};
use super::sides::BoardSide;

type Bus = usb::UsbBusType;

/// USB resources and class implementations
pub struct Usb {
    pub dev: UsbDevice<'static, Bus>,
    pub keyboard: HidKeyboard<'static, Bus>,
    pub mouse: HidMouse<'static, Bus>,
    pub consumer: HidConsumer<'static, Bus>,
    // this does not need to be share but it should be cleaner to have it here
    pub dfu: DfuRuntimeClass<reboot::DfuBootloader>,
    wake_up_counter: u16,
}

impl Usb {
    pub fn new(bus: &'static UsbBusAllocator<Bus>, side: &BoardSide) -> Self {
        // Classes
        let keyboard = HidKeyboard::new(bus);
        let mouse = HidMouse::new(bus);
        let consumer = HidConsumer::new(bus);
        // NOTE: Create it last or else the device won't enumerate on Windows. It seems that Windows
        // does not like having DFU interface with number 0 and will report invalid configuration
        // descriptor.
        let dfu = usbd_dfu_rt::DfuRuntimeClass::new(bus, reboot::DfuBootloader);

        // Device
        // TODO: follow guidelines from https://github.com/obdev/v-usb/blob/master/usbdrv/USB-IDs-for-free.txt
        // VID:PID recognised as Van Ooijen Technische Informatica:Keyboard
        let generic_keyboard = UsbVidPid(0x16c0, 0x27db);
        let dev = UsbDeviceBuilder::new(bus, generic_keyboard)
            .composite_with_iads()
            // From my measurements, with all LEDs set to constant white, the keyboard (both halves)
            // can draw up to 2 Amps, which is totally out of spec, but seems to work anyway.
            // With half brightness it is around 300 mA.
            .max_power(500)
            .supports_remote_wakeup(true)
            // Device info
            .manufacturer("inscrib.io")
            .product(match side {
                BoardSide::Left => "ghanima keyboard (L)",
                BoardSide::Right => "ghanima keyboard (R)"
            })
            .serial_number(env!("CARGO_PKG_VERSION"))
            .device_release(Self::bcd_device())
            .build();

        Self { dev, keyboard, mouse, consumer, dfu, wake_up_counter: 0 }
    }

    /// Periodic USB poll
    pub fn poll(&mut self) -> bool {
        self.dev.poll(&mut [
            self.keyboard.class(),
            self.mouse.class(),
            self.consumer.class(),
            &mut self.dfu,
        ])
    }

    /// Set wake up state; call repeatedly, ticks should take 1-15 ms
    pub fn wake_up_update(&mut self, wake_up: bool, ticks: u16) {
        if wake_up && self.wake_up_counter == 0 {
            self.dev.bus().remote_wakeup(true);
            self.wake_up_counter = ticks;
        } else {
            self.wake_up_counter = self.wake_up_counter.saturating_sub(1);
            self.dev.bus().remote_wakeup(self.wake_up_counter != 0);
        }
    }

    const fn bcd_device() -> u16 {
        const_assert!(pkg_version_major!() < 0xff);
        const_assert!(pkg_version_minor!() < 0xff);
        let major: u16 = (pkg_version_major!() & 0xff) << 8;
        let minor: u16 = pkg_version_minor!() & 0xff;
        major | minor
    }
}
