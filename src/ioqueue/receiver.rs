use ringbuffer::{ConstGenericRingBuffer, RingBufferRead, RingBufferWrite};
use serde::Deserialize;

use crate::hal_ext::dma::{self, DmaRx};
use super::{PacketId, ReceiveQueue};
use super::packet::{Packet, PacketDeser, Accumulator};

#[derive(Deserialize)]
struct MarkedPacket<P: Packet> {
    id: PacketId,
    packet: P,
}

impl<P: Packet> Packet for MarkedPacket<P> {
    type Checksum = P::Checksum;
}

/// Packet reception queue
pub struct Receiver<P, RX, const N: usize, const B: usize>
where
    P: PacketDeser,
    RX: DmaRx,
{
    rx: RX,
    // Other fields in separate struct to satisfy borrow checker in `on_interrupt`
    state: RxState<P, N, B>,
}

/// Packet receiver logic
struct RxState<P, const N: usize, const B: usize> {
    queue: ConstGenericRingBuffer<P, N>,
    accumulator: Accumulator<B>,
    id_counter: Option<PacketId>,
}

impl<P, RX, const N: usize, const B: usize> ReceiveQueue<P> for Receiver<P, RX, N, B>
where
    P: PacketDeser,
    RX: DmaRx,
{
    // TODO: is there a way to just expose &mut impl RingBufferRead<P>? Seems impossible via trait
    fn get(&mut self) -> Option<P> {
        self.queue().dequeue()
    }
}

impl<P, RX, const N: usize, const B: usize> Receiver<P, RX, N, B>
where
    P: PacketDeser,
    RX: DmaRx,
{
    /// Create new receiver
    pub fn new(rx: RX) -> Self {
        Self {
            rx,
            state: RxState::new(),
        }
    }

    /// Handle interrupts; on [`dma::InterruptResult::Done`] new data may be available
    pub fn on_interrupt(&mut self, checksum: &mut P::Checksum) -> dma::InterruptResult {
        // Data pushing logic must be in different struct as we need
        // to keep a mutable borrow to rx while in the callback.
        self.rx.on_interrupt(|r| self.state.push(r, checksum))
    }

    // TODO: or should we use callbacks?
    /// Read data from the underlying queue
    pub fn queue(&mut self) -> &mut impl RingBufferRead<P> {
        &mut self.state.queue
    }
}

impl<P, const N: usize, const B: usize> RxState<P, N, B>
where
    P: PacketDeser,
{
    pub fn new() -> Self {
        Self {
            queue: ConstGenericRingBuffer::new(),
            accumulator: Accumulator::new(),
            id_counter: None,
        }
    }

    pub fn push(&mut self, data: &[u8], checksum: &mut P::Checksum) {
        for p in MarkedPacket::<P>::iter_from_slice(&mut self.accumulator, checksum, data) {
            // Ignore packets with the same ID as the last packet, assuming it's a retransmission.
            let ignore = match self.id_counter {
                Some(id) => p.id == id,
                None => false,
            };

            if !ignore {
                self.id_counter = Some(p.id);
                self.queue.push(p.packet);
            }
        }
    }
}
