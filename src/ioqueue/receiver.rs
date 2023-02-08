use core::marker::PhantomData;

use defmt::Format;
use postcard::experimental::max_size::MaxSize;
use bbqueue::Consumer;
use serde::Deserialize;

use super::PacketId;
use super::packet::{self, Packet, PacketDeser, Accumulator, PacketMaxSize};

#[derive(Deserialize)]
struct MarkedPacket<P: Packet> {
    id: PacketId,
    packet: P,
}

impl<P: Packet> Packet for MarkedPacket<P> {
    type Checksum = P::Checksum;
}

impl<P: Packet> MaxSize for MarkedPacket<P> {
    const POSTCARD_MAX_SIZE: usize = core::mem::size_of::<PacketId>() + P::PACKET_MAX_SIZE;
}

/// Packet reception queue
pub struct Receiver<P, const N: usize, const B: usize>
where
    P: PacketDeser,
{
    rx: Consumer<'static, N>,
    accumulator: Accumulator<B>,
    id_counter: Option<PacketId>,
    stats: Stats,
    _packet: PhantomData<P>,
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

pub const fn max_packet_size<P: Packet>() -> usize {
    MarkedPacket::<P>::PACKET_MAX_SIZE
}

impl<P, const N: usize, const B: usize> Receiver<P, N, B>
where
    P: PacketDeser,
{
    pub const QUEUE_BUF_SIZE: usize = N;
    pub const TMP_BUF_SIZE: usize = B;
    pub const MAX_PACKET_SIZE: usize = MarkedPacket::<P>::PACKET_MAX_SIZE;

    /// Create new receiver
    pub fn new(rx: Consumer<'static, N>) -> Self {
        Self {
            rx,
            accumulator: Accumulator::new(),
            id_counter: None,
            stats: Default::default(),
            _packet: PhantomData,
        }
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn read(&mut self, checksum: &mut P::Checksum) -> Option<P> {
        let inc = |val: &mut u32| *val = val.saturating_add(1);

        let grant = match self.rx.read() {
            Ok(grant) => grant,
            Err(err) => match err {
                bbqueue::Error::InsufficientSize => return None, // size = 0
                bbqueue::Error::GrantInProgress => unreachable!(),
                bbqueue::Error::AlreadySplit => unreachable!(),
            },
        };

        use packet::FeedResult as F;
        let (result, remaining) = match self.accumulator.feed::<MarkedPacket<P>>(checksum, &grant) {
            F::Success { msg, remaining } => (Ok(Some(msg)), remaining),
            F::Consumed => (Ok(None), &[][..]),
            F::OverFull(r) => (Err(&mut self.stats.accumulator_overflows), r),
            F::CobsDecodingError(r) => (Err(&mut self.stats.cobs_errors), r),
            F::ChecksumError(r) => (Err(&mut self.stats.checksum_errors), r),
            F::DeserError(r) => (Err(&mut self.stats.deser_errors), r),
        };

        let msg = match result {
            Err(error) => { inc(error); None },
            Ok(None) => None,
            Ok(Some(p)) => {
                // Ignore packets with the same ID as the last packet, assuming it's a retransmission.
                let ignore = match self.id_counter {
                    Some(id) => p.id == id,
                    None => false,
                };

                if ignore {
                    inc(&mut self.stats.ignored_retransmissions);
                    None
                } else {
                    self.id_counter = Some(p.id);
                    Some(p.packet)
                }
            },
        };

        let consumed = grant.len() - remaining.len();
        grant.release(consumed);

        msg
    }
}
