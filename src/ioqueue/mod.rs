//! Packet-based IO protocol
//!
//! Implementation of packet transmission and reception queues with means of ensuring
//! fault-tolerant communication between two endpoints. The [`Packet`] trait allows
//! to mark a type to be used for protocol messages. User must implement [`serde::Serialize`]
//! and [`serde::Deserialize`] for the type. Packages will be sent with additional
//! ID and checksum. [`Transmitter`] and [`Receiver`] provide circular buffer based
//! packet queues with compile-time configurable sizes.

/// Serialization/deserialization of packets with checksum
pub mod packet;
/// Packet reception queue
pub mod receiver;
/// Packet transmission queue
pub mod transmitter;

pub use packet::Packet;
pub use receiver::{Receiver, Stats};
pub use transmitter::Transmitter;

type PacketId = u16;

/// Get maximum size of packets for given message
///
/// This is different than size of serialized `P` as ioqueue adds additional data.
/// Use this value to set the sizes of the "temporary" buffers in [`Transmitter`]
/// and [`Receiver`].
pub const fn max_packet_size<P: Packet>() -> usize {
    // FIXME: how to assert these are the same? just create a test?
    // const RX: usize = receiver::max_packet_size::<P>();
    // const TX: usize = transmitter::max_packet_size::<P>();
    // static_assertions::const_assert_eq!(rx, tx);
    receiver::max_packet_size::<P>()
}
