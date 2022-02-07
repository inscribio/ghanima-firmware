use defmt::Format;
use serde::{Serialize, Deserialize};
use smlang::statemachine;

use crate::hal_ext::{crc, ChecksumGen};
use super::packet::Packet;

#[derive(Serialize, Deserialize, Format)]
pub enum Message {
    EstablishMaster,
    ReleaseMaster,
    Ack,
}

impl Packet for Message {
    // #[cfg(not(test))]
    type Checksum = crc::Crc;
    // #[cfg(test)]
    // type Checksum = crate::hal_ext::checksum_mock::Crc32;
}

// Events:
// * UsbConnected: should be fed periodically
// * UsbDisconnected
// * EstablishMaster: received Message::EstablishMaster
// * Ack: received Message::Ack
// * Timeout: no Message::Ack after sending Message::EstablishMaster
// States:
// * Undefined: we don't know who we are
// * WaitingAck: we want to become master and wait for Message::Ack
// * AsSlave: we are working as slave
// * AsMaster: we are working as master
statemachine! {
    transitions: {
        *Undefined + UsbConnected / send_establish_master = WaitingAck,
        Undefined + EstablishMaster / send_ack = AsSlave,
        WaitingAck + Ack = AsMaster,
        WaitingAck + Timeout / send_establish_master = WaitingAck,
        WaitingAck + EstablishMaster = Undefined,  // TODO: how to deal with negotiation? (unlikely to happen)
        AsMaster + UsbDisconnected / send_release_master = Undefined,
    }
}

struct Context {

}

impl StateMachineContext for Context {
    fn send_ack(&mut self,) -> () {
        // todo!()
    }


    fn send_establish_master(&mut self,) -> () {
        // todo!()
    }


    fn send_release_master(&mut self,) -> () {
        // todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Clone for States {
        fn clone(&self) -> Self {
            match self {
                States::Undefined => States::Undefined,
                States::WaitingAck => States::WaitingAck,
                States::AsMaster => States::AsMaster,
                States::AsSlave => States::AsSlave,
            }
        }
    }

    impl core::fmt::Debug for States {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let string = match self {
                States::Undefined => "Undefined",
                States::WaitingAck => "WaitingAck",
                States::AsMaster => "AsMaster",
                States::AsSlave => "AsSlave",
            };
            f.debug_struct(string).finish()
        }
    }

    impl core::fmt::Debug for Events {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let string = match self {
                Events::UsbConnected => "UsbConnected",
                Events::EstablishMaster => "EstablishMaster",
                Events::Ack => "Ack",
                Events::Timeout => "Timeout",
                Events::UsbDisconnected => "UsbDisconnected",
            };
            f.debug_struct(string).finish()
        }
    }

    fn test_sequence<const N: usize>(init: States, seq: [(Events, States); N]) {
        let mut fsm = StateMachine::new(Context {});
        assert!(fsm.state() == &init);

        for (event, state) in seq {
            fsm.process_event(event);
            assert_eq!(fsm.state(), &state);
        }
    }

    #[test]
    fn straightforward() {
        test_sequence(States::Undefined, [
            (Events::UsbConnected, States::WaitingAck),
            (Events::Ack, States::AsMaster),
            (Events::UsbDisconnected, States::Undefined),
        ]);
    }
}
