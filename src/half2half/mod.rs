mod fsm;
mod packet;
mod receiver;
mod transmitter;

use packet::Packet;
use fsm::Message;

type PacketId = u16;

pub trait TransmitQueue<P: Packet> {
    fn push(&mut self, packet: P);
}

pub trait ReceiveQueue<P: Packet> {
    fn get(&mut self) -> Option<P>;
}

pub type Transmitter<TX, const N: usize> = transmitter::Transmitter<Message, TX, N>;
pub type Receiver<RX, const N: usize, const B: usize> = receiver::Receiver<Message, RX, N, B>;
pub use fsm::Fsm;
