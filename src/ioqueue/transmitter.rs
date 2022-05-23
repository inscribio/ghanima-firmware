use serde::Serialize;
use ringbuffer::{ConstGenericRingBuffer, RingBuffer, RingBufferExt, RingBufferRead, RingBufferWrite};

use super::{PacketId, TransmitQueue};
use super::packet::{Packet, PacketSer};
use crate::hal_ext::dma::{self, DmaTx};

/// Packet with an ID that allows to detect retransmissions
#[derive(Serialize)]
struct MarkedPacket<'a, P: Packet> {
    id: PacketId,
    #[serde(borrow)]
    packet: &'a P,
}

impl<'a, P: Packet> Packet for MarkedPacket<'a, P> {
    type Checksum = P::Checksum;
}

/// Packet transmission queue
pub struct Transmitter<P, TX, const N: usize>
where
    P: PacketSer,
    TX: DmaTx,
{
    queue: ConstGenericRingBuffer<P, N>,
    tx: TX,
    id_counter: PacketId,
    // TODO: implement retransmission? it is probably unnecessary as we have good data integrity
    _retransmissions: u8,
}

impl<P, TX, const N: usize> TransmitQueue<P> for Transmitter<P, TX, N>
where
    P: PacketSer,
    TX: DmaTx,
{
    fn push(&mut self, packet: P) {
        self.push(packet)
    }
}

impl<P, TX, const N: usize> Transmitter<P, TX, N>
where
    P: PacketSer,
    TX: DmaTx,
{
    /// Create new transmitter
    pub fn new(tx: TX) -> Self {
        Self {
            queue: ConstGenericRingBuffer::new(),
            tx,
            id_counter: 0,
            _retransmissions: 0,
        }
    }

    /// Push a packet if there is enough space in queue
    pub fn try_push(&mut self, packet: P) -> Result<(), ()> {
        if self.queue.is_full() {
            Err(())
        } else {
            self.push(packet);
            Ok(())
        }
    }

    /// Push a packet, overwrite oldest if queue is full
    pub fn push(&mut self, packet: P) {
        self.queue.push(packet);
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

            while let Some(packet) = self.queue.peek() {
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
                self.queue.skip();
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
    use super::*;
    use std::vec::Vec;
    use std::cell::Cell;
    use crate::hal_ext::dma::mock::DmaTxMock;
    use crate::hal_ext::checksum_mock::Crc32;

    // Explicit type because using just [] yields "multiple `impl`s of PartialEq" because of the crate `fixed`
    const EMPTY: [u8; 0] = [];

    #[derive(Serialize)]
    struct Message(u16, u8);

    impl Packet for Message {
        type Checksum = Crc32;
    }

    #[test]
    fn send_single() {
        let mut crc = Crc32::new();
        let sent = Cell::new(Vec::new());
        let dma = DmaTxMock::<_, 30>::new(true, |data| sent.set(data));
        let mut tx = Transmitter::<Message, _, 4>::new(dma);

        assert_eq!(sent.take(), EMPTY);
        tx.push(Message(0xaabb, 0xcc));
        assert_eq!(sent.take(), EMPTY);

        // Before COBS = id(00 00) 0(bb aa) 1(cc) crc32(23 09 66 61)
        let cobs = [1, 1, 8, 0xbb, 0xaa, 0xcc, 0x23, 0x09, 0x66, 0x61, 0];

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
        let mut tx = Transmitter::<Message, _, 4>::new(dma);

        for _ in 0..3 {
            tx.push(Message(0xaabb, 0xcc));
            assert_eq!(sent.take(), EMPTY);
        }
        let cobs = [
            1, 1, 8, 0xbb, 0xaa, 0xcc, 0x23, 0x09, 0x66, 0x61, 0,     // id(00 00)
            2, 0x01, 8, 0xbb, 0xaa, 0xcc, 0xae, 0x6e, 0x6b, 0x28, 0,  // id(01 00)
            2, 0x02, 8, 0xbb, 0xaa, 0xcc, 0x39, 0xc6, 0x7c, 0xf3, 0,  // id(02 00)
        ];

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
        let mut tx = Transmitter::<Message, _, 4>::new(dma);

        for _ in 0..3 {
            tx.push(Message(0xaabb, 0xcc));
            assert_eq!(sent.take(), EMPTY);
        }
        let cobs = [
            1, 1, 8, 0xbb, 0xaa, 0xcc, 0x23, 0x09, 0x66, 0x61, 0,     // id(00 00)
            2, 0x01, 8, 0xbb, 0xaa, 0xcc, 0xae, 0x6e, 0x6b, 0x28, 0,  // id(01 00)
            2, 0x02, 8, 0xbb, 0xaa, 0xcc, 0x39, 0xc6, 0x7c, 0xf3, 0,  // id(02 00)
        ];

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
        let mut tx = Transmitter::<Message, _, 4>::new(dma);

        tx.push(Message(0xaabb, 0xcc));
        let cobs = [1, 1, 8, 0xbb, 0xaa, 0xcc, 0x23, 0x09, 0x66, 0x61, 0];

        tx.tick(&mut crc);
        assert_eq!(sent.take(), EMPTY);
        tx.on_interrupt();
        tx.tick(&mut crc);
        assert_eq!(sent.take(), cobs);
        tx.tick(&mut crc);
        assert_eq!(sent.take(), EMPTY);
    }
}
