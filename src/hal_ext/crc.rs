use postcard::flavors::SerFlavor;

use crate::hal;
use super::checksum::ChecksumGen;

/// Wrapper around CRC peripheral
pub struct Crc {
    crc: hal::pac::CRC,
    #[allow(dead_code)]
    variant: Variant,
}

pub enum Variant {
    // TODO: Other variants, or some way to use CRC16, as CRC32 is too much for short serial messages
    Crc32MPEG2,  // CRC-32-MPEG-2
    // Crc16IBM,  // CRC-16-IBM / CRC-16-ANSI
}

impl Crc {
    /// Configure CRC peripheral
    pub fn new(crc: hal::pac::CRC, _rcc: &mut hal::rcc::Rcc, variant: Variant) -> Self {
        // Need to access `.regs` but it's private
        let rcc_regs = unsafe { &*hal::pac::RCC::ptr() };

        rcc_regs.ahbenr.modify(|_, w| w.crcen().enabled());

        match variant {
            Variant::Crc32MPEG2 => {
                // Use the defaults: poly=0x04c11db7, init=0xffffffff (not available in PAC anyway?)
                crc.cr.write(|w| {
                    w
                        .rev_out().normal()
                        .rev_in().normal()
                        .polysize().polysize32()
                        .reset().reset()
                });
            },
        }

        Self { crc, variant }
    }
}

impl ChecksumGen for Crc {
    type Output = u32;

    fn reset(&mut self) {
        self.crc.cr.modify(|_, w| w.reset().reset());
    }

    fn push(&mut self, data: &[u8]) {
        let mut chunks32 = data.chunks_exact(4);
        let tail = chunks32.remainder();

        // Feed most of the buffer as 32-bit values for faster calculation
        while let Some(chunk) = chunks32.next() {
            let word = u32::from_be_bytes(chunk.try_into().unwrap());
            self.crc.dr().write(|w| w.dr().bits(word));
        }

        // Process the remainder
        match tail.len() {
            0 => {},
            1 => self.crc.dr8().write(|w| w.dr8().bits(tail[0])),
            2 => {
                let hword = u16::from_be_bytes(tail.try_into().unwrap());
                self.crc.dr16().write(|w| w.dr16().bits(hword));
            },
            3 => {
                let hword = u16::from_be_bytes(tail[..2].try_into().unwrap());
                self.crc.dr16().write(|w| w.dr16().bits(hword));
                self.crc.dr8().write(|w| w.dr8().bits(tail[2]));
            },
            _ => unreachable!(),
        }
    }

    fn get(&self) -> Self::Output {
        self.crc.dr().read().bits()
    }
}
