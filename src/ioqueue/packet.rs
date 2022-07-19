use core::marker::PhantomData;

use serde::{Serialize, Deserialize};
use postcard::ser_flavors::{Cobs, Slice};

use crate::hal_ext::{ChecksumGen, ChecksumEncoder};

/// Mark trait to define types used as checksumed packets in a protocol
///
/// While this trait does not provide anything it is used to automatically
/// implement [`PacketSer`] and [`PacketDeser`] for given type if it implements
/// proper serde traits.
pub trait Packet {
    /// Checksum generator used to add checksum to the data packets
    type Checksum: ChecksumGen;
}

// Automatically implement all the sub-traits of Packet
impl<P: Packet + Serialize> PacketSer for P {}
impl<P: Packet + for<'de> Deserialize<'de>> PacketDeser for P {}
impl<'de, P: Packet + Deserialize<'de>> PacketDeserRef<'de> for P {}

/// Serializable packet
pub trait PacketSer: Packet + Serialize {
    /// Serialize to slice
    fn to_slice<'a>(&self, checksum: &mut Self::Checksum, buf: &'a mut [u8]) -> postcard::Result<&'a mut [u8]> {
        postcard::serialize_with_flavor::<Self, ChecksumEncoder<Cobs<Slice>, Self::Checksum>, &'a mut [u8]>(
            self,
            ChecksumEncoder::new(Cobs::try_new(Slice::new(buf))?, checksum),
        )
    }
}

/// Deserializable, owned packet
///
/// This trait means that the packet can be deserialized from any lifetime, which
/// basically means that the packet does not reference the data from which it is
/// deserialized (i.e. packet struct cannot have references).
pub trait PacketDeser: Packet + for<'de> Deserialize<'de> {
    /// Iterate over packets deserialized from a slice (if any) accumulating in Accumulator
    fn iter_from_slice<'acc, 'chk, 'dat, const N: usize>(
        acc: &'acc mut Accumulator<N>, checksum: &'chk mut Self::Checksum, data: &'dat [u8]
    ) -> Iterator<'acc, 'chk, 'dat, Self, N>
    {
        Iterator { acc, checksum, window: data, _msg: PhantomData }
    }
}

/// Desierializable packet that can reference deserialized data
pub trait PacketDeserRef<'de>: Packet + Deserialize<'de> {
    /// Deserialize packet from slice (if any) and return unused slice data
    ///
    /// Note that packet lifetime is bounded by the lifetime of accumulator because
    /// it may reference the data in the Accumulator buffer that would be invalid on
    /// next Accumulator usage.
    fn get_from_slice<'chk, 'dat, const N: usize>(
        acc: &'de mut Accumulator<N>,
        checksum: &'chk mut Self::Checksum,
        data: &'dat [u8],
    ) -> (Option<Result<Self, DeserError>>, &'dat [u8]) {
        let result = acc.feed::<Self>(checksum, data);

        use FeedResult as R;
        use DeserError as E;
        let (msg, new_window) = match result {
            R::Consumed => (None, &[][..]),
            R::Success { msg, remaining } => (Some(Ok(msg)), remaining),
            R::OverFull(r) => (Some(Err(E::OverFull)), r),
            R::CobsDecodingError(r) => (Some(Err(E::CobsDecodingError)), r),
            R::ChecksumError(r) => (Some(Err(E::ChecksumError)), r),
            R::DeserError(r) => (Some(Err(E::DeserError)), r),
        };

        (msg, new_window)
    }

    // Too much magic for me, but this should be possible ...
    // fn for_each_in_slice<'acc, 'chk, 'dat, F, const N: usize>(
    //     acc: &'acc mut Accumulator<N>,
    //     checksum: &'chk mut Self::Checksum,
    //     data: &'dat [u8],
    //     handler: F,
    // )
    // where
    //     // F: FnMut(&'de Self),
    //     F: FnMut(Self),
    //     Self: 'de,
    // {
    //     let mut window = data;
    //     while !window.is_empty() {
    //         let (msg, new_window) = Self::get_from_slice(acc, checksum, window);
    //         window = new_window;
    //         if let Some(msg) = msg {
    //             handler(msg)
    //             // handler(&msg)
    //         }
    //     }
    // }

}

/// Type of error when deserializing a packet
#[derive(Debug)]
pub enum DeserError {
    /// No sentinel found and data too long to fit in internal buffer
    OverFull,
    /// Found sentinel byte but COBS decoding on the packet failed
    CobsDecodingError,
    /// COBS decoding succeeded, but checksum verification failed
    ChecksumError,
    /// Failed to deserialize message from the data even though checksum was correct
    DeserError,
}

/// Clone of postcard::CobsAccumulator but with checksum decoding
pub struct Accumulator<const N: usize> {
    buf: [u8; N],
    head: usize,
}

/// Iterator over packets deserialized from a slice
pub struct Iterator<'acc, 'chk, 'dat, P: PacketDeser, const N: usize> {
    acc: &'acc mut Accumulator<N>,
    checksum: &'chk mut P::Checksum,
    window: &'dat [u8],
    _msg: PhantomData<P>,
}

impl<'acc, 'chk, 'dat, P: PacketDeser, const N: usize> core::iter::Iterator for Iterator<'acc, 'chk, 'dat, P, N> {
    type Item = Result<P, DeserError>;

    fn next(&mut self) -> Option<Self::Item> {
        let (msg, new_window) = P::get_from_slice(self.acc, self.checksum, self.window);
        self.window = new_window;
        msg
    }
}

#[cfg_attr(test, derive(Debug))]
pub enum FeedResult<'a, P> {
    /// Consumed all data, still pending
    Consumed,
    /// No sentinel found and data too long to fit in internal buf; dropped accumulated data
    OverFull(&'a [u8]),
    /// Found sentinel byte but COBS decoding on the packet failed
    CobsDecodingError(&'a [u8]),
    /// COBS decoding succeeded, but checksum verification failed
    ChecksumError(&'a [u8]),
    /// Failed to deserialize message from the data even though checksum was correct
    DeserError(&'a [u8]),
    /// Successfully deserialized a message, returing unused input data
    Success { msg: P, remaining: &'a [u8] }
}

impl<const N: usize> Accumulator<N> {
    pub const fn new() -> Self {
        Self { buf: [0; N], head: 0 }
    }

    pub fn feed<'dat, 'chk, 'acc, P>(&'acc mut self, checksum: &'chk mut P::Checksum, data: &'dat [u8]) -> FeedResult<'dat, P>
    where
        // TODO: or maybe use PhantomData ensuring one accumulator always decodes same type of message?
        P: PacketDeserRef<'acc>
    {
        if data.is_empty() {
            return FeedResult::Consumed;
        }

        let sentinel = data.iter().position(|&i| i == 0);

        if let Some(n) = sentinel {
            // Include sentinel in the taken part
            let (take, release) = data.split_at(n + 1);

            // Just drop any data if it doesn't fit
            if (self.head + n) > N {
                self.head = 0;
                return FeedResult::OverFull(release);
            }

            // Copy only actual data without sentinel
            self.extend_unchecked(take);

            // If we found a sentinel, we'll drop all accumulated data regardless of result
            let head = self.head;
            self.head = 0;

            // Decode COBS-encoded data
            let size = match cobs::decode_in_place(&mut self.buf[..head]) {
                // Size of decoded data without sentinel
                Ok(size) => size,
                // Error could happen if some code pointed outside of the sentinel-delimited packet
                Err(_) => return FeedResult::CobsDecodingError(release),
            };

            // Verify that the decoded data contains correct checksum, else don't interpret it
            let data = match checksum.verify(&self.buf[..size])  {
                Ok(data) => data,
                Err(_) => return FeedResult::ChecksumError(release),
            };

            // Deserialize the data into a message
            let (msg, _remaining) = match postcard::take_from_bytes(data) {
                Err(_) => return FeedResult::DeserError(release),
                Ok(result) => result,
            };

            FeedResult::Success { msg, remaining: release }
        } else {
            // No sentinel - accumulate if it fits
            if (self.head + data.len()) <= N {
                self.extend_unchecked(data);
                FeedResult::Consumed
            } else {
                let new_start = N - self.head;
                self.head = 0;
                FeedResult::OverFull(&data[new_start..])
            }
        }
    }

    fn extend_unchecked(&mut self, data: &[u8]) {
        let new_head = self.head + data.len();
        self.buf[self.head..new_head].copy_from_slice(data);
        self.head = new_head;
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use core::iter;
    use std::vec::Vec;
    use std::string::String;
    use crate::hal_ext::checksum_mock::Crc32;

    // Transforms "0b11001111 1001 0001 d15 x0f" into [0xcf, 0x91, 15, 0x0f]
    pub fn bytes(description: &str) -> Vec<u8> {
        let parts = description.split_whitespace();
        parts.flat_map(|s| {
            if s.starts_with('#') {  // Comment
                vec![]
            } else if s.starts_with('c') {  // ASCII characters
                s[1..].bytes().collect()
            } else if s.starts_with('d') {  // Decimal
                vec![u8::from_str_radix(&s[1..], 10).unwrap()]
            } else if s.starts_with('x') {  // Hexadecimal
                let s = &s[1..];
                assert!(s.len() % 2 == 0, "Each format must describe multiple of a byte");
                let n_bytes = s.len();
                assert!(s.is_ascii());
                s.as_bytes().chunks(2).map(|pair| {
                    let base = '0' as u8;
                    u8::from_str_radix(&String::from_utf8(vec![pair[0], pair[1]]).unwrap(), 16).unwrap()
                }).collect()
            } else {  // Binary
                // Remove underscores
                let s = s.chars().filter_map(|c| if c == '_' {
                    None
                } else {
                    Some(c)
                }).collect::<Vec<_>>();

                // Parse as binary
                assert!(s.len() % 8 == 0, "Each format must describe multiple of a byte");
                s.chunks_exact(8).map(|bits| {
                    let bitstr: String = bits.iter().collect();
                    u8::from_str_radix(&bitstr, 2).unwrap()
                }).collect()
            }
        }).collect()
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Message {
        a: u32,
        b: u16,
        c: u8,
    }

    impl Packet for Message {
        type Checksum = Crc32;
    }

    #[test]
    fn serialize_to_slice() {
        let mut crc = Crc32::new();
        let mut buf = [0u8; 16];
        let msg = Message { a: 0x000a55bb, b: 0x1234, c: 0xff };
        let data = msg.to_slice(&mut crc, &mut buf).unwrap();
        assert_eq!(data, bytes(r"
            d11
            1_0111011 1_0101011 00101001
            1_0110100 0_0100100  xff
            xfe xab x30 xf7
            d0
        "));
    }

    #[test]
    fn deserialize_with_accumulator() {
        let mut crc = Crc32::new();
        let mut acc = Accumulator::<32>::new();
        let buf = &bytes(r"
            x12 x34 x56 x78 x90 x00                                                       #1
            d11 1_0111011 1_0101011 00101001 1_0110100 0_0100100 xff xfe xab x30 xf7 d0   #2
            d3 xee xdd d3 xbb xaa d0                                                      #3
            d11 1_0111011 1_0101011 00101001 1_0110100 0_0100100 xff xfe xab x30 xf7 d0   #4
            d0 x02                                                                        #5
        ");

        // 1
        let buf = match acc.feed::<Message>(&mut crc, buf) {
            FeedResult::CobsDecodingError(remaining) => remaining,
            r => panic!("Unexpected result: {:02x?}", r),
        };

        // 2
        let (buf, msg) = match acc.feed::<Message>(&mut crc, buf) {
            FeedResult::Success { msg, remaining } => (remaining, msg),
            r => panic!("Unexpected result: {:02x?}", r),
        };
        assert_eq!(msg, Message { a: 0x000a55bb, b: 0x1234, c: 0xff });

        // 3
        let buf = match acc.feed::<Message>(&mut crc, buf) {
            FeedResult::ChecksumError(remaining) => remaining,
            r => panic!("Unexpected result: {:02x?}", r),
        };

        // 4
        let (buf, msg) = match acc.feed::<Message>(&mut crc, buf) {
            FeedResult::Success { msg, remaining } => (remaining, msg),
            r => panic!("Unexpected result: {:02x?}", r),
        };
        assert_eq!(msg, Message { a: 0x000a55bb, b: 0x1234, c: 0xff });

        // 5
        let buf = match acc.feed::<Message>(&mut crc, buf) {
            FeedResult::ChecksumError(remaining) => remaining,
            r => panic!("Unexpected result: {:02x?}", r),
        };
        assert!(matches!(acc.feed::<Message>(&mut crc, buf), FeedResult::Consumed));
    }

    #[test]
    fn deserialize_iter_from_slice() {
        let mut crc = Crc32::new();
        let mut acc = Accumulator::<32>::new();
        let buf = &bytes(r"
            x12 x34 x56 x78 x90 x00                                                       #1
            d11 1_0111011 1_0101011 00101001 1_0110100 0_0100100 xff xfe xab x30 xf7 d0   #2
            d3 xee xdd d3 xbb xaa d0                                                      #3
            d11 1_0111011 1_0101011 00101001 1_0110100 0_0100100 xff xfe xab x30 xf7 d0   #4
            d0 x02                                                                        #5
        ");
        let msgs = Message::iter_from_slice(&mut acc, &mut crc, &buf)
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();
        assert_eq!(msgs, vec![Message { a: 0x000a55bb, b: 0x1234, c: 0xff }; 2]);
    }

    #[test]
    fn deserialize_iter_from_slice_missing() {
        let mut crc = Crc32::new();
        let mut acc = Accumulator::<32>::new();
        let buf = &bytes(r"
            x12 x34 x56 x78 x90 x90  #no_0x00_so_decoding_should_fail                     #1
            d11 1_0111011 1_0101011 00101001 1_0110100 0_0100100 xff xfe xab x30 xf7 d0   #2
            d3 xee xdd d3 xbb xaa d0                                                      #3
            d11 1_0111011 1_0101011 00101001 1_0110100 0_0100100 xff xfe xab x30 xf7 d0   #4
            d0 x02                                                                        #5
        ");
        let msgs = Message::iter_from_slice(&mut acc, &mut crc, &buf)
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();
        assert_eq!(msgs, vec![Message { a: 0x000a55bb, b: 0x1234, c: 0xff }; 1]);
    }

    #[derive(Serialize)]
    struct MessageWithRef<'a> {
        id: u16,
        other: &'a Message,
    }

    impl<'a> Packet for MessageWithRef<'a> {
        type Checksum = Crc32;
    }

    #[test]
    fn serialize_to_slice_with_ref() {
        let mut crc = Crc32::new();
        let mut buf = [0u8; 18];
        let other = &Message { a: 0xaa0055bb, b: 0x1234, c: 0xff };
        let msg = MessageWithRef { id: 0xbaad, other };
        let data = msg.to_slice(&mut crc, &mut buf).unwrap();
        assert_eq!(data, bytes(r"
            d16
            xad 1_1110101 000000_10
            1_0111011 1_0101011 1_0000001 1_1010000 0000_1010
            1_0110100 0_0100100
            xff
            xbb x13 xc5 x0f
            d0
        "));
    }

    // Deserialize with references probably won't work for types other than &str or &[u8]
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct MessageWithSimpleRef<'a> {
        id: u16,
        name: &'a str,
    }

    impl<'a> Packet for MessageWithSimpleRef<'a> {
        type Checksum = Crc32;
    }

    #[test]
    fn serialize_to_slice_with_simple_ref() {
        let mut crc = Crc32::new();
        let mut buf = [0u8; 32];
        let name = "Yossarian";  // bytes: 89, 111, 115, 115, 97, 114, 105, 97, 110
        let msg = MessageWithSimpleRef { id: 0xbaad, name };
        let data = msg.to_slice(&mut crc, &mut buf).unwrap();
        assert_eq!(data, bytes(r"
            d18
            xad 1_1110101 000000_10
            d9 cYossarian
            x77 xa0 xce x3f
            d0
        "));
    }

    #[test]
    fn deserialize_from_slice_with_ref() {
        let mut crc = Crc32::new();
        let mut acc = Accumulator::<20>::new();
        let buf = [
            0xff, 0xee, 0xdd, 0xcc, 0,
            17,
            0xad, 0xba,
            9, b'Y', b'o', b's', b's', b'a', b'r', b'i', b'a', b'n',
            0xc5, 0xab, 0x20, 0xd6,
            0,
            4, 0x22, 0x33, 0x44, 0
        ];
        let buf = &bytes(r"
            xff xee xdd xcc d0
            d18
            xad 1_1110101 000000_10
            d9 cYossarian
            x77 xa0 xce x3f
            d0
            d4 x22 x33 x44 d0
        ");

        let (msg, buf) = MessageWithSimpleRef::get_from_slice(&mut acc, &mut crc, &buf);
        assert!(matches!(msg.unwrap(), Err(DeserError::CobsDecodingError)));
        assert_eq!(buf, bytes(r"
            d18
            xad 1_1110101 000000_10
            d9 cYossarian
            x77 xa0 xce x3f
            d0
            d4 x22 x33 x44 d0
        "));

        let (msg, buf) = MessageWithSimpleRef::get_from_slice(&mut acc, &mut crc, &buf);
        assert_eq!(msg.unwrap().unwrap(), MessageWithSimpleRef { id: 0xbaad, name: "Yossarian" });
        assert_eq!(buf, bytes(r"
            d4 x22 x33 x44 d0
        "));

        let (msg, buf) = MessageWithSimpleRef::get_from_slice(&mut acc, &mut crc, &buf);
        assert!(matches!(msg.unwrap(), Err(DeserError::ChecksumError)));
        let empty: [u8; 0] = [];  // multiple `impl`s of PartialEq because of the crate `fixed`
        assert_eq!(buf, empty);
    }
}
