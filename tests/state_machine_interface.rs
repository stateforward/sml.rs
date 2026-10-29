//! `sml::StateMachine` without the `alloc` feature: tables without `outputs`
//! implement it with `Output = ()` and an always-empty drain.

use sml::{sml, Machine, StateMachine, StateName};

pub struct Ping;

sml! {
    Quiet {
        *Idle + on_entry<_> / entered,
         Idle + event<Ping> = Busy,
         Busy + event<Ping> = Idle,
    }
}

#[derive(Default)]
pub struct QuietContext {
    entries: u8,
}

impl QuietStateMachineContext for QuietContext {
    fn entered(&mut self) -> Result<(), ()> {
        self.entries += 1;
        Ok(())
    }
}

type QuietLabel = <QuietStateMachine<QuietContext> as StateMachine>::Label;

#[test]
fn a_table_without_outputs_names_itself_and_drains_nothing() {
    let mut quiet = QuietStateMachine::new(QuietContext::default());
    assert_eq!(
        <QuietStateMachine<QuietContext> as StateMachine>::NAME,
        "Quiet"
    );
    assert!(StateMachine::initialize(&mut quiet));
    assert_eq!(quiet.context().entries, 1);
    assert!(StateMachine::process(&mut quiet, Ping.into()));
    assert_eq!(StateMachine::label(&quiet), QuietLabel::Busy);
    assert_eq!(StateMachine::label(&quiet).name(), "Busy");
    let mut drain = StateMachine::drain_outputs(&mut quiet);
    assert_eq!(drain.size_hint(), (0, Some(0)));
    assert!(drain.next().is_none());
    assert!(format!("{drain:?}").contains("Drain"));
    drop(drain);
    assert!(!StateMachine::stop_requested(&quiet));
    assert!(sml::Drain::<u8>::empty().next().is_none());
}

// A qualified leaf name that a table already uses gets a numeric suffix.
pub struct Go;

sml! {
    Inner {
        *Deep + event<Go> = X,
    }

    Outer {
        *Outside + event<Go> = state<Inner>,
         state<Inner> = InnerX,
         InnerX + event<Go> = X,
    }
}

pub struct OuterContext;

impl OuterStateMachineContext for OuterContext {}

type OuterLabel = <OuterStateMachine<OuterContext> as StateMachine>::Label;

#[test]
fn a_taken_qualified_label_gets_a_numeric_suffix() {
    let mut outer = OuterStateMachine::new(OuterContext);
    assert!(StateMachine::process(&mut outer, Go.into()));
    assert_eq!(StateMachine::label(&outer), OuterLabel::Deep);
    assert!(StateMachine::process(&mut outer, Go.into()));
    assert_eq!(StateMachine::label(&outer), OuterLabel::InnerX);
    assert!(StateMachine::process(&mut outer, Go.into()));
    assert_eq!(StateMachine::label(&outer), OuterLabel::OuterX);
    assert_eq!(OuterLabel::InnerX2.name(), "InnerX2");
    assert!(!Machine::<OuterEvents>::process_event(
        &mut outer,
        Go.into()
    ));
}

// Events that borrow a lifetime shared with the states keep their inherent
// API; the table still compiles and does not implement `StateMachine`.
pub struct Borrowed<'a>(pub &'a str);

#[derive(Default)]
pub struct Held<'a>(pub &'a str);

sml! {
    Lending {
        *Empty + event<Borrowed<'a>> / hold = Holding(Held<'a>),
         Holding(Held<'a>) + event<Borrowed<'a>> = Empty,
    }
}

pub struct LendingContext;

impl LendingStateMachineContext for LendingContext {
    fn hold<'a>(&mut self, event: &Borrowed<'a>) -> Result<Held<'a>, ()> {
        Ok(Held(event.0))
    }
}

#[test]
fn tables_with_borrowed_events_keep_their_inherent_api() {
    let text = String::from("lent");
    let mut lending = LendingStateMachine::new(LendingContext);
    lending.process_event(Borrowed(&text)).unwrap();
    assert!(matches!(
        lending.state(),
        LendingStates::Holding(Held("lent"))
    ));
}
