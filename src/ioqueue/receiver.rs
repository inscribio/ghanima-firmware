use defmt::Format;
use postcard::experimental::max_size::MaxSize;
use ringbuf::{Consumer, Producer};
use ringbuf::ring_buffer::{RbRef, RbRead, RbWrite};
use serde::Deserialize;

use crate::hal_ext::dma::{self, DmaRx};
use super::{PacketId, Queue};
use super::packet::{Packet, PacketDeser, Accumulator, DeserError, PacketMaxSize};

#[derive(Deserialize, MaxSize)]
struct MarkedPacket<P: Packet> {
    id: PacketId,
    packet: P,
}

impl<P: Packet> Packet for MarkedPacket<P> {
    type Checksum = P::Checksum;
}

/// Packet reception queue
pub struct Receiver<P, RX, RB, const B: usize>
where
    P: PacketDeser,
    RX: DmaRx,
    RB: RbRef,
    RB::Rb: RbWrite<P>,
{
    rx: RX,
    // Other fields in separate struct to satisfy borrow checker in `on_interrupt`
    state: RxState<P, RB, B>,
}

impl<P, RX, RB, const B: usize> Queue for Receiver<P, RX, RB, B>
where
    P: PacketDeser,
    RX: DmaRx,
    RB: RbRef,
    RB::Rb: RbRead<P> + RbWrite<P> + Sized,
{
    type Buffer = RB::Rb;
    type Endpoint = Consumer<P, RB>;
}

/// Packet receiver logic
struct RxState<P, RB, const B: usize>
where
    RB: RbRef,
    <RB as RbRef>::Rb: RbWrite<P>,
{
    queue: Producer<P, RB>,
    accumulator: Accumulator<B>,
    id_counter: Option<PacketId>,
    stats: Stats,
}

#[derive(Format, Default, Clone, PartialEq)]
pub struct Stats {
    pub queue_overflows: u32,
    pub accumulator_overflows: u32,
    pub cobs_errors: u32,
    pub checksum_errors: u32,
    pub deser_errors: u32,
    pub ignored_retransmissions: u32,
}

impl<P, RX, RB, const B: usize> Receiver<P, RX, RB, B>
where
    P: PacketDeser + MaxSize,
    RX: DmaRx,
    RB: RbRef,
    <RB as RbRef>::Rb: RbWrite<P>,
{
    pub const MAX_PACKET_SIZE: usize = MarkedPacket::<P>::PACKET_MAX_SIZE;
}

impl<P, RX, RB, const B: usize> Receiver<P, RX, RB, B>
where
    P: PacketDeser,
    RX: DmaRx,
    RB: RbRef,
    <RB as RbRef>::Rb: RbWrite<P>,
{
    /// Create new receiver
    pub fn new(rx: RX, queue: Producer<P, RB>) -> Self {
        Self {
            rx,
            state: RxState::new(queue),
        }
    }

    /// Handle interrupts; on [`dma::InterruptResult::Done`] new data may be available
    pub fn on_interrupt(&mut self, checksum: &mut P::Checksum) -> dma::InterruptResult {
        // Data pushing logic must be in different struct as we need
        // to keep a mutable borrow to rx while in the callback.
        self.rx.on_interrupt(|r| self.state.push(r, checksum))
    }

    pub fn stats(&self) -> &Stats {
        &self.state.stats
    }
}

impl<P, RB, const B: usize> RxState<P, RB, B>
where
    P: PacketDeser,
    RB: RbRef,
    <RB as RbRef>::Rb: RbWrite<P>,
{
    pub fn new(queue: Producer<P, RB>) -> Self {
        Self {
            queue,
            accumulator: Accumulator::new(),
            id_counter: None,
            stats: Default::default(),
        }
    }

    pub fn push(&mut self, data: &[u8], checksum: &mut P::Checksum) {
        let inc = |val: &mut u32| *val = val.saturating_add(1);
        MarkedPacket::<P>::iter_from_slice(&mut self.accumulator, checksum, data)
            .filter_map(|res| {
                if let Err(ref err) = res {
                    let cnt = match err {
                        DeserError::OverFull => &mut self.stats.accumulator_overflows,
                        DeserError::CobsDecodingError => &mut self.stats.cobs_errors,
                        DeserError::ChecksumError => &mut self.stats.checksum_errors,
                        DeserError::DeserError => &mut self.stats.deser_errors,
                    };
                    inc(cnt);
                }
                res.ok()
            })
            .for_each(|p| {
                // Ignore packets with the same ID as the last packet, assuming it's a retransmission.
                let ignore = match self.id_counter {
                    Some(id) => p.id == id,
                    None => false,
                };

                if !ignore {
                    self.id_counter = Some(p.id);
                    if self.queue.push(p.packet).is_err() {
                        inc(&mut self.stats.queue_overflows);
                    };
                } else {
                    inc(&mut self.stats.ignored_retransmissions);
                }
            })
    }
}
