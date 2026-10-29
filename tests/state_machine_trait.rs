//! The 1.5 `sml::StateMachine` interface: uniform processing, leaf labels,
//! names, machine-owned action outputs, and journalable generated events.
#![cfg(feature = "alloc")]

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sml::{sml, Outbox, StateMachine, StateName};

// ---------------------------------------------------------------------------
// A flat table with outputs, a name, and journalable events.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Open {
    pub id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Close;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fail;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DoorOutput {
    Opened(u32),
    Entered(&'static str),
    Closed,
    Recovered,
}

sml! {
    Door[
        name: "door",
        outputs: DoorOutput,
        events_attr: #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)],
        labels_attr: #[derive(Serialize, Deserialize)]
    ] {
        *Shut + event<Open> [allowed] / (record_open, eval [loud] / shout) = Ajar,
         Ajar + on_entry<_> / entered,
         Ajar + event<Close> / close = Shut,
         Ajar + event<Fail> / fail = Ajar,
         Ajar + exception<_> / recover = Done,
         Done + event<Close> / halt = X,
    }
}

#[derive(Default)]
pub struct DoorContext {
    opened: u32,
}

impl DoorStateMachineContext for DoorContext {
    fn allowed(&self, event: &Open) -> Result<bool, ()> {
        Ok(event.id != 0)
    }

    fn loud(&self, event: &Open) -> Result<bool, ()> {
        Ok(event.id > 10)
    }

    fn record_open(&mut self, outbox: &mut Outbox<DoorOutput>, event: &Open) -> Result<(), ()> {
        self.opened += 1;
        outbox.emit(DoorOutput::Opened(event.id));
        Ok(())
    }

    fn shout(&mut self, outbox: &mut Outbox<DoorOutput>, _event: &Open) -> Result<(), ()> {
        outbox.emit(DoorOutput::Entered("loud"));
        Ok(())
    }

    fn entered(&mut self, outbox: &mut Outbox<DoorOutput>) -> Result<(), ()> {
        outbox.emit(DoorOutput::Entered("ajar"));
        Ok(())
    }

    fn close(&mut self, outbox: &mut Outbox<DoorOutput>, _event: &Close) -> Result<(), ()> {
        outbox.emit(DoorOutput::Closed);
        Ok(())
    }

    fn fail(&mut self, _outbox: &mut Outbox<DoorOutput>, _event: &Fail) -> Result<(), ()> {
        Err(())
    }

    fn recover(&mut self, outbox: &mut Outbox<DoorOutput>) -> Result<(), ()> {
        outbox.emit(DoorOutput::Recovered);
        Ok(())
    }

    fn halt(&mut self, outbox: &mut Outbox<DoorOutput>, _event: &Close) -> Result<(), ()> {
        outbox.stop();
        Ok(())
    }
}

type DoorLabel = <DoorStateMachine<DoorContext> as StateMachine>::Label;

fn drained<M: StateMachine>(machine: &mut M) -> Vec<M::Output> {
    machine.drain_outputs().collect()
}

#[test]
fn flat_machine_names_labels_and_drains_outputs_in_emit_order() {
    assert_eq!(
        <DoorStateMachine<DoorContext> as StateMachine>::NAME,
        "door"
    );
    let mut door = DoorStateMachine::new(DoorContext::default());
    assert_eq!(StateMachine::label(&door), DoorLabel::Shut);
    assert_eq!(StateMachine::label(&door).name(), "Shut");

    assert!(!StateMachine::process(&mut door, Open { id: 0 }.into()));
    assert!(drained(&mut door).is_empty());

    assert!(StateMachine::process(&mut door, Open { id: 11 }.into()));
    assert_eq!(StateMachine::label(&door), DoorLabel::Ajar);
    assert_eq!(door.outbox().pending().len(), 3);
    assert_eq!(
        drained(&mut door),
        vec![
            DoorOutput::Opened(11),
            DoorOutput::Entered("loud"),
            DoorOutput::Entered("ajar"),
        ]
    );
    assert!(door.outbox().pending().is_empty());
    assert_eq!(StateMachine::context(&door).opened, 1);
    StateMachine::context_mut(&mut door).opened = 5;
    assert_eq!(door.context().opened, 5);

    assert!(StateMachine::process(&mut door, Fail.into()));
    assert_eq!(StateMachine::label(&door), DoorLabel::Done);
    assert_eq!(drained(&mut door), vec![DoorOutput::Recovered]);

    assert!(!StateMachine::stop_requested(&door));
    assert!(StateMachine::process(&mut door, Close.into()));
    assert_eq!(StateMachine::label(&door), DoorLabel::X);
    assert!(StateMachine::stop_requested(&door));
    assert!(drained(&mut door).is_empty());
    assert!(
        StateMachine::stop_requested(&door),
        "a stop request is sticky"
    );
}

const fn journalable<T: Clone + core::fmt::Debug + PartialEq + Serialize + DeserializeOwned>() {}

#[test]
fn derived_events_and_labels_are_journalable_and_compare_payloads() {
    journalable::<DoorEvents>();
    journalable::<DoorLabel>();
    assert_eq!(
        DoorEvents::from(Open { id: 1 }),
        DoorEvents::from(Open { id: 1 })
    );
    // A derived `PartialEq` replaces the variant-only comparison.
    assert_ne!(
        DoorEvents::from(Open { id: 1 }),
        DoorEvents::from(Open { id: 2 })
    );

    // Journal the generated events, then replay them into a fresh machine and
    // compare snapshots (label plus the caller's digest of its context).
    let journal = vec![
        DoorEvents::from(Open { id: 3 }),
        DoorEvents::from(Close),
        DoorEvents::from(Open { id: 4 }),
    ];
    let mut live = DoorStateMachine::new(DoorContext::default());
    let mut snapshots = Vec::new();
    for event in journal.clone() {
        StateMachine::process(&mut live, event);
        let _ = drained(&mut live);
        snapshots.push((StateMachine::label(&live), live.context().opened));
    }
    let mut replayed = DoorStateMachine::new(DoorContext::default());
    for (event, snapshot) in journal.into_iter().zip(snapshots) {
        StateMachine::process(&mut replayed, event);
        assert_eq!(
            (StateMachine::label(&replayed), replayed.context().opened),
            snapshot
        );
    }
}

// ---------------------------------------------------------------------------
// A composite: a parent hosting one child table.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Start;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step(pub u8);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stop;

sml! {
    Work {
        *Planning + event<Step> [first] / plan = Running,
         Running + completion<Step> / ran = X,
    }

    Job[
        outputs: u8,
        events_attr: #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)],
        states_attr: #[derive(Debug)]
    ] {
        *Idle + event<Start> / begin = state<Work>,
         state<Work> + event<Stop> / end = X,
         state<Work> / finished = Idle,
    }
}

#[derive(Default)]
pub struct JobContext;

impl JobStateMachineContext for JobContext {
    fn first(&self, event: &Step) -> Result<bool, ()> {
        Ok(event.0 == 1)
    }

    fn plan(&mut self, outbox: &mut Outbox<u8>, event: &Step) -> Result<(), ()> {
        outbox.emit(event.0);
        Ok(())
    }

    fn ran(&mut self, outbox: &mut Outbox<u8>, event: &Step) -> Result<(), ()> {
        outbox.emit(event.0.saturating_add(10));
        Ok(())
    }

    fn begin(&mut self, outbox: &mut Outbox<u8>, _event: &Start) -> Result<(), ()> {
        outbox.emit(0);
        Ok(())
    }

    fn end(&mut self, outbox: &mut Outbox<u8>, _event: &Stop) -> Result<(), ()> {
        outbox.stop();
        Ok(())
    }

    fn finished(&mut self, outbox: &mut Outbox<u8>) -> Result<(), ()> {
        outbox.emit(99);
        Ok(())
    }
}

type JobLabel = <JobStateMachine<JobContext> as StateMachine>::Label;

#[test]
fn composite_label_names_the_active_leaf_and_qualifies_shared_names() {
    let mut job = JobStateMachine::new(JobContext);
    assert_eq!(<JobStateMachine<JobContext> as StateMachine>::NAME, "Job");
    assert_eq!(StateMachine::label(&job), JobLabel::Idle);

    assert!(StateMachine::process(&mut job, Start.into()));
    assert_eq!(StateMachine::label(&job), JobLabel::Planning);
    assert_eq!(drained(&mut job), vec![0]);

    // Both tables have an `X`, so each is qualified by its table name.
    assert!(StateMachine::process(&mut job, Step(1).into()));
    assert_eq!(StateMachine::label(&job), JobLabel::Idle);
    assert_eq!(drained(&mut job), vec![1, 11, 99]);
    assert_eq!(JobLabel::WorkX.name(), "WorkX");
    assert_eq!(JobLabel::JobX.name(), "JobX");

    assert!(StateMachine::process(&mut job, Start.into()));
    assert!(StateMachine::process(&mut job, Stop.into()));
    assert_eq!(StateMachine::label(&job), JobLabel::JobX);
    assert!(StateMachine::stop_requested(&job));
    assert_eq!(drained(&mut job), vec![0]);
}

#[test]
fn composite_applies_the_parent_attrs_and_implements_machine() {
    journalable::<JobEvents>();
    assert_eq!(format!("{:?}", JobStates::Idle), "Idle");
    let mut job = JobStateMachine::new(JobContext);
    assert!(sml::Machine::<JobEvents>::process_event(
        &mut job,
        Start.into()
    ));
    assert!(!sml::Machine::<JobEvents>::process_event(
        &mut job,
        Start.into()
    ));
    assert!(job.is(&JobStates::Work));
}

// ---------------------------------------------------------------------------
// A composite tree: a grandchild under a child.

pub struct Go;
pub struct Leave;

sml! {
    Inner {
        *Deep + event<Go> = X,
    }

    Middle {
        *Shallow + event<Go> = state<Inner>,
         state<Inner> / done = X,
    }

    Outer[name: "outer", outputs: &'static str] {
        *Outside + event<Go> / enter = state<Middle>,
         state<Middle> + event<Leave> = X,
    }
}

pub struct OuterContext;

impl OuterStateMachineContext for OuterContext {
    fn enter(&mut self, outbox: &mut Outbox<&'static str>, _event: &Go) -> Result<(), ()> {
        outbox.emit("enter");
        Ok(())
    }

    fn done(&mut self, outbox: &mut Outbox<&'static str>) -> Result<(), ()> {
        outbox.emit("done");
        Ok(())
    }
}

type OuterLabel = <OuterStateMachine<OuterContext> as StateMachine>::Label;

#[test]
fn composite_tree_labels_the_deepest_active_state() {
    let mut outer = OuterStateMachine::new(OuterContext);
    assert_eq!(
        <OuterStateMachine<OuterContext> as StateMachine>::NAME,
        "outer"
    );
    assert_eq!(StateMachine::label(&outer), OuterLabel::Outside);
    assert!(StateMachine::process(&mut outer, Go.into()));
    assert_eq!(StateMachine::label(&outer), OuterLabel::Shallow);
    assert!(StateMachine::process(&mut outer, Go.into()));
    assert_eq!(StateMachine::label(&outer), OuterLabel::Deep);
    assert!(StateMachine::process(&mut outer, Go.into()));
    assert_eq!(StateMachine::label(&outer), OuterLabel::MiddleX);
    assert_eq!(drained(&mut outer), vec!["enter", "done"]);
    assert!(StateMachine::process(&mut outer, Leave.into()));
    assert_eq!(StateMachine::label(&outer), OuterLabel::OuterX);
    assert_eq!(OuterLabel::InnerX.name(), "InnerX");
    assert!(!sml::Machine::<OuterEvents>::process_event(
        &mut outer,
        Leave.into()
    ));
}

// ---------------------------------------------------------------------------
// Orthogonal regions have several active leaves, so they keep their own API,
// but their actions can still emit through the outbox.

pub struct Tick;

sml! {
    Pair[outputs: u8] {
        *Left + event<Tick> / left = X,
        *Right + event<Tick> / right = X,
    }
}

pub struct PairContext;

impl PairStateMachineContext for PairContext {
    fn left(&mut self, outbox: &mut Outbox<u8>, _event: &Tick) -> Result<(), ()> {
        outbox.emit(1);
        Ok(())
    }

    fn right(&mut self, outbox: &mut Outbox<u8>, _event: &Tick) -> Result<(), ()> {
        outbox.emit(2);
        Ok(())
    }
}

#[test]
fn orthogonal_actions_emit_through_the_inherent_outbox() {
    let mut pair = PairStateMachine::new(PairContext);
    pair.process_event(Tick).unwrap();
    assert_eq!(pair.outbox().pending(), &[1, 2]);
    assert_eq!(pair.outbox_mut().drain().collect::<Vec<_>>(), vec![1, 2]);
    assert!(!pair.outbox().stop_requested());
}

#[test]
fn outbox_is_a_plain_value() {
    let mut outbox = Outbox::<u8>::default();
    assert_eq!(outbox, Outbox::new());
    outbox.emit(7);
    let copy = outbox.clone();
    assert_eq!(copy.pending(), &[7]);
    assert!(format!("{outbox:?}").contains('7'));
    let mut drain = outbox.drain();
    assert_eq!(drain.size_hint(), (1, Some(1)));
    assert_eq!(drain.next(), Some(7));
}

// ---------------------------------------------------------------------------
// Async callbacks keep their inherent API; their actions emit the same way.

pub struct Fetch;

sml! {
    Loader[outputs: u8] {
        *Idle + event<Fetch> / async load = Loaded,
    }
}

pub struct LoaderContext;

impl LoaderStateMachineContext for LoaderContext {
    async fn load(&mut self, outbox: &mut Outbox<u8>, _event: &Fetch) -> Result<(), ()> {
        smol::future::yield_now().await;
        outbox.emit(42);
        Ok(())
    }
}

pub struct Enter;

sml! {
    Fetching {
        *Waiting + event<Fetch> / async fetched = X,
    }

    Session[outputs: u8] {
        *Outside + event<Enter> = state<Fetching>,
    }
}

pub struct SessionContext;

impl SessionStateMachineContext for SessionContext {
    async fn fetched(&mut self, outbox: &mut Outbox<u8>, _event: &Fetch) -> Result<(), ()> {
        smol::future::yield_now().await;
        outbox.emit(7);
        outbox.stop();
        Ok(())
    }
}

#[test]
fn async_flat_and_composite_actions_emit_through_the_outbox() {
    smol::block_on(async {
        let mut loader = LoaderStateMachine::new(LoaderContext);
        loader.process_event(Fetch).await.unwrap();
        assert_eq!(loader.outbox_mut().drain().collect::<Vec<_>>(), vec![42]);

        let mut session = SessionStateMachine::new(SessionContext);
        session.process_event(Enter).await.unwrap();
        session.process_event(Fetch).await.unwrap();
        assert_eq!(session.outbox().pending(), &[7]);
        assert!(session.outbox().stop_requested());
    });
}
