use core::mem::MaybeUninit;

use crate::hal::{self, usb};
use cortex_m::{peripheral::SCB, asm::bootload};
use usbd_dfu_rt::DfuRuntimeOps;

const MAGIC_JUMP_BOOTLOADER: u32 = 0xdeadbeef;
const SYSTEM_MEMORY_BASE: u32 = 0x1fffc800;

#[link_section = ".uninit.MAGIC"]
static mut MAGIC: MaybeUninit<u32> = MaybeUninit::uninit();

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
    if MAGIC.assume_init() == MAGIC_JUMP_BOOTLOADER {
        // reset the magic value not to jump again
        MAGIC.as_mut_ptr().write(0);
        // jump to bootloader located in System Memory
        bootload(SYSTEM_MEMORY_BASE as *const u32);
    }
}

pub struct DfuBootloader;

impl DfuRuntimeOps for DfuBootloader {
    fn enter(&mut self) {
        // I suspect this works without force_reenumeration because we actually reset
        // the system twice: once on sys_reset, then in jump_bootloader, but not sure.
        unsafe { reboot(true, None); }
    }
}
