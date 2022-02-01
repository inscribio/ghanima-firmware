use crate::hal;

pub struct Crc {
    crc: hal::pac::CRC,
}

impl Crc {
    pub fn new(crc: hal::pac::CRC, _rcc: &mut hal::rcc::Rcc) -> Self {
        // Need to access `.regs` but it's private
        let rcc_regs = unsafe { &*hal::pac::RCC::ptr() };

        rcc_regs.ahbenr.modify(|_, w| w.crcen().enabled());

        // Use the defaults: poly=0x04c11db7, init=0xffffffff (not available in PAC anyway?)
        crc.cr.write(|w| {
            w
                .rev_out().normal()
                .rev_in().normal()
                .polysize().polysize32()
                .reset().reset()
        });

        Self { crc }
    }

    /// Calculate CRC value for given buffer of arbitrary length
    pub fn calculate(&mut self, buf: &[u8]) -> u32 {
        self.crc.cr.modify(|_, w| w.reset().reset());

        let mut chunks32 = buf.chunks_exact(4);
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

        self.crc.dr().read().bits()
    }
}
