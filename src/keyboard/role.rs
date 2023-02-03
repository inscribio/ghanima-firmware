use smlang::statemachine;
use serde::{Serialize, Deserialize};
use postcard::experimental::max_size::MaxSize;
use defmt::Format;

use crate::bsp::sides::BoardSide;

pub type Fsm = StateMachine<Context>;

/// Role negotiation messages
#[derive(Serialize, Deserialize, MaxSize, Format, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub enum Message {
    /// Used to request establishing master role when USB is on
    EstablishMaster,
    /// Signalize that USB connection is lost and master state can be released
    ReleaseMaster,
    /// Acknowledge other board's EstablishMaster request
    Ack,
}

/// Describes current role of keyboard half
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum Role {
    /// Board should act as master: process keyboard events, send USB HID reports,
    /// send commands to slave over serial, etc.
    Master,
    /// Board should act as slave: transmit key press/release coordinates to master
    /// over serial, respond to commands from master, etc.
    Slave,
}

// FIXME: sometimes both end up thinking they are masters?
// scenario: connect right, connect left, disconnect right, connect right
statemachine! {
    transitions: {
        // Both sides starts as slaves
        *AsSlave + UsbOn / send_establish_master = WantsMaster,

        // Acknowledge
        AsSlave + EstablishMaster / send_ack = AsSlave,

        // Trying to acquire master
        WantsMaster + UsbOff = AsSlave,
        WantsMaster + Ack = AsMaster,
        WantsMaster + Timeout / send_establish_master = WantsMaster,
        WantsMaster + EstablishMaster [resign] = AsSlave,

        // When releasing master stay as master until slave gets usb
        AsMaster + UsbOff / send_release_master = AsMaster,
        AsMaster + EstablishMaster [no_usb] / send_ack = AsSlave,
        WantsMaster + ReleaseMaster / send_establish_master = WantsMaster,
    }
}

pub struct Context {
    usb_on: bool,
    is_alone: bool,
    side: BoardSide,
    message: Option<Message>,
    timeout: u32,
    timeout_cnt: Option<u32>,
}

impl Context {
    fn send(&mut self, message: Message) {
        let prev = self.message.replace(message);
        debug_assert!(prev.is_none(), "TX message not consumed since last send");
    }

    fn start_timeout(&mut self) {
        self.timeout_cnt = Some(self.timeout);
    }
}

impl StateMachineContext for Context {
    fn send_ack(&mut self) {
        defmt::info!("Send Ack");
        self.send(Message::Ack);
    }

    fn send_establish_master(&mut self) {
        defmt::info!("Send EstablishMaster");
        self.start_timeout();
        self.send(Message::EstablishMaster);
    }

    fn send_release_master(&mut self) {
        defmt::info!("Send ReleaseMaster");
        self.send(Message::ReleaseMaster);
    }

    fn no_usb<'a>(&mut self) -> Result<(), ()>  {
        if !self.usb_on { Ok(()) } else { Err(()) }
    }

    fn resign(&mut self) -> Result<(), ()>  {
        match self.side {
            BoardSide::Left => Err(()),
            BoardSide::Right => Ok(()),
        }
    }

}

impl StateMachine<Context> {
    /// Construct state machine with timeout of given number of ticks
    pub fn with(side: BoardSide, timeout: u32) -> Self {
        Self::new(Context {
            usb_on: false,
            is_alone: false,
            side,
            message: None,
            timeout_cnt: None,
            timeout,
        })
    }

    /// Inform about current USB state; to be called periodically
    pub fn usb_state(&mut self, on: bool) -> Option<Message> {
        // Event only on state change
        if self.context.usb_on != on {
            defmt::info!("USB {=bool}", on);
            self.process_event(match on {
                true => Events::UsbOn,
                false => Events::UsbOff,
            }).ok();
        }
        self.context.usb_on = on;
        self.context.message.take()
    }

    /// Process recived message
    pub fn on_rx(&mut self, message: Message) -> Option<Message> {
        // If we received something than there is a transmitter
        self.context.is_alone = false;
        let event = match message {
            Message::Ack => Events::Ack,
            Message::EstablishMaster => Events::EstablishMaster,
            Message::ReleaseMaster => Events::ReleaseMaster,
        };
        self.process_event(event).ok();
        self.context.message.take()
    }

    /// Advance time by one tick
    pub fn tick(&mut self) -> Option<Message> {
        // If timeout hasn't been set then nothing to do
        let cnt = self.context.timeout_cnt.take()?;
        if cnt == 0 {
            // Timeout event, may return a message
            self.context.is_alone = true;
            self.process_event(Events::Timeout).ok();
            self.context.message.take()
        } else {
            // Time ticks
            self.context.timeout_cnt = Some(cnt - 1);
            None
        }
    }

    /// Get current role of this board
    pub fn role(&self) -> Role {
        match *self.state() {
            States::AsMaster => Role::Master,
            States::WantsMaster if self.context.is_alone => Role::Master,
            _ => Role::Slave,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Display;
    use std::collections::VecDeque;
    use std::vec::Vec;

    impl Clone for States {
        fn clone(&self) -> Self {
            match self {
                States::AsSlave => States::AsSlave,
                States::WantsMaster => States::WantsMaster,
                States::AsMaster => States::AsMaster,
            }
        }
    }

    impl core::fmt::Debug for States {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let string = match self {
                States::AsSlave => "AsSlave",
                States::WantsMaster => "WantsMaster",
                States::AsMaster => "AsMaster",
            };
            f.debug_struct(string).finish()
        }
    }

    impl core::fmt::Debug for Events {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let string = match self {
                Events::UsbOn => "UsbOn",
                Events::UsbOff => "UsbOff",
                Events::EstablishMaster => "EstablishMaster",
                Events::ReleaseMaster => "ReleaseMaster",
                Events::Timeout => "Timeout",
                Events::Ack => "Ack",
            };
            f.debug_struct(string).finish()
        }
    }

    fn events_seq<const N: usize>(init: States, seq: [(Events, States); N]) {
        let mut fsm = Fsm::with(BoardSide::Left, 10);
        assert!(fsm.state() == &init);
        println!();
        for (event, state) in seq {
            assert_eq!(fsm.process_event(event).unwrap(), &state);
        }
    }

    #[test]
    fn basic_events_sequence() {
        events_seq(States::AsSlave, [
            (Events::UsbOn, States::WantsMaster),
            (Events::Ack, States::AsMaster),
        ]);
    }

    // Mock for tests with simulation of 2 boards
    #[derive(Default)]
    struct Connection {
        pub left_to_right: VecDeque<Message>,
        pub right_to_left: VecDeque<Message>,
    }

    #[derive(Clone, Copy)]
    enum Dir {
        Left,
        Right,
    }

    impl Display for Dir {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self {
                Dir::Left => f.write_str("left"),
                Dir::Right => f.write_str("right"),
            }
        }
    }

    enum Step {
        Tick(States, States),
        DropNext(Dir, Message),
        DropNextAll(Dir),
        #[allow(dead_code)]
        Inject(Dir, Message),
        Usb(Dir, bool),
    }

    fn scenario<const N: usize>(timeout: u32, steps: [Step; N]) {
        let mut ch = Connection::default();
        let mut left = Fsm::with(BoardSide::Left, timeout);
        let mut right = Fsm::with(BoardSide::Right, timeout);

        let mut time = 0;
        let mut drop_next = (Vec::new(), Vec::new());
        println!("\nState at {}: left={:?} right={:?}", time, left.state(), right.state());
        assert_eq!((left.state(), right.state()), (&States::AsSlave, &States::AsSlave));

        let drop = |dir: Dir, to_drop: &mut Vec<Message>, msg: Message| -> Option<Message> {
            if to_drop.len() == 0 {
                return Some(msg);
            }
            print!("Drop({}):", dir);
            let found =  to_drop.iter().find(|m| &msg == *m);
            let msg = if let Some(m) = found {
                print!(" {:?}", m);
                None
            } else {
                Some(msg)
            };
            println!();
            to_drop.clear();
            msg
        };

        let maybe_tx = |dir: Dir, channel: &mut VecDeque<Message>, to_drop: &mut Vec<Message>, msg: Option<Message>| {
            if let Some(msg) = msg.and_then(|msg| drop(dir, to_drop, msg)) {
                println!("Push {} {:?}", dir, msg);
                channel.push_back(msg);
            }
            to_drop.clear();
        };

        for step in steps {
            match step {
                Step::Usb(dir, on) => {
                    println!("USB {} {}", dir, if on { "on" } else { "off" });
                    let (fsm, ch, to_drop) = match dir {
                        Dir::Left => (&mut left, &mut ch.left_to_right, &mut drop_next.0),
                        Dir::Right => (&mut right, &mut ch.right_to_left, &mut drop_next.1),
                    };
                    maybe_tx(dir, ch, to_drop, fsm.usb_state(on));
                },
                Step::DropNext(dir, msg) => {
                    match dir {
                        Dir::Left => &mut drop_next.0,
                        Dir::Right => &mut drop_next.1,
                    }.push(msg);
                },
                Step::DropNextAll(dir) => {
                    let msgs = [
                        Message::EstablishMaster,
                        Message::ReleaseMaster,
                        Message::Ack,
                    ];
                    for msg in msgs {
                        match dir {
                            Dir::Left => &mut drop_next.0,
                            Dir::Right => &mut drop_next.1,
                        }.push(msg);
                    }
                },
                Step::Inject(dir, msg) => {
                    println!("Inject({}: {:?})", match dir {
                        Dir::Left => "L->R",
                        Dir::Right => "R->L",
                    }, &msg);
                    match dir {
                        Dir::Left => ch.left_to_right.push_back(msg),
                        Dir::Right => ch.right_to_left.push_back(msg),
                    }
                },
                Step::Tick(new_l, new_r) => {
                    time += 1;

                    let dirs = [Dir::Left, Dir::Right];

                    // RX data
                    for dir in dirs.iter() {
                        // msgs from opposite e.g. rx(right) => ch=left_to_right
                        let (msgs, fsm, ch, to_drop) = match dir {
                            Dir::Left => (&mut ch.right_to_left, &mut left, &mut ch.left_to_right, &mut drop_next.0),
                            Dir::Right => (&mut ch.left_to_right, &mut right, &mut ch.right_to_left, &mut drop_next.1),
                        };
                        for msg in msgs.drain(..) {
                            println!("Pop {} {:?}", dir, msg);
                            maybe_tx(*dir, ch, to_drop, fsm.on_rx(msg));
                        }
                    }

                    println!("State at {}: left={:?} right={:?} [RX]", time, left.state(), right.state());

                    // Tick
                    for dir in dirs.iter() {
                        let (fsm, ch, to_drop) = match dir {
                            Dir::Left => (&mut left, &mut ch.left_to_right, &mut drop_next.0),
                            Dir::Right => (&mut right, &mut ch.right_to_left, &mut drop_next.1),
                        };
                        maybe_tx(*dir, ch, to_drop, fsm.tick());
                    }

                    println!("      at {}: left={:?} right={:?} [TX]", time, left.state(), right.state());

                    assert_eq!((left.state(), right.state()), (&new_l, &new_r));
                },
            }
        }
    }

    use Step::*;
    use Dir::*;
    use States::*;

    #[test]
    fn basic_establish_master() {
        scenario(3, [
            Tick(AsSlave, AsSlave),
            Usb(Left, true),
            Tick(WantsMaster, AsSlave),
            Tick(AsMaster, AsSlave),
        ]);
    }

    #[test]
    fn found_usb_as_slave() {
        scenario(3, [
            Tick(AsSlave, AsSlave),
            Usb(Left, true),
            Tick(WantsMaster, AsSlave),
            Tick(AsMaster, AsSlave),
            Usb(Right, true),
            Tick(AsMaster, WantsMaster),
            Tick(AsMaster, WantsMaster),
        ]);
    }

    #[test]
    fn swap_usb_master() {
        scenario(3, [
            Tick(AsSlave, AsSlave),
            Usb(Left, true),
            Tick(WantsMaster, AsSlave),
            Tick(AsMaster, AsSlave),
            Usb(Right, true),
            Tick(AsMaster, WantsMaster),
            Tick(AsMaster, WantsMaster),
            Usb(Left, false),
            Tick(AsMaster, WantsMaster),
            Tick(AsSlave, AsMaster),
            Tick(AsSlave, AsMaster),
        ]);
    }

    #[test]
    fn establish_master_timeout() {
        scenario(3, [
            Tick(AsSlave, AsSlave),
            DropNext(Left, Message::EstablishMaster),
            Usb(Left, true),  // L sends, timeout=3
            Tick(WantsMaster, AsSlave),  // 3 -> 2
            Tick(WantsMaster, AsSlave),  // -> 1
            Tick(WantsMaster, AsSlave),  // -> 0
            Tick(WantsMaster, AsSlave),  // 0, L resends
            Tick(WantsMaster, AsSlave),  // R sends Ack
            Tick(AsMaster, AsSlave),
            Tick(AsMaster, AsSlave),
        ]);
    }

    #[test]
    fn lost_usb_as_master_swap_later() {
        scenario(3, [
            Tick(AsSlave, AsSlave),
            Usb(Left, true),
            Tick(WantsMaster, AsSlave),
            Tick(AsMaster, AsSlave),
            Usb(Left, false),  // L sends ReleaseMaster
            Tick(AsMaster, AsSlave),
            Tick(AsMaster, AsSlave),
            Usb(Right, true),  // R sends EstablishMaster
            Tick(AsSlave, AsMaster),  // L reads EstablishMaster, pushes Ack; R reads Ack
        ]);
    }

    #[test]
    fn lost_usb_as_master_swap_later_with_timeout() {
        scenario(2, [
            Tick(AsSlave, AsSlave),
            Usb(Left, true),
            Tick(WantsMaster, AsSlave),
            Tick(AsMaster, AsSlave),
            Usb(Left, false),  // L sends ReleaseMaster
            Tick(AsMaster, AsSlave),
            Tick(AsMaster, AsSlave),
            DropNext(Right, Message::EstablishMaster),
            Usb(Right, true),  // R sends EstablishMaster, t=2
            Tick(AsMaster, WantsMaster),  // -> 1
            Tick(AsMaster, WantsMaster),  // -> 0
            Tick(AsMaster, WantsMaster),  // 0, R resends EstablishMaster
            Tick(AsSlave, AsMaster),
        ]);
    }


    #[test]
    fn one_half_connected_later() {
        // Simulate disconnecting right half by dropping all messages from left half.
        // That's basically just testing timeout and resending EstablishMaster.
        scenario(2, [
            Tick(AsSlave, AsSlave),
            DropNextAll(Left),
            Usb(Left, true),  // send, dropped, t=2
            DropNextAll(Left),
            Tick(WantsMaster, AsSlave), // -> 1
            DropNextAll(Left),
            Tick(WantsMaster, AsSlave), // -> 0
            DropNextAll(Left),
            Tick(WantsMaster, AsSlave), // 0, resends, t=2
            DropNextAll(Left),
            Tick(WantsMaster, AsSlave), // -> 1
            DropNextAll(Left),
            Tick(WantsMaster, AsSlave), // -> 0
            // Right half connected, stop dropping messages.
            Tick(WantsMaster, AsSlave), // 0, resends
            Tick(WantsMaster, AsSlave), // Ack
            Tick(AsMaster, AsSlave),
            Tick(AsMaster, AsSlave),
        ]);
    }

    #[test]
    fn usb_both_resolved_for_left() {
        scenario(2, [
            Tick(AsSlave, AsSlave),
            Usb(Left, true),  // L sends, tL=2
            Usb(Right, true),  // R sends, tR=2
            Tick(WantsMaster, AsSlave),  // both receive, R resigns, tL->1
            Tick(WantsMaster, AsSlave),  // tL->0
            Tick(WantsMaster, AsSlave),  // tL=0, L resends
            Tick(WantsMaster, AsSlave),  // R sends Ack
            Tick(AsMaster, AsSlave),
            Tick(AsMaster, AsSlave),
            Tick(AsMaster, AsSlave),
            Tick(AsMaster, AsSlave),
        ]);
    }
}
