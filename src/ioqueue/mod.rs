mod packet;
mod receiver;
mod transmitter;

pub use packet::Packet;
pub use receiver::Receiver;
pub use transmitter::Transmitter;

type PacketId = u16;

pub trait TransmitQueue<P: Packet> {
    fn push(&mut self, packet: P);
}

pub trait ReceiveQueue<P: Packet> {
    fn get(&mut self) -> Option<P>;
}
