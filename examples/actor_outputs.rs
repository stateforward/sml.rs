//! An actor shell driving any generated machine through `sml::StateMachine`:
//! actions emit outputs instead of doing I/O, the shell journals each event
//! with the machine's label around it, executes the drained outputs, and
//! stops when an action asks to. Replaying the journal into a fresh machine
//! reproduces every label.

use sml::{sml, Outbox, StateMachine, StateName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub item: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shipped;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Close;

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Reserve(&'static str),
    Notify(&'static str),
}

sml! {
    Fulfil {
        *Reserving + event<Shipped> / notify_shipped = X,
    }

    Shop[
        name: "shop",
        outputs: Command,
        events_attr: #[derive(Debug, Clone, PartialEq, Eq)]
    ] {
        *Open + event<Order> / reserve = state<Fulfil>,
         state<Fulfil> / settle = Open,
         Open + event<Close> / close = X,
    }
}

#[derive(Default)]
pub struct Shop {
    orders: u32,
}

impl ShopStateMachineContext for Shop {
    fn reserve(&mut self, outbox: &mut Outbox<Command>, order: &Order) -> Result<(), ()> {
        self.orders += 1;
        outbox.emit(Command::Reserve(order.item));
        Ok(())
    }

    fn notify_shipped(&mut self, outbox: &mut Outbox<Command>, _event: &Shipped) -> Result<(), ()> {
        outbox.emit(Command::Notify("shipped"));
        Ok(())
    }

    fn settle(&mut self, _outbox: &mut Outbox<Command>) -> Result<(), ()> {
        Ok(())
    }

    fn close(&mut self, outbox: &mut Outbox<Command>, _event: &Close) -> Result<(), ()> {
        outbox.stop();
        Ok(())
    }
}

/// One journal line: the event and the machine's label after it.
struct Entry<M: StateMachine> {
    event: M::Events,
    after: M::Label,
}

/// Runs `events` through `machine` like an actor shell would.
fn run<M>(machine: &mut M, events: Vec<M::Events>) -> Vec<Entry<M>>
where
    M: StateMachine,
    M::Events: Clone,
    M::Output: core::fmt::Debug,
{
    let mut journal = Vec::new();
    for event in events {
        let accepted = machine.process(event.clone());
        journal.push(Entry {
            event,
            after: machine.label(),
        });
        for output in machine.drain_outputs() {
            println!("[{}] execute {output:?}", M::NAME);
        }
        println!(
            "[{}] accepted={accepted} now {}",
            M::NAME,
            machine.label().name()
        );
        if machine.stop_requested() {
            println!("[{}] stopping", M::NAME);
            break;
        }
    }
    journal
}

fn main() {
    let mut live = ShopStateMachine::new(Shop::default());
    let journal = run(
        &mut live,
        vec![Order { item: "book" }.into(), Shipped.into(), Close.into()],
    );
    assert_eq!(live.context().orders, 1);

    let mut replayed = ShopStateMachine::new(Shop::default());
    for entry in journal {
        replayed.process(entry.event);
        let _ = replayed.drain_outputs();
        assert_eq!(replayed.label(), entry.after);
    }
    println!("replay matched every label");
}
