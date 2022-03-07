use usb_device::bus::UsbBusAllocator;
use usb_device::device::{UsbDevice, UsbVidPid, UsbDeviceBuilder};
use usbd_dfu_rt::DfuRuntimeClass;

use crate::hal::usb;
use crate::hal_ext::reboot;
use super::sides::BoardSide;

type Bus = usb::UsbBusType;

/// USB resources and class implementations
pub struct Usb<L>
where
    L: keyberon::keyboard::Leds,
{
    pub dev: UsbDevice<'static, Bus>,
    pub serial: usbd_serial::SerialPort<'static, Bus>,
    // pub cdc: usbd_serial::CdcAcmClass<'static, Bus>,
    pub keyboard: keyberon::Class<'static, Bus, L>,
    // pub mouse: HIDClass<'static, Bus>,
    // this does not need to be share but it should be cleaner to have it here
    pub dfu: DfuRuntimeClass<reboot::DfuBootloader>,
}

impl<L> Usb<L>
where
    L: keyberon::keyboard::Leds,
{
    pub fn new(bus: &'static UsbBusAllocator<Bus>, side: &BoardSide, leds: L) -> Self {
        // Classes
        let serial = usbd_serial::SerialPort::new(bus);
        let dfu = usbd_dfu_rt::DfuRuntimeClass::new(bus, reboot::DfuBootloader);
        let keyboard = keyberon::new_class(bus, leds);

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

        Self { dev, serial, keyboard, dfu }
    }

    pub fn keyboard_leds(&mut self) -> &L {
        self.keyboard.device_mut().leds_mut()
    }

    /// Periodic USB poll
    pub fn poll(&mut self) -> bool {
        self.dev.poll(&mut [&mut self.keyboard, &mut self.serial, &mut self.dfu])
    }

    /// Perform CDC-ACM loopback. Useful for testing.
    pub fn serial_loopback(&mut self, transform: impl FnMut(&mut u8)) {
        let mut buf = [0u8; 64];

        match self.serial.read(&mut buf) {
            Ok(count) if count > 0 => {
                // toggle case
                buf[..count].iter_mut().for_each(transform);

                // send back
                let mut write_offset = 0;
                while write_offset < count {
                    match self.serial.write(&buf[write_offset..count]) {
                        Ok(len) if len > 0 => write_offset += len,
                        _ => {},
                    }
                }
            },
            _ => {},
        }
    }
}
