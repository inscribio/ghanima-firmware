use postcard::flavors::{SerFlavor, Cobs, Slice, HVec};
use heapless::Vec;

/// Checksum generator
///
/// In principle this is similar to `core::hash::Hasher` but allows to use output
/// different than u64.
pub trait ChecksumGen {
    /// Checksum type that can be serialized (e.g. `u32`)
    type Output: AsBytes;

    const LEN: usize = <Self::Output as AsBytes>::N;

    /// Push data from slice to the generator
    fn push(&mut self, data: &[u8]);

    /// Finalize checksum generation and retrieve the result
    fn get(self) -> Self::Output;

    /// Push `data` and retrive the final checksum
    fn decode(mut self, data: &[u8]) -> Self::Output
    where
        Self: Sized
    {
        self.push(data);
        self.get()
    }

    /// Encode checksum of `buf[..data_len]` at the end of `buf`
    fn encode(self, buf: &mut [u8], data_len: usize) -> Result<(), Error>
    where
        Self: Sized
    {
        if buf.len() < data_len + Self::LEN {
            return Err(Error::BufTooShort);
        }
        let checksum = self.decode(&buf[..data_len]).as_be_bytes();
        buf[data_len..data_len + Self::LEN].copy_from_slice(checksum.as_ref());
        Ok(())
    }

    /// Verify that the
    fn verify<'a>(self, data: &'a [u8]) -> Result<&'a [u8], Error>
    where
        Self: Sized
    {
        if data.len() < Self::LEN {
            return Err(Error::BufTooShort);
        }
        let (data, checksum) = data.split_at(data.len() - Self::LEN);
        if checksum == self.decode(data).as_be_bytes().as_ref() {
            Ok(data)
        } else {
            Err(Error::ChecksumInvalid)
        }
    }

    /// Serialize an object to slice, appending checksum
    fn serialize_to_slice<'a, 'b, T>(
        self,
        value: &'b T,
        buf: &'a mut [u8],
    ) -> postcard::Result<&'a mut [u8]>
    where
        T: serde::Serialize + ?Sized,
        Self: Sized,
    {
        postcard::serialize_with_flavor::<T, ChecksumEncoder<Slice, Self>, &'a mut [u8]>(
            value,
            ChecksumEncoder::new(Slice::new(buf), self),
        )
    }

    /// Serialize to slice, appending checksum and then encoding all data with COBS
    fn serialize_to_slice_cobs<'a, 'b, T>(
        self,
        value: &'b T,
        buf: &'a mut [u8],
    ) -> postcard::Result<&'a mut [u8]>
    where
        T: serde::Serialize + ?Sized,
        Self: Sized,
    {
        // Note: outer-most type is performing its encoding first
        postcard::serialize_with_flavor::<T, ChecksumEncoder<Cobs<Slice>, Self>, &'a mut [u8]>(
            value,
            ChecksumEncoder::new(Cobs::try_new(Slice::new(buf))?, self),
        )
    }

    /// Serialize an object to heapless::Vec, appending checksum
    fn serialize_to_vec<T, const N: usize>(self, value: &T) -> postcard::Result<Vec<u8, N>>
    where
        T: serde::Serialize + ?Sized,
        Self: Sized,
    {
        postcard::serialize_with_flavor::<T, ChecksumEncoder<HVec<N>, Self>, Vec<u8, N>>(
            value,
            ChecksumEncoder::new(HVec::default(), self),
        )
    }

    /// Serialize an object to heapless::Vec, appending checksum, then encoding in COBS
    fn serialize_to_vec_cobs<T, const N: usize>(self, value: &T) -> postcard::Result<Vec<u8, N>>
    where
        T: serde::Serialize + ?Sized,
        Self: Sized,
    {
        postcard::serialize_with_flavor::<T, ChecksumEncoder<Cobs<HVec<N>>, Self>, Vec<u8, N>>(
            value,
            ChecksumEncoder::new(Cobs::try_new(HVec::default())?, self),
        )
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
/// This is a postcard serialization flavor that uses some `Checksum` encoder
/// to append checksum at the end of the data.
pub struct ChecksumEncoder<F, C>
where
    F: SerFlavor,
    C: ChecksumGen,
{
    flavor: F,
    state: C,
}

impl<F, C> ChecksumEncoder<F, C>
where
    F: SerFlavor,
    C: ChecksumGen,
{
    pub fn new(flavor: F, state: C) -> Self {
        Self { flavor, state }
    }
}

impl<F, C> SerFlavor for ChecksumEncoder<F, C>
where
    F: SerFlavor,
    C: ChecksumGen,
{
    type Output = <F as SerFlavor>::Output;

    fn try_push(&mut self, data: u8) -> Result<(), ()> {
        self.state.push(&[data]);
        self.flavor.try_push(data)
    }

    fn release(mut self) -> Result<Self::Output, ()> {
        let checksum = self.state.get().as_be_bytes();
        self.flavor.try_extend(checksum.as_ref())?;
        self.flavor.release()
    }
}

/// Integer type that can be converted to big-endian bytes
///
/// This is a helper trait implemented for all unsigned integers.
pub trait AsBytes {
    type Bytes: AsRef<[u8]>;

    const N: usize = core::mem::size_of::<Self::Bytes>();

    fn as_be_bytes(self) -> Self::Bytes;
}

macro_rules! impl_as_bytes {
    ($($int:ty: $n:literal),+ $(,)?) => {
        $(
            impl AsBytes for $int {
                type Bytes = [u8; $n];

                fn as_be_bytes(self) -> Self::Bytes {
                    self.to_be_bytes()
                }
            }
        )+
    }
}

impl_as_bytes!(u8: 1, u16: 2, u32: 4, u64: 8);

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

        fn push(&mut self, data: &[u8]) {
            self.0.extend_from_slice(data);
        }

        fn get(self) -> Self::Output {
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
        0xa5, 0xa5, 0xa5, 0xa5, 0x1b, 0xad, 0xb0, 0x02, 0x0d, 0x15, 0xe3, 0xb2, 0xde, 0x49
    ];
    const DATA_CRC_COBS: [u8; N + 4 + 2] = [
        15, 0xa5, 0xa5, 0xa5, 0xa5, 0x1b, 0xad, 0xb0, 0x02, 0x0d, 0x15, 0xe3, 0xb2, 0xde, 0x49, 0
    ];

    #[test]
    fn serialize_to_slice() {
        let mut buf = [0u8; 32];
        let buf = Crc32::new().serialize_to_slice(&DATA, &mut buf).unwrap();
        assert_eq!(buf, DATA_CRC);
    }

    #[test]
    fn serialize_to_slice_cobs() {
        let mut buf = [0u8; 32];
        let buf = Crc32::new().serialize_to_slice_cobs(&DATA, &mut buf).unwrap();
        assert_eq!(buf, DATA_CRC_COBS);
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
