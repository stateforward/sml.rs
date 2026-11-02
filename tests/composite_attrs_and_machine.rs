//! The parent table's `states_attr` and `events_attr` reach a single-child
//! composite's generated enums, a derived `PartialEq` on events compares
//! payloads, and synchronous composites implement `sml::Machine<Events>`.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sml::sml;

const fn journalable<T: Clone + core::fmt::Debug + PartialEq + Serialize + DeserializeOwned>() {}

// ---------------------------------------------------------------------------
// A flat table: a derived `PartialEq` replaces the variant-only comparison.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Open {
    pub id: u32,
}

sml! {
    Door[events_attr: #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]] {
        *Shut + event<Open> = X,
    }
}

#[test]
fn derived_event_partial_eq_compares_payloads() {
    journalable::<DoorEvents>();
    assert_eq!(
        DoorEvents::from(Open { id: 1 }),
        DoorEvents::from(Open { id: 1 })
    );
    assert_ne!(
        DoorEvents::from(Open { id: 1 }),
        DoorEvents::from(Open { id: 2 })
    );
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
        *Planning + event<Step> = X,
    }

    Job[
        events_attr: #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)],
        states_attr: #[derive(Debug)]
    ] {
        *Idle + event<Start> = state<Work>,
         state<Work> + event<Stop> = X,
    }
}

pub struct JobContext;

impl JobStateMachineContext for JobContext {}

#[test]
fn single_child_composite_applies_the_parent_attrs_and_implements_machine() {
    journalable::<JobEvents>();
    assert_ne!(JobEvents::from(Step(1)), JobEvents::from(Step(2)));
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
         state<Inner> = X,
    }

    Outer {
        *Outside + event<Go> = state<Middle>,
         state<Middle> + event<Leave> = X,
    }
}

pub struct OuterContext;

impl OuterStateMachineContext for OuterContext {}

fn drive<M: sml::Machine<E>, E>(machine: &mut M, event: E) -> bool {
    machine.process_event(event)
}

#[test]
fn composite_tree_implements_machine() {
    let mut outer = OuterStateMachine::new(OuterContext);
    assert!(drive(&mut outer, OuterEvents::from(Go)));
    assert!(drive(&mut outer, OuterEvents::from(Go)));
    assert!(drive(&mut outer, OuterEvents::from(Go)));
    assert!(drive(&mut outer, OuterEvents::from(Leave)));
    assert!(outer.is_terminated());
    assert!(!drive(&mut outer, OuterEvents::from(Leave)));
}
