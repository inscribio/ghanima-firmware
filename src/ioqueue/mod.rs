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

use ringbuf::{Producer, ring_buffer::{RbRef, RbWrite}};

type PacketId = u16;

/// Helper trait to simplify specifying ring buffer types for [`Transmitter`]/[`Receiver`]
pub trait Queue {
    /// Type of backing buffer
    type Buffer;
    /// Type of user endpoint (opposite to the internal endpoint inside [`Transmitter`]/[`Receiver`])
    type Endpoint;
}

/// Extension trait for [`ringbuf::Producer`]
pub trait ProducerExt {
    type Elem;

    /// Push element if there is space available
    fn try_push(&mut self, msg: impl Into<Self::Elem>) -> bool;
}

impl<T, R: RbRef> ProducerExt for Producer<T, R>
where
    R::Rb: RbWrite<T>,
{
    type Elem = T;

    fn try_push(&mut self, msg: impl Into<Self::Elem>) -> bool {
        self.push(msg.into()).is_ok()
    }
}
