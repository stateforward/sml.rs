use core::any::TypeId;
use core::cell::Cell;

use sml::{sml, Event, EventVisitor};

pub struct FlatGo;
pub struct FlatGuarded;
pub struct FlatDone;
pub struct FlatSpecificUnexpected;

sml! {
    CurrentEventsFlat {
        *Idle + event<FlatGo> = Ready,
        Idle + event<FlatGuarded> [guard] = Guarded,
        Idle + unexpected_event<FlatSpecificUnexpected> = Ready,
        Idle + unexpected_event<_> = Ready,
        Ready + event<FlatDone> = X,
    }
}

sml! {
    CurrentEventsWithoutExternalEvents {
        *Empty + completion<_> = Ready,
    }
}

#[derive(Default)]
struct FlatContext {
    guard_calls: Cell<usize>,
}

impl CurrentEventsFlatStateMachineContext for FlatContext {
    fn guard(&self, _event: &FlatGuarded) -> Result<bool, ()> {
        self.guard_calls.set(self.guard_calls.get() + 1);
        Ok(false)
    }
}

pub struct CompositeEnter;
pub struct CompositeParentOnly;
pub struct CompositeChildOnly;
pub struct CompositeShared;

sml! {
    CurrentEventsChild {
        *ChildIdle + event<CompositeChildOnly> = ChildReady,
        ChildIdle + event<CompositeShared> = ChildReady,
    }

    CurrentEventsParent {
        *Outside + event<CompositeEnter> = state<CurrentEventsChild>,
        state<CurrentEventsChild> + event<CompositeParentOnly> = Outside,
        state<CurrentEventsChild> + event<CompositeShared> = Outside,
    }
}

pub struct OrthogonalLeft;
pub struct OrthogonalRight;
pub struct OrthogonalShared;
pub struct OrthogonalFinish;

sml! {
    CurrentEventsOrthogonal {
        *Left + event<OrthogonalLeft> = LeftDone,
        Left + event<OrthogonalShared> = LeftDone,
        LeftDone + event<OrthogonalFinish> = X,
        *Right + event<OrthogonalRight> = RightDone,
        Right + event<OrthogonalShared> = RightDone,
        RightDone + event<OrthogonalFinish> = X,
    }
}

pub struct MultiEnterA;
pub struct MultiEnterB;
pub struct MultiParentA;
pub struct MultiParentB;
pub struct MultiAOnly;
pub struct MultiBOnly;

sml! {
    CurrentEventsMultiA {
        *AIdle + event<MultiAOnly> = X,
    }

    CurrentEventsMultiB {
        *BIdle + event<MultiBOnly> = X,
    }

    CurrentEventsMultiParent {
        *Outside + event<MultiEnterA> = state<CurrentEventsMultiA>,
        Outside + event<MultiEnterB> = state<CurrentEventsMultiB>,
        state<CurrentEventsMultiA> + event<MultiParentA> = Outside,
        state<CurrentEventsMultiB> + event<MultiParentB> = Outside,
    }
}

struct CompositeContext;

impl CurrentEventsParentStateMachineContext for CompositeContext {}

struct OrthogonalContext;

impl CurrentEventsOrthogonalStateMachineContext for OrthogonalContext {}

struct MultiContext;

impl CurrentEventsMultiParentStateMachineContext for MultiContext {}

struct EmptyEventContext;

impl CurrentEventsWithoutExternalEventsStateMachineContext for EmptyEventContext {}

#[derive(Default)]
struct Capture {
    names: Vec<&'static str>,
    ids: Vec<TypeId>,
}

impl EventVisitor for Capture {
    fn visit<E: Event>(&mut self) {
        self.names.push(E::name());
        self.ids.push(TypeId::of::<E>());
    }
}

fn assert_event<E: Event>(capture: &Capture) {
    assert!(capture.ids.contains(&TypeId::of::<E>()));
    assert!(capture.names.contains(&E::name()));
}

#[test]
fn flat_query_is_static_and_does_not_run_guards() {
    let context = FlatContext::default();
    let machine = CurrentEventsFlatStateMachine::new(context);
    let mut capture = Capture::default();

    machine.visit_current_events(&mut capture);

    assert_event::<FlatGo>(&capture);
    assert_event::<FlatGuarded>(&capture);
    assert!(!capture.ids.contains(&TypeId::of::<FlatDone>()));
    assert_eq!(machine.context().guard_calls.get(), 0);
    assert!(CurrentEventsFlatEvents::EVENT_NAMES.contains(&"FlatGo"));
    assert!(CurrentEventsFlatEvents::EVENT_NAMES.contains(&"FlatGuarded"));
    assert!(CurrentEventsFlatEvents::EVENT_NAMES.contains(&"FlatSpecificUnexpected"));
}

#[test]
fn composite_query_includes_active_ancestor_and_child_transitions_once() {
    let mut machine = CurrentEventsParentStateMachine::new(CompositeContext);
    let mut capture = Capture::default();

    machine.process_event(CompositeEnter).unwrap();
    machine.visit_current_events(&mut capture);

    assert_event::<CompositeParentOnly>(&capture);
    assert_event::<CompositeChildOnly>(&capture);
    assert_event::<CompositeShared>(&capture);
    assert_eq!(
        capture
            .ids
            .iter()
            .filter(|id| **id == TypeId::of::<CompositeShared>())
            .count(),
        1
    );
}

#[test]
fn orthogonal_query_is_the_union_of_active_regions() {
    let machine = CurrentEventsOrthogonalStateMachine::new(OrthogonalContext);
    let mut capture = Capture::default();

    machine.visit_current_events(&mut capture);

    assert_event::<OrthogonalLeft>(&capture);
    assert_event::<OrthogonalRight>(&capture);
    assert_event::<OrthogonalShared>(&capture);
    assert_eq!(capture.ids.len(), 3);
}

#[test]
fn multi_composite_query_tracks_only_the_active_child_and_ancestor() {
    let mut machine = CurrentEventsMultiParentStateMachine::new(MultiContext);
    let mut capture = Capture::default();

    machine.visit_current_events(&mut capture);
    assert_event::<MultiEnterA>(&capture);
    assert_event::<MultiEnterB>(&capture);
    assert_eq!(capture.ids.len(), 2);

    machine.process_event(MultiEnterA).unwrap();
    let mut capture = Capture::default();
    machine.visit_current_events(&mut capture);
    assert_event::<MultiAOnly>(&capture);
    assert_event::<MultiParentA>(&capture);
    assert!(!capture.ids.contains(&TypeId::of::<MultiBOnly>()));
    assert!(!capture.ids.contains(&TypeId::of::<MultiParentB>()));
    assert_eq!(capture.ids.len(), 2);
}

#[test]
fn catch_all_is_not_reported_but_specific_unexpected_events_are() {
    let machine = CurrentEventsFlatStateMachine::new(FlatContext::default());
    let mut capture = Capture::default();

    machine.visit_current_events(&mut capture);

    assert_event::<FlatSpecificUnexpected>(&capture);
    assert_eq!(capture.ids.len(), 3);
}

#[test]
fn event_names_are_exhaustive_when_the_event_enum_has_no_variants() {
    let machine = CurrentEventsWithoutExternalEventsStateMachine::new(EmptyEventContext);
    let mut capture = Capture::default();

    machine.visit_current_events(&mut capture);

    assert!(capture.ids.is_empty());
    assert_eq!(
        CurrentEventsWithoutExternalEventsEvents::EVENT_NAMES,
        &[] as &[&str]
    );
}
