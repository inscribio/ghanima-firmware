use stm32f0xx_hal as hal;

use hal::usb;
use usb_device::device::UsbDevice;
use usbd_dfu_rt::DfuRuntimeClass;

pub struct Usb {
    pub dev: UsbDevice<'static, usb::UsbBusType>,
    pub serial: usbd_serial::SerialPort<'static, usb::UsbBusType>,
    // pub cdc: usbd_serial::CdcAcmClass<'static, usb::UsbBusType>,
    // pub keyboard: keyberon::Class<'static, usb::UsbBusType, ()>,
    // pub mouse: HIDClass<'static, usb::UsbBusType>,
    // this does not need to be share but it should be cleaner to have it here
    pub dfu: DfuRuntimeClass<crate::reboot::DfuBootloader>,
}

impl Usb {
    pub fn poll(&mut self) -> bool {
        // unsafe {
        //     if let Some(serial) = &crate::logger::SERIAL {
        //         let serial: &mut usbd_serial::SerialPort<'static, usb::UsbBusType> = &mut serial.borrow_mut();
        //         self.dev.poll(&mut [serial, &mut self.dfu])
        //     } else {
        //         false
        //     }
        // }
        self.dev.poll(&mut [&mut self.serial, &mut self.dfu])
        // self.dev.poll(&mut [&mut self.cdc, &mut self.dfu])
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
