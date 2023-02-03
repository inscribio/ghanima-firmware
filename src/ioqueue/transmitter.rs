use postcard::experimental::max_size::MaxSize;
use ringbuf::ring_buffer::{RbRef, RbRead, RbWrite};
use serde::Serialize;
use ringbuf::{Consumer, Producer};

use super::{PacketId, Queue};
use super::packet::{Packet, PacketSer, PacketMaxSize};
use crate::hal_ext::dma::{self, DmaTx};

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
pub struct Transmitter<P, TX, RB>
where
    P: PacketSer,
    TX: DmaTx,
    RB: RbRef,
    RB::Rb: RbRead<P>,
{
    queue: Consumer<P, RB>,
    tx: TX,
    id_counter: PacketId,
    // TODO: implement retransmission? it is probably unnecessary as we have good data integrity
    _retransmissions: u8,
}

impl<P, TX, RB> Queue for Transmitter<P, TX, RB>
where
    P: PacketSer,
    TX: DmaTx,
    RB: RbRef,
    RB::Rb: RbRead<P> + RbWrite<P> + Sized,
{
    type Buffer = RB::Rb;
    type Endpoint = Producer<P, RB>;
}

impl<'a, P, TX, RB> Transmitter<P, TX, RB>
where
    P: PacketSer + MaxSize + 'a,
    TX: DmaTx,
    RB: RbRef,
    RB::Rb: RbRead<P>,
{
    pub const MAX_PACKET_SIZE: usize = MarkedPacket::<'a, P>::PACKET_MAX_SIZE;
}

impl<P, TX, RB> Transmitter<P, TX, RB>
where
    P: PacketSer,
    TX: DmaTx,
    RB: RbRef,
    RB::Rb: RbRead<P>,
{
    /// Create new transmitter
    pub fn new(tx: TX, queue: Consumer<P, RB>) -> Self {
        Self {
            queue,
            tx,
            id_counter: 0,
            _retransmissions: 0,
        }
    }

    /// Check DMA TX state and send packets from queue if possible.
    /// Returns `true` if started a transfer.
    pub fn tick(&mut self, checksum: &mut P::Checksum) -> bool {
        if !self.tx.is_ready() || self.queue.is_empty() {
            return false;
        }

        // Push as much as possible
        // TODO: retransmission if only a single packet is in queue
        self.tx.push(|buf| {
            let n = buf.len();
            let mut window = buf;

            while let Some(packet) = self.queue.iter().last() { // FIXME: implement .peek() in extension trait
                let packet = MarkedPacket {
                    id: self.id_counter,
                    packet,
                };

                let len = match packet.to_slice(checksum, window) {
                    // When there is no more space in DMA buffer we won't transmit this packet
                    Err(postcard::Error::SerializeBufferFull) => break,
                    // On other errors we discard this packet
                    Err(_) => 0,
                    Ok(data) => {
                        self.id_counter = self.id_counter.wrapping_add(1);
                        data.len()
                    }
                };

                // Consume this packet and update the window to point to remaining buffer space
                self.queue.skip(1);
                window = &mut window[len..]
            }

            // Indicate how many bytes have been written
            n - window.len()
        }).map_err(|_| ()).unwrap();  // Infallible as we've already checked that tx.is_ready()

        // Start the transfer, should never fail because we check that tx.is_ready()
        nb::block!(self.tx.start()).map_err(|_| ()).unwrap();
        true
    }

    /// Perform interrupt processing, should be called in all relevant IRQ handlers
    pub fn on_interrupt(&mut self) -> dma::InterruptResult {
        self.tx.on_interrupt()
    }
}

#[cfg(test)]
mod tests {
    use ringbuf::StaticRb;

    use super::*;
    use std::vec::Vec;
    use std::cell::Cell;
    use crate::hal_ext::dma::mock::DmaTxMock;
    use crate::hal_ext::checksum_mock::Crc32;
    use crate::ioqueue::packet::tests::bytes;

    // Explicit type because using just [] yields "multiple `impl`s of PartialEq" because of the crate `fixed`
    const EMPTY: [u8; 0] = [];

    #[derive(Serialize, Debug)]
    struct Message(u16, u8);

    impl Packet for Message {
        type Checksum = Crc32;
    }

    #[test]
    fn send_single() {
        let mut crc = Crc32::new();
        let sent = Cell::new(Vec::new());
        let dma = DmaTxMock::<_, 30>::new(true, |data| sent.set(data));
        let mut rb = StaticRb::<Message, 4>::default();
        let (mut prod, cons) = rb.split_ref();
        let mut tx = Transmitter::new(dma, cons);

        assert_eq!(sent.take(), EMPTY);
        prod.push(Message(0xaabb, 0xcc)).unwrap();
        assert_eq!(sent.take(), EMPTY);

        // Message                     encoded (varints, checksum)
        // id(00 00) 0(bb aa) 1(cc) -> id(00) 0(1_0111011 1_1010101 000000_10) 1(cc) crc32(ee e6 af 2c)
        let cobs = bytes("d1  d9  1_0111011 1_1010101 000000_10  xcc  xee xe6 xaf x2c  d0");

        tx.tick(&mut crc);
        assert_eq!(sent.take(), cobs);
        tx.tick(&mut crc);
        assert_eq!(sent.take(), EMPTY);
    }

    #[test]
    fn send_multiple() {
        let mut crc = Crc32::new();
        let sent = Cell::new(Vec::new());
        let dma = DmaTxMock::<_, 40>::new(true, |data| sent.set(data));
        let mut rb = StaticRb::<Message, 4>::default();
        let (mut prod, cons) = rb.split_ref();
        let mut tx = Transmitter::new(dma, cons);

        for _ in 0..3 {
            prod.push(Message(0xaabb, 0xcc)).unwrap();
            assert_eq!(sent.take(), EMPTY);
        }
        let cobs = bytes(r"
            d1   d9  1_0111011 1_1010101 000000_10  xcc  xee xe6 xaf x2c  d0  #id=0x0000
            d10 x01  1_0111011 1_1010101 000000_10  xcc  x63 x81 xa2 x65  d0  #id=0x0001
            d10 x02  1_0111011 1_1010101 000000_10  xcc  xf4 x29 xb5 xbe  d0  #id=0x0002
        ");

        tx.tick(&mut crc);
        assert_eq!(sent.take(), cobs);
        tx.tick(&mut crc);
        assert_eq!(sent.take(), EMPTY);
    }

    #[test]
    fn send_as_much_as_possible() {
        let mut crc = Crc32::new();
        let sent = Cell::new(Vec::new());
        let dma = DmaTxMock::<_, 30>::new(true, |data| sent.set(data));
        let mut rb = StaticRb::<Message, 4>::default();
        let (mut prod, cons) = rb.split_ref();
        let mut tx = Transmitter::new(dma, cons);

        for _ in 0..3 {
            prod.push(Message(0xaabb, 0xcc)).unwrap();
            assert_eq!(sent.take(), EMPTY);
        }
        let cobs = bytes(r"
            d1   d9  1_0111011 1_1010101 000000_10  xcc  xee xe6 xaf x2c  d0  #id=0x0000
            d10 x01  1_0111011 1_1010101 000000_10  xcc  x63 x81 xa2 x65  d0  #id=0x0001
            d10 x02  1_0111011 1_1010101 000000_10  xcc  xf4 x29 xb5 xbe  d0  #id=0x0002
        ");

        // cobs.len()=33 so it won't fit in the first tick
        tx.tick(&mut crc);
        assert_eq!(sent.take(), cobs[..22]);
        // now the rest of data should be sent
        tx.tick(&mut crc);
        assert_eq!(sent.take(), cobs[22..]);
        // no more data to send
        tx.tick(&mut crc);
        assert_eq!(sent.take(), EMPTY);
    }

    #[test]
    fn send_only_after_dma_ready() {
        let mut crc = Crc32::new();
        let sent = Cell::new(Vec::new());
        let dma = DmaTxMock::<_, 30>::new(false, |data| sent.set(data));
        let mut rb = StaticRb::<Message, 4>::default();
        let (mut prod, cons) = rb.split_ref();
        let mut tx = Transmitter::new(dma, cons);

        prod.push(Message(0xaabb, 0xcc)).unwrap();
        let cobs = bytes("d1  d9  1_0111011 1_1010101 000000_10  xcc  xee xe6 xaf x2c  d0");

        tx.tick(&mut crc);
        assert_eq!(sent.take(), EMPTY);
        tx.on_interrupt();
        tx.tick(&mut crc);
        assert_eq!(sent.take(), cobs);
        tx.tick(&mut crc);
        assert_eq!(sent.take(), EMPTY);
    }
}
