use ufmt::uwrite;
use pkg_version::{pkg_version_major, pkg_version_minor};
use static_assertions::const_assert;
use usb_device::UsbError;
use usb_device::bus::UsbBusAllocator;
use usb_device::device::{UsbDevice, UsbVidPid, UsbDeviceBuilder};
use usbd_dfu_rt::DfuRuntimeClass;
use usbd_microsoft_os::MsOsUsbClass;

use crate::hal::usb;
use crate::hal_ext::reboot;
use crate::keyboard::hid;
use super::sides::BoardSide;

pub use reboot::DfuBootloader;

type Bus = usb::UsbBusType;

/// USB resources and class implementations
pub struct Usb {
    pub dev: UsbDevice<'static, Bus>,
    pub hid: hid::HidClass<'static, Bus>,
    // this does not need to be share but it should be cleaner to have it here
    pub dfu: DfuRuntimeClass<reboot::DfuBootloader>,
    ms_os: MsOsUsbClass,
    wake_up_counter: u16,
    keyboard_leds: hid::KeyboardLeds,
}

pub struct UsbConfig<const N: usize> {
    pub bus: &'static UsbBusAllocator<Bus>,
    pub side: BoardSide,
    pub bootload_strict: bool,
    pub serial_num: &'static mut heapless::String<N>,
    pub device_id: Option<u16>,
}

/// Storage for serial number string, e.g. `1.10.100:65535`
pub const SERIAL_NUM_MAX_LEN: usize = 20;

impl Usb {
    pub fn new<const N: usize>(cfg: UsbConfig<N>) -> Self {
        // Classes
        let hid = hid::new_hid_class(cfg.bus);
        // NOTE: Create it last or else the device won't enumerate on Windows. It seems that Windows
        // does not like having DFU interface with number 0 and will report invalid configuration
        // descriptor.
        let dfu = usbd_dfu_rt::DfuRuntimeClass::new(cfg.bus, reboot::DfuBootloader::new(!cfg.bootload_strict));

        let ms_os = ms_os::class();

        // Device
        // TODO: follow guidelines from https://github.com/obdev/v-usb/blob/master/usbdrv/USB-IDs-for-free.txt
        // VID:PID recognised as Van Ooijen Technische Informatica:Keyboard
        let generic_keyboard = UsbVidPid(0x16c0, 0x27db);
        let serial_number = Self::format_serial_num(cfg.serial_num, cfg.device_id).unwrap();
        let dev = UsbDeviceBuilder::new(cfg.bus, generic_keyboard)
            .composite_with_iads()
            // From my measurements, with all LEDs set to constant white, the keyboard (both halves)
            // can draw up to 2 Amps, which is totally out of spec, but seems to work anyway.
            // With half brightness it is around 300 mA.
            .max_power(500)
            .supports_remote_wakeup(true)
            // Device info
            .manufacturer("inscrib.io")
            .product(match cfg.side {
                BoardSide::Left => "ghanima keyboard (L)",
                BoardSide::Right => "ghanima keyboard (R)"
            })
            .serial_number(serial_number)
            .device_release(Self::bcd_device())
            .build();

        Self { dev, hid, dfu, ms_os, wake_up_counter: 0, keyboard_leds: Default::default() }
    }

    /// Periodic USB poll
    pub fn poll(&mut self) -> bool {
        let mut got_data = self.dev.poll(&mut [
            &mut self.hid,
            &mut self.dfu,
            &mut self.ms_os,
        ]);

        if got_data {
            let keyboard: &hid::KeyboardInterface<'_, _> = self.hid.interface();
            match keyboard.read_report() {
                Err(UsbError::WouldBlock) => {},
                Err(_) => panic!("Keyboard read_report failed"),
                Ok(leds) => {
                    self.keyboard_leds = leds.into();
                    got_data = false;
                },
            }
        }

        got_data
    }

    pub fn keyboard_leds(&self) -> hid::KeyboardLeds {
        self.keyboard_leds
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

    fn format_serial_num<const N: usize>(s: &mut heapless::String<N>, device_id: Option<u16>) -> Result<&str, ()> {
        if let Some(id) = device_id {
            uwrite!(s, "{}:{}", env!("CARGO_PKG_VERSION"), id)?;
        } else {
            uwrite!(s, "{}:?", env!("CARGO_PKG_VERSION"))?;
        };
        Ok(s.as_str())
    }
}

mod ms_os {
    use usbd_microsoft_os::{os_20, MsOsUsbClass, WindowsVersion, utf16_lit, utf16_null_le_bytes};

    const DESCRIPTOR_SET: os_20::DescriptorSet = os_20::DescriptorSet {
        version: WindowsVersion::MINIMAL,
        features: &[],
        configurations: &[
            os_20::ConfigurationSubset {
                configuration: 0,
                features: &[],
                functions: &[
                    os_20::FunctionSubset {
                        first_interface: 3,
                        features: &[
                            os_20::FeatureDescriptor::CompatibleId {
                                id: b"WINUSB\0\0",
                                sub_id: b"\0\0\0\0\0\0\0\0",
                            },
                            os_20::FeatureDescriptor::RegistryProperty {
                                data_type: os_20::PropertyDataType::RegMutliSz,
                                name: &utf16_lit::utf16_null!("DeviceInterfaceGUIDs"),
                                data: &utf16_null_le_bytes!("{897d7b90-5aae-43e5-9c36-aa0f2fdbafc9}\0"),
                            },
                        ]
                    }
                ]
            }
        ],
    };

    const CAPABILITIES: os_20::Capabilities = os_20::Capabilities {
        infos: &[
            os_20::CapabilityInfo {
                descriptors: &DESCRIPTOR_SET,
                alt_enum_cmd: os_20::ALT_ENUM_CODE_NOT_SUPPORTED,
            }
        ],
    };

    const DESCRIPTOR_SET_BYTES: [u8; DESCRIPTOR_SET.size()] = DESCRIPTOR_SET.descriptor();
    const CAPABILITIES_BYTES: [u8; CAPABILITIES.data_len()] = CAPABILITIES.descriptor_data();

    pub const fn class() -> MsOsUsbClass {
        MsOsUsbClass {
            os_20_capabilities_data: &CAPABILITIES_BYTES,
            os_20_descriptor_sets: &[&DESCRIPTOR_SET_BYTES],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PKG_VER: &str = env!("CARGO_PKG_VERSION");

    #[test]
    fn format_serial_num_none() {
        let mut s = heapless::String::<SERIAL_NUM_MAX_LEN>::new();
        Usb::format_serial_num(&mut s, None).unwrap();
        assert_eq!(s.as_str(), format!("{PKG_VER}:?"));
    }

    #[test]
    fn format_serial_num_small() {
        let mut s = heapless::String::<SERIAL_NUM_MAX_LEN>::new();
        Usb::format_serial_num(&mut s, Some(42)).unwrap();
        assert_eq!(s.as_str(), format!("{PKG_VER}:42"));
    }

    #[test]
    fn format_serial_num_huge() {
        let mut s = heapless::String::<SERIAL_NUM_MAX_LEN>::new();
        Usb::format_serial_num(&mut s, Some(0xfffa)).unwrap();
        assert_eq!(s.as_str(), format!("{PKG_VER}:65530"));
    }

    #[test]
    fn format_serial_num_huge_pkgver() {
        // Check that SERIAL_NUM_MAX_LEN is enough
        let mut s = heapless::String::<SERIAL_NUM_MAX_LEN>::new();
        uwrite!(s, "999.999.999:65535").unwrap();
    }
}
