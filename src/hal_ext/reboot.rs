use core::mem::MaybeUninit;
use cortex_m::{peripheral::SCB, asm::bootload};
use usbd_dfu_rt::DfuRuntimeOps;

use crate::hal::{pac, usb};

const MAGIC_JUMP_BOOTLOADER: u32 = 0xdeadbeef;
const SYSTEM_MEMORY_BASE: u32 = 0x1fffc800;

#[link_section = ".uninit.MAGIC"]
static mut MAGIC: MaybeUninit<u32> = MaybeUninit::uninit();

/// Reboot the MCU
///
/// Triggers system reset. If `bootloader` is true, then a flag will be set
/// such that after reset, before any code execution we will jump to the embedded
/// MCU bootloader. Some USB hosts may have problems with enumerating the bootloader
/// after a reset. If `usb_bus` is passed, then USB reenumeration will be triggered
/// before system reset, which may prevent the issue.
pub unsafe fn reboot(bootloader: bool, usb_bus: Option<&usb::UsbBusType>) -> ! {
    if bootloader {
        MAGIC.as_mut_ptr().write(MAGIC_JUMP_BOOTLOADER);
    }
    if let Some(bus) = usb_bus {
        // Sometimes host fails to reenumerate our device when jumping to bootloader,
        // so we force reenumeration and only after that we do reset.
        bus.force_reenumeration(|| {
            SCB::sys_reset();
        });
        // not going any further, but not using if-else to satisfy return type
    }
    SCB::sys_reset()
}

#[cortex_m_rt::pre_init]
unsafe fn jump_bootloader() {
    // Verify that this was a software reset
    let software_reset = (*pac::RCC::ptr()).csr.read().sftrstf().bit_is_set();

    if software_reset && MAGIC.assume_init() == MAGIC_JUMP_BOOTLOADER {
        // reset the magic value not to jump again
        MAGIC.as_mut_ptr().write(0);
        // jump to bootloader located in System Memory
        bootload(SYSTEM_MEMORY_BASE as *const u32);
    }
}

/// Implements switching to USB DFU mode via rebooting to an embedded DFU bootloader
pub struct DfuBootloader {
    allow: bool,
}

impl DfuBootloader {
    pub fn new(allow: bool) -> Self {
        Self { allow }
    }

    pub fn set_allowed(&mut self, allowed: bool) {
        self.allow = allowed;
    }

    pub fn reboot(&mut self, bootloader: bool, usb_bus: Option<&usb::UsbBusType>) {
        unsafe { reboot(bootloader, usb_bus) }
    }
}

impl DfuRuntimeOps for DfuBootloader {
    fn detach(&mut self) {
        // I suspect this works without force_reenumeration because we actually reset
        // the system twice: once on sys_reset, then in jump_bootloader, but not sure.
        unsafe { reboot(true, None); }
    }

    fn allow(&mut self, timeout: u16) -> Option<u16> {
        if self.allow {
            Some(timeout)
        } else {
            None
        }
    }

    const WILL_DETACH: bool = false;
}
