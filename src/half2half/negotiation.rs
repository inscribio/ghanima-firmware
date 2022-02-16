use defmt::Format;
use serde::{Serialize, Deserialize};
use smlang::statemachine;

use crate::hal_ext::crc::Crc;
use super::{packet::Packet, SenderQueue, ReceiverQueue};

#[derive(Serialize, Deserialize, Debug, Format, PartialEq)]
pub enum Message {
    EstablishMaster,
    ReleaseMaster,
    Ack,
}

impl Packet for Message {
    // #[cfg(not(test))]
    type Checksum = Crc;
    // #[cfg(test)]
    // type Checksum = crate::hal_ext::checksum_mock::Crc32;
}

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
        // TODO: how to deal with negotiation? (unlikely to happen)
        WantsMaster + EstablishMaster = AsSlave,

        // When releasing master stay as master until slave gets usb
        AsMaster + UsbOff / send_release_master = AsMaster,
        AsMaster + EstablishMaster [no_usb] / send_ack = AsSlave,
        WantsMaster + ReleaseMaster / send_establish_master = WantsMaster,
    }
}

struct Context<TX: SenderQueue<Message>, RX: ReceiverQueue<Message>> {
    sender: TX,
    receiver: RX,
    usb_on: bool,
    // Context sets `timeout`; in tick() we push it to timeout_at
    timeout: bool,
    timeout_value: u32,
    timeout_at: Option<u32>,
}

impl<TX: SenderQueue<Message>, RX: ReceiverQueue<Message>> Context<TX, RX> {
    pub fn new(sender: TX, receiver: RX, timeout: u32) -> Self {
        Self {
            sender,
            receiver,
            timeout: false,
            timeout_value: timeout,
            timeout_at: None,
            usb_on: false
        }
    }
}

impl<TX: SenderQueue<Message>, RX: ReceiverQueue<Message>> StateMachineContext for Context<TX, RX> {
    fn send_ack(&mut self) {
        self.sender.push(Message::Ack);
    }


    fn send_establish_master(&mut self) {
        self.timeout = true;
        self.sender.push(Message::EstablishMaster);
    }

    fn send_release_master(&mut self) {
        self.sender.push(Message::ReleaseMaster);
    }

    // fn has_usb(&mut self) -> Result<(), ()>  {
    //     if self.usb_state { Ok(()) } else { Err(()) }
    // }

    fn no_usb(&mut self) -> Result<(), ()>  {
        if !self.usb_on { Ok(()) } else { Err(()) }
    }
}

impl<TX: SenderQueue<Message>, RX: ReceiverQueue<Message>> StateMachine<Context<TX, RX>> {
    pub fn usb_state(&mut self, on: bool) {
        // Event only on state change
        if self.context.usb_on != on {
            self.process_event(match on {
                true => Events::UsbOn,
                false => Events::UsbOff,
            }).ok();
        }
        self.context.usb_on = on;
    }

    pub fn tick(&mut self, time: u32) {
        // Process any received messages
        while let Some(packet) = self.context.receiver.get() {
            let event = match packet {
                Message::Ack => Some(Events::Ack),
                Message::EstablishMaster => Some(Events::EstablishMaster),
                Message::ReleaseMaster => Some(Events::ReleaseMaster),
            };
            if let Some(event) = event {
                self.process_event(event).ok();
            }
        }

        // Process timeouts
        if self.context.timeout {
            self.context.timeout = false;
            // Ignore any possible past timeouts
            self.context.timeout_at = Some(time + self.context.timeout_value);
        }
        if let Some(timeout_at) = self.context.timeout_at {
            if time >= timeout_at {
                self.context.timeout_at = None;
                self.process_event(Events::Timeout).ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::borrow::BorrowMut;
    use core::marker::PhantomData;
    use std::string::{String, ToString};
    use std::collections::VecDeque;
    use std::cell::RefCell;
    use std::boxed::Box;
    use std::rc::Rc;
    use std::vec::Vec;

    // Mock for tests; connects endpoints as follows:
    //   tx_left -> rx_right
    //   rx_left <- tx_right
    struct Connection {
        // Fields passed to state machines
        pub tx_left: Endpoint<Tx>,
        pub rx_left: Endpoint<Rx>,
        pub tx_right: Endpoint<Tx>,
        pub rx_right: Endpoint<Rx>,
        // Fields that can be used to inject/remove messages
        pub left_to_right: Rc<RefCell<VecDeque<Message>>>,
        pub right_to_left: Rc<RefCell<VecDeque<Message>>>,
    }

    struct Tx;
    struct Rx;

    struct Endpoint<DIR> {
        channel: Rc<RefCell<VecDeque<Message>>>,
        name: String,
        _dir: PhantomData<DIR>,
    }

    impl<DIR> Endpoint<DIR> {
        pub fn new(name: &str, channel: Rc<RefCell<VecDeque<Message>>>) -> Self {
            Self { name: name.to_string(), channel, _dir: PhantomData }
        }
    }

    impl SenderQueue<Message> for Endpoint<Tx> {
        fn push(&mut self, msg: Message) {
            println!("  Push({}: {:?})", self.name, msg);
            self.channel.as_ref().borrow_mut().push_back(msg);
        }
    }

    impl ReceiverQueue<Message> for Endpoint<Rx> {
        fn get(&mut self) -> Option<Message> {
            let msg = self.channel.as_ref().borrow_mut().pop_front();
            if let Some(ref msg) = msg {
                println!("   Pop({}: {:?})", self.name, msg);
            }
            msg
        }
    }

    impl Connection {
        pub fn new() -> Self {
            let left_to_right = Rc::new(RefCell::new(VecDeque::new()));
            let right_to_left = Rc::new(RefCell::new(VecDeque::new()));
            Self {
                tx_left: Endpoint::new("L->R", Rc::clone(&left_to_right)),
                rx_left: Endpoint::new("R->L", Rc::clone(&right_to_left)),
                tx_right: Endpoint::new("R->L", Rc::clone(&right_to_left)),
                rx_right: Endpoint::new("L->R", Rc::clone(&left_to_right)),
                left_to_right,
                right_to_left,
            }
        }
    }

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
        let ch = Connection::new();
        let ctx = Context::new(ch.tx_left, ch.rx_left, 10);
        let mut fsm = StateMachine::new(ctx);
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

    type Fsm = StateMachine<Context<Endpoint<Tx>, Endpoint<Rx>>>;

    enum Dir {
        Left,
        Right,
    }

    enum Step<'a> {
        Tick(States, States),
        DropNext(Dir, Message),
        Inject(Dir, Message),
        Usb(Dir, bool),
        Act(Box<dyn FnOnce(&mut Fsm, &mut Fsm) + 'a>),
    }

    impl<'a> Step<'a> {
        fn act(action: impl FnOnce(&mut Fsm, &mut Fsm) + 'a) -> Self {
            Self::Act(Box::new(action))
        }
    }

    fn scenario<'a, const N: usize>(timeout: u32, steps: [Step<'a>; N]) {
        let ch = Connection::new();
        let mut left = StateMachine::new(Context::new(ch.tx_left, ch.rx_left, timeout));
        let mut right = StateMachine::new(Context::new(ch.tx_right, ch.rx_right, timeout));

        let mut time = 0;
        let mut drop_next = (Vec::new(), Vec::new());
        let mut current_state = (States::AsSlave, States::AsSlave);
        println!("\nState at {}: left={:?} right={:?}", time, left.state(), right.state());
        assert_eq!((left.state(), right.state()), (&current_state.0, &current_state.1));

        let mut drop = |dir: Dir, channel: &mut VecDeque<Message>, to_drop: &mut Vec<Message>| {
            if to_drop.len() == 0 {
                return;
            }
            print!("Drop({}):", match dir {
                Dir::Left => "L->R",
                Dir::Right => "R->L",
            });
            let new: Vec::<_> = channel.drain(..)
                .filter(|msg| {
                    let found = to_drop.iter().find(|d| msg == *d);
                    if let Some(m) = found {
                        print!(" {:?}", m);
                    }
                    found.is_none()
                }).collect();
            println!();
            channel.clear();
            channel.extend(new);
            to_drop.clear();
        };

        for step in steps {
            match step {
                Step::Act(act) => {
                    println!("Act!");
                    act(&mut left, &mut right)
                },
                Step::Usb(dir, on) => {
                    let (text, fsm, mut channel, to_drop) = match dir {
                        Dir::Left => ("left", &mut left, ch.left_to_right.as_ref(), &mut drop_next.0),
                        Dir::Right => ("right", &mut right, ch.right_to_left.as_ref(), &mut drop_next.1),
                    };
                    println!("USB {} {}", text, if on { "on" } else { "off" });
                    fsm.usb_state(on);
                    drop(dir, &mut channel.borrow_mut(), to_drop);
                },
                Step::DropNext(dir, msg) => {
                    match dir {
                        Dir::Left => &mut drop_next.0,
                        Dir::Right => &mut drop_next.1,
                    }.push(msg);
                },
                Step::Inject(dir, msg) => {
                    println!("Inject({}: {:?})", match dir {
                        Dir::Left => "L->R",
                        Dir::Right => "R->L",
                    }, &msg);
                    match dir {
                        Dir::Left => ch.left_to_right.as_ref().borrow_mut().push_back(msg),
                        Dir::Right => ch.right_to_left.as_ref().borrow_mut().push_back(msg),
                    }
                },
                Step::Tick(new_l, new_r) => {
                    time += 1;

                    left.tick(time);
                    println!("State at {}: left={:?} right={:?} [L]", time, left.state(), right.state());

                    drop(Dir::Left, &mut ch.left_to_right.as_ref().borrow_mut(), &mut drop_next.0);

                    right.tick(time);
                    println!("      at {}: left={:?} right={:?} [R]", time, left.state(), right.state());

                    drop(Dir::Right, &mut ch.right_to_left.as_ref().borrow_mut(), &mut drop_next.1);

                    assert_eq!((left.state(), right.state()), (&new_l, &new_r));
                    current_state = (new_l, new_r);
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
            Usb(Left, true),
            DropNext(Left, Message::EstablishMaster),
            Tick(WantsMaster, AsSlave),  // t=1, timeout_at=1+3=4
            Tick(WantsMaster, AsSlave),  // t=2
            Tick(WantsMaster, AsSlave),  // t=3
            Tick(WantsMaster, AsSlave),  // t=4, resends
            Tick(AsMaster, AsSlave),  // t=5
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
            Usb(Right, true),  // R sends EstablishMaster
            Tick(AsMaster, WantsMaster),  // t=6, timeout_at=6+2=8
            Tick(AsMaster, WantsMaster),  // t=7
            Tick(AsMaster, WantsMaster),  // t=8, R resends EstablishMaster
            Tick(AsSlave, AsMaster),
        ]);
    }
}