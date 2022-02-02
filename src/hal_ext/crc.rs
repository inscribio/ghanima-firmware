use crate::hal;

use postcard::flavors::SerFlavor;

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

pub enum Error {
    InvalidCrc,
    BufferTooShort,
}

pub struct CrcEncoder<'a, F: SerFlavor> {
    flavor: F,
    state: &'a mut Crc,
}

impl Crc {
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

    fn reset(&mut self) {
        self.crc.cr.modify(|_, w| w.reset().reset());
    }

    fn push(&mut self, data: u8) {
        self.crc.dr8().write(|w| w.dr8().bits(data));
    }

    fn get(&mut self) -> u32 {
        self.crc.dr().read().bits()
    }

    /// Calculate CRC value for given buffer of arbitrary length
    pub fn calculate(&mut self, buf: &[u8]) -> u32 {
        self.reset();

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

        self.get()
    }

    /// Encode CRC in the buffer
    ///
    /// This will use first `data_len` bytes in the buffer to calculate CRC and
    /// then store it at the end. Returns `Err` when there is not enough space
    /// in the buffer to encode CRC.
    pub fn encode(&mut self, buf: &mut [u8], data_len: usize) -> Result<(), Error> {
        if buf.len() < data_len + 4 {
            return Err(Error::BufferTooShort);
        }
        let crc = self.calculate(&buf[..data_len]);
        buf[data_len..data_len + 4].copy_from_slice(&crc.to_be_bytes());
        Ok(())
    }

    /// Verify CRC in the buffer
    ///
    /// Assumes that CRC is located at the end of the buffer and checks if it
    /// matches the rest of data. Returns the data slice without CRC part.
    pub fn verify<'a>(&mut self, buf: &'a [u8]) -> Result<&'a [u8], Error> {
        if buf.len() < 4 {
            return Err(Error::BufferTooShort);
        }
        let (data, crc) = buf.split_at(buf.len() - 4);
        if self.calculate(data) == u32::from_be_bytes(crc.try_into().unwrap()) {
            Ok(data)
        } else {
            Err(Error::InvalidCrc)
        }
    }
}

impl<'a, F: SerFlavor> CrcEncoder<'a, F> {
    pub fn new(flavor: F, state: &'a mut Crc) -> Self {
        state.reset();
        Self { flavor, state }
    }
}

impl<'a, F: SerFlavor> SerFlavor for CrcEncoder<'a, F> {
    type Output = <F as SerFlavor>::Output;

    fn try_push(&mut self, data: u8) -> Result<(), ()> {
        self.state.push(data);
        self.flavor.try_push(data)
    }

    fn release(mut self) -> Result<Self::Output, ()> {
        let crc = self.state.get();
        self.flavor.try_extend(&crc.to_be_bytes())?;
        self.flavor.release()
    }
}

pub fn test_serdes_cobs_crc(crc: &mut Crc) {
    use postcard::{serialize_with_flavor, flavors::{Slice, Cobs}};

    const N: usize = 10;

    let data: [u8; N] = [0xa5, 0xa5, 0xa5, 0xa5, 0x1b, 0xad, 0xb0, 0x02, 0x0d, 0x15];
    defmt::info!("Data: {=[u8]:02x}", data);

    {
        let mut buf = [0u8; 32];
        let ser = serialize_with_flavor::<[u8], Slice, &mut [u8]>(
            &data, Slice::new(&mut buf),
            ).unwrap();
        defmt::info!("SER: {=[u8]:02x}", ser);
    }

    {
        let mut buf = [0u8; 32];
        let ser = serialize_with_flavor::<[u8], CrcEncoder<Slice>, &mut [u8]>(
            &data, CrcEncoder::new(Slice::new(&mut buf), crc),
            ).unwrap();
        defmt::info!("SER + CRC: {=[u8]:02x}", ser);
    }

    let mut buf = [0u8; 32];
    let ser = serialize_with_flavor::<[u8], CrcEncoder<Cobs<Slice>>, &mut [u8]>(
        &data, CrcEncoder::new(Cobs::try_new(Slice::new(&mut buf)).unwrap(), crc),
        ).unwrap();
    defmt::info!("SER + CRC + COBS: {=[u8]:02x}", ser);

    // +1 for postcard serialization, +4 for CRC
    let des = postcard::from_bytes_cobs::<[u8; N + 1 + 4]>(ser).unwrap();
    defmt::info!("inv(COBS) {=[u8]:02x}", des);

    let des = crc.verify(&des);
    let result = match des {
        Ok(_) => "OK",
        Err(_) => "ERROR",
    };
    defmt::info!("inv(CRC + SER): {=[u8]:02x} [{=str}]", des.unwrap_or(&[]), result);
}
