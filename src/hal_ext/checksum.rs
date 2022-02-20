use num::PrimInt;
use postcard::flavors::SerFlavor;

/// Checksum generator
///
/// In principle this is similar to [`core::hash::Hasher`] but allows to use output
/// different than u64.
pub trait ChecksumGen {
    /// Checksum type that can be serialized (e.g. [`u32`])
    type Output: PrimInt;

    /// Reset internal state to start generating checksum for new data
    fn reset(&mut self);

    /// Push data from slice to the generator
    ///
    /// Note that with this method one must ensure correct state by calling
    /// [`Self::reset`] before any [`Self::push`] sequence followed by
    /// [`Self::get`].
    fn push(&mut self, data: &[u8]);

    /// Retrieve checksum generation result for all data since last [`Self::reset`]
    ///
    /// Note that wiht this method one must ensure correct state by calling
    /// [`Self::reset`] before any [`Self::push`] sequence followed by [`Self::get`].
    fn get(&self) -> Self::Output;

    /// Push `data` and retrive the final checksum
    fn decode(&mut self, data: &[u8]) -> Self::Output
    where
        Self: Sized
    {
        self.reset();
        self.push(data);
        self.get()
    }

    /// Number of bytes in the output checksum
    const LEN: usize = core::mem::size_of::<Self::Output>();

    /// Reinterpret checksum as little-endian byte slice
    #[inline(always)]
    fn as_le_bytes<'a>(checksum: &'a mut Self::Output) -> &'a [u8] {
        // Use little-endian byte order (same as postcard would use)
        *checksum = checksum.to_le();
        // NOTE(safety): reinterpreting primitive integer, we know it's byte size
        unsafe {
            // Convert Self::Output into u8 ptr
            let ptr = checksum as *const Self::Output;
            let ptr: *const u8 = core::mem::transmute(ptr);
            // Construct a byte slice
            core::slice::from_raw_parts(ptr, Self::LEN)
        }
    }

    /// Encode checksum of `buf[..data_len]` at the end of `buf`
    fn encode<'a>(&mut self, buf: &'a mut [u8], data_len: usize) -> Result<&'a [u8], Error>
    where
        Self: Sized
    {
        if buf.len() < data_len + Self::LEN {
            return Err(Error::BufTooShort);
        }
        let mut checksum = self.decode(&buf[..data_len]);
        buf[data_len..data_len + Self::LEN].copy_from_slice(Self::as_le_bytes(&mut checksum));
        Ok(&buf[..data_len + Self::LEN])
    }

    /// Verify that the data
    fn verify<'a>(&mut self, data: &'a [u8]) -> Result<&'a [u8], Error>
    where
        Self: Sized
    {
        if data.len() < Self::LEN {
            return Err(Error::BufTooShort);
        }
        let (data, checksum) = data.split_at(data.len() - Self::LEN);
        let mut computed = self.decode(data);
        if checksum == Self::as_le_bytes(&mut computed) {
            Ok(data)
        } else {
            Err(Error::ChecksumInvalid)
        }
    }
}

/// Checksum error
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    ChecksumInvalid,
    BufTooShort,
}

/// Encoder that appends checksum at the end of data
///
/// This is a postcard serialization flavor that uses some [`ChecksumGen`] encoder
/// to append checksum at the end of the data. Checksum bytes are appended in
/// **little-endian** order!
pub struct ChecksumEncoder<'a, F, C>
where
    F: SerFlavor,
    C: ChecksumGen,
{
    flavor: F,
    state: &'a mut C,
}

impl<'a, F, C> ChecksumEncoder<'a, F, C>
where
    F: SerFlavor,
    C: ChecksumGen,
{
    /// Initialize the encoder with clear state
    pub fn new(flavor: F, state: &'a mut C) -> Self {
        state.reset();
        Self { flavor, state }
    }
}

impl<'a, F, C> SerFlavor for ChecksumEncoder<'a, F, C>
where
    F: SerFlavor,
    C: ChecksumGen,
{
    type Output = <F as SerFlavor>::Output;

    fn try_push(&mut self, data: u8) -> Result<(), ()> {
        self.state.push(&[data]);
        self.flavor.try_push(data)
    }

    fn try_extend(&mut self, data: &[u8]) -> Result<(), ()> {
        self.state.push(data);
        self.flavor.try_extend(data)
    }

    fn release(mut self) -> Result<Self::Output, ()> {
        let mut checksum = self.state.get();
        self.flavor.try_extend(C::as_le_bytes(&mut checksum))?;
        self.flavor.release()
    }
}

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::vec::Vec;
    use crc::{Crc, CRC_32_MPEG_2};

    pub struct Crc32(Vec<u8>);

    impl Crc32 {
        pub fn new() -> Self {
            Self(Vec::new())
        }
    }

    impl ChecksumGen for Crc32 {
        type Output = u32;

        fn reset(&mut self) {
            self.0.clear();
        }

        fn push(&mut self, data: &[u8]) {
            self.0.extend_from_slice(data);
        }

        fn get(&self) -> Self::Output {
            let crc = Crc::<u32>::new(&CRC_32_MPEG_2);
            let mut digest = crc.digest();
            digest.update(&self.0);
            digest.finalize()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::mock::Crc32;

    const N: usize = 10;
    const DATA: [u8; N] = [0xa5, 0xa5, 0xa5, 0xa5, 0x1b, 0xad, 0xb0, 0x02, 0x0d, 0x15];
    const DATA_CRC: [u8; N + 4] = [
        0xa5, 0xa5, 0xa5, 0xa5, 0x1b, 0xad, 0xb0, 0x02, 0x0d, 0x15,
        0x49, 0xde, 0xb2, 0xe3,
    ];

    #[test]
    fn encode() {
        let mut buf = [0u8; 32];
        buf[..DATA.len()].copy_from_slice(&DATA);
        let buf = Crc32::new().encode(&mut buf, DATA.len()).unwrap();
        assert_eq!(buf, DATA_CRC);
    }

    #[test]
    fn verify() {
        assert_eq!(Crc32::new().verify(&DATA_CRC).unwrap(), DATA);
        let mut buf = [0u8; 32];
        let data = &mut buf[..DATA_CRC.len()];
        data.copy_from_slice(&DATA_CRC);
        data[0] ^= 0x10;
        assert_eq!(Crc32::new().verify(data).unwrap_err(), Error::ChecksumInvalid);
    }
}
