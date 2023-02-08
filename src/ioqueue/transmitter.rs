use core::marker::PhantomData;

use bbqueue::Producer;
use postcard::experimental::max_size::MaxSize;
use serde::Serialize;

use super::PacketId;
use super::packet::{Packet, PacketSer, PacketMaxSize};

/// Packet with an ID that allows to detect retransmissions
#[derive(Serialize, MaxSize)]
struct MarkedPacket<'a, P: Packet + 'a> {
    id: PacketId,
    #[serde(borrow)]
    packet: &'a P,
}

impl<'a, P: Packet> Packet for MarkedPacket<'a, P> {
    type Checksum = P::Checksum;
}

/// Packet transmission queue
pub struct Transmitter<'a, P, const N: usize, const B: usize>
where
    P: PacketSer,
{
    tx: Producer<'a, N>,
    buf: [u8; B],
    id_counter: PacketId,
    // TODO: implement retransmission? it is probably unnecessary as we have good data integrity
    _retransmissions: u8,
    _packet: PhantomData<P>,
}

pub const fn max_packet_size<P: Packet>() -> usize {
    MarkedPacket::<'_, P>::PACKET_MAX_SIZE
}

impl<'a, P, const N: usize, const B: usize> Transmitter<'a, P, N, B>
where
    P: PacketSer,
{
    pub const QUEUE_BUF_SIZE: usize = N;
    pub const TMP_BUF_SIZE: usize = B;
    pub const MAX_PACKET_SIZE: usize = MarkedPacket::<'_, P>::PACKET_MAX_SIZE;

    /// Create new transmitter
    pub fn new(tx: Producer<'a, N>) -> Self {
        Self {
            tx,
            buf: [0; B],
            id_counter: 0,
            _retransmissions: 0,
            _packet: PhantomData,
        }
    }

    pub fn send(&mut self, checksum: &mut P::Checksum, packet: impl Into<P>) -> bool {
        let packet = MarkedPacket {
            id: self.id_counter,
            packet: &packet.into(),
        };

        let serialized = match packet.to_slice(checksum, &mut self.buf) {
            Err(postcard::Error::SerializeBufferFull) => panic!("Packet larger than max size"),
            res => res.map_err(drop).unwrap(), // It should not be possible to get any other error
        };

        let mut grant = match self.tx.grant_exact(serialized.len()) {
            Ok(grant) => grant,
            Err(e) => match e {
                bbqueue::Error::InsufficientSize => return false,
                bbqueue::Error::GrantInProgress => unreachable!(),
                bbqueue::Error::AlreadySplit => unreachable!(),
            }
        };

        grant.copy_from_slice(serialized);
        grant.commit(serialized.len());
        self.id_counter = self.id_counter.wrapping_add(1);

        true
    }
}

#[cfg(test)]
mod tests {
    use bbqueue::BBBuffer;

    use super::*;
    use std::vec::Vec;
    use std::cell::Cell;
    use crate::hal_ext::dma::mock::DmaTxMock;
    use crate::hal_ext::checksum_mock::Crc32;
    use crate::ioqueue::packet::tests::bytes;

    // Explicit type because using just [] yields "multiple `impl`s of PartialEq" because of the crate `fixed`
    const EMPTY: [u8; 0] = [];

    #[derive(Serialize, MaxSize, Debug)]
    struct Message(u16, u8);

    impl Packet for Message {
        type Checksum = Crc32;
    }

    const MAX_SIZE: usize = max_packet_size::<Message>();

    #[test]
    fn send_single() {
        let mut crc = Crc32::new();
        let rb = BBBuffer::<16>::new();
        let (prod, mut cons) = rb.try_split().unwrap();
        let mut tx = Transmitter::<Message, 16, MAX_SIZE>::new(prod);

        assert_eq!(cons.read(), Err(bbqueue::Error::InsufficientSize));
        assert_eq!(tx.send(&mut crc, Message(0xaabb, 0xcc)), true);

        // Message                     encoded (varints, checksum)
        // id(00 00) 0(bb aa) 1(cc) -> id(00) 0(1_0111011 1_1010101 000000_10) 1(cc) crc32(ee e6 af 2c)
        let cobs = bytes("d1  d9  1_0111011 1_1010101 000000_10  xcc  xee xe6 xaf x2c  d0");

        let grant = cons.read().unwrap();
        assert_eq!(grant.buf(), &cobs);
    }

    #[test]
    fn send_multiple() {
        let mut crc = Crc32::new();
        let rb = BBBuffer::<40>::new();
        let (prod, mut cons) = rb.try_split().unwrap();
        let mut tx = Transmitter::<Message, 40, MAX_SIZE>::new(prod);

        for _ in 0..3 {
            assert_eq!(tx.send(&mut crc, Message(0xaabb, 0xcc)), true);
        }
        let cobs = bytes(r"
            d1   d9  1_0111011 1_1010101 000000_10  xcc  xee xe6 xaf x2c  d0  #id=0x0000
            d10 x01  1_0111011 1_1010101 000000_10  xcc  x63 x81 xa2 x65  d0  #id=0x0001
            d10 x02  1_0111011 1_1010101 000000_10  xcc  xf4 x29 xb5 xbe  d0  #id=0x0002
        ");

        let grant = cons.read().unwrap();
        assert_eq!(grant.buf(), &cobs);
    }

    #[test]
    fn send_as_much_as_possible() {
        let mut crc = Crc32::new();
        let rb = BBBuffer::<30>::new();
        let (prod, mut cons) = rb.try_split().unwrap();
        let mut tx = Transmitter::<Message, 30, MAX_SIZE>::new(prod);

        let results = [true, true, false];
        for i in 0..3 {
            assert_eq!(tx.send(&mut crc, Message(0xaabb, 0xcc)), results[i]);
        }
        let cobs = bytes(r"
            d1   d9  1_0111011 1_1010101 000000_10  xcc  xee xe6 xaf x2c  d0  #id=0x0000
            d10 x01  1_0111011 1_1010101 000000_10  xcc  x63 x81 xa2 x65  d0  #id=0x0001
            d10 x02  1_0111011 1_1010101 000000_10  xcc  xf4 x29 xb5 xbe  d0  #id=0x0002
        ");

        let grant = cons.read().unwrap();
        assert_eq!(grant.buf(), &cobs[..22]);
    }
}
