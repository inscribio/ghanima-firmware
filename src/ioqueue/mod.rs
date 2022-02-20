//! Packet-based IO protocol
//!
//! Implementation of packet transmission and reception queues with means of ensuring
//! fault-tolerant communication between two endpoints. The [`Packet`] trait allows
//! to mark a type to be used for protocol messages. User must implement [`serde::Serialize`]
//! and [`serde::Deserialize`] for the type. Packages will be sent with additional
//! ID and checksum. [`Transmitter`] and [`Receiver`] provide circular buffer based
//! packet queues with compile-time configurable sizes.

/// Serialization/deserialization of packets with checksum
mod packet;
/// Packet reception queue
mod receiver;
/// Packet transmission queue
mod transmitter;

pub use packet::Packet;
pub use receiver::Receiver;
pub use transmitter::Transmitter;

type PacketId = u16;

/// Access to transmitter's queue
pub trait TransmitQueue<P: Packet> {
    /// Push packet overwriting oldest one if the queue is full
    fn push(&mut self, packet: P);
}

/// Access to receiver's queue
pub trait ReceiveQueue<P: Packet> {
    /// Read oldest packet from the queue
    fn get(&mut self) -> Option<P>;
}
