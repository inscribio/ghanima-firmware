pub mod negotiation;
pub mod packet;
pub mod receiver;
pub mod sender;

type PacketId = u16;

pub trait SenderQueue<P: packet::Packet> {
    fn push(&mut self, packet: P);
}

pub trait ReceiverQueue<P: packet::Packet> {
    fn get(&mut self) -> Option<P>;
}
