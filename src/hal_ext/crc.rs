use crate::hal;
use super::checksum::ChecksumGen;

/// Wrapper around CRC peripheral
pub struct Crc {
    crc: hal::pac::CRC,
}

impl Crc {
    /// Configure CRC peripheral
    pub fn new(crc: hal::pac::CRC, _rcc: &mut hal::rcc::Rcc) -> Self {
        // Need to access `.regs` but it's private
        let rcc_regs = unsafe { &*hal::pac::RCC::ptr() };

        rcc_regs.ahbenr.modify(|_, w| w.crcen().enabled());

        let mut crc = Self { crc };
        crc.set_variant();
        crc.crc.cr.modify(|_, w| w.reset().reset());

        crc
    }

    // TODO: type safe variants, something like Crc<Crc32MPEG2>

    // // CRC-32-MPEG-2
    // fn set_variant(&mut self) {
    //     // Use the defaults: poly=0x04c11db7, init=0xffffffff (not available in PAC anyway?)
    //     self.crc.cr.write(|w| {
    //         w
    //             .rev_out().normal()
    //             .rev_in().normal()
    //             .polysize().polysize32()
    //     });
    // }

    // CRC-16-IBM / CRC-16-ANSI
    // Warning: works only on STM32F07x/STM32F09x
    fn set_variant(&mut self) {
        self.crc.cr.write(|w| {
            w
                .rev_out().reversed()
                .rev_in().byte()
                .polysize().polysize16()
        });
        // CRC polynomial register is not available in PAC?
        let poly_offset = 0x14;
        unsafe {
            let crc_poly = (hal::pac::CRC::ptr() as *const u8).add(poly_offset) as *mut u32;
            *crc_poly = 0x8005;
        }
    }
}

// 16-bit
impl ChecksumGen for Crc {
    type Output = u16;

    fn reset(&mut self) {
        self.crc.cr.modify(|_, w| w.reset().reset());
    }

    fn push(&mut self, data: &[u8]) {
        let mut chunks16 = data.chunks_exact(2);
        let tail = chunks16.remainder();

        // Feed most of the buffer as 16-bit values for faster calculation
        while let Some(chunk) = chunks16.next() {
            let hword = u16::from_be_bytes(chunk.try_into().unwrap());
            self.crc.dr16().write(|w| w.dr16().bits(hword));
        }

        // Process the remainder
        match tail.len() {
            0 => {},
            1 => self.crc.dr8().write(|w| w.dr8().bits(tail[0])),
            _ => unreachable!(),
        }
    }

    fn get(&self) -> Self::Output {
        self.crc.dr16().read().bits()
    }
}

// 32-bit
// impl ChecksumGen for Crc {
//     type Output = u32;
//
//     fn reset(&mut self) {
//         self.crc.cr.modify(|_, w| w.reset().reset());
//     }
//
//     fn push(&mut self, data: &[u8]) {
//         let mut chunks32 = data.chunks_exact(4);
//         let tail = chunks32.remainder();
//
//         // Feed most of the buffer as 32-bit values for faster calculation
//         while let Some(chunk) = chunks32.next() {
//             let word = u32::from_be_bytes(chunk.try_into().unwrap());
//             self.crc.dr().write(|w| w.dr().bits(word));
//         }
//
//         // Process the remainder
//         match tail.len() {
//             0 => {},
//             1 => self.crc.dr8().write(|w| w.dr8().bits(tail[0])),
//             2 => {
//                 let hword = u16::from_be_bytes(tail.try_into().unwrap());
//                 self.crc.dr16().write(|w| w.dr16().bits(hword));
//             },
//             3 => {
//                 let hword = u16::from_be_bytes(tail[..2].try_into().unwrap());
//                 self.crc.dr16().write(|w| w.dr16().bits(hword));
//                 self.crc.dr8().write(|w| w.dr8().bits(tail[2]));
//             },
//             _ => unreachable!(),
//         }
//     }
//
//     fn get(&self) -> Self::Output {
//         self.crc.dr().read().bits()
//     }
// }
