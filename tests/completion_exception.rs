//! Faults raised while a composite or orthogonal machine stabilizes
//! (`completion<E>` and anonymous `completion<_>` actions and guards) are routed
//! to `exception<E>` rows exactly like faults raised during event dispatch: the
//! innermost active state with a matching row handles it, walking outward to
//! the parents, and the machine never stays mid-flow in a child.

mod support;

use sml::sml;

/// Returns whether root region `$region` is in a state matching `$pattern`.
macro_rules! region_is {
    ($machine:expr, $region:expr, $pattern:pat) => {{
        let mut index = 0;
        let mut matched = false;
        $machine.visit_current_states(|state| {
            if index == $region {
                matched = matches!(state, $pattern);
            }
            index += 1;
        });
        matched
    }};
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault(u8);

/// `fail_at` names the completion step that fails; 0 never fails.
pub struct Go {
    fail_at: u8,
}

sml! {
    TwoLevelKid {
        *"first"_s + completion<Go> / kid_first = "second"_s,
         "first"_s + exception<Fault> / kid_first_fault = X,
         "second"_s + completion<Go> / kid_second = X,
    }

    TwoLevel {
        *"idle"_s + event<Go> = state<TwoLevelKid>,
         state<TwoLevelKid> + completion<_> / parent_done = "idle"_s,
         state<TwoLevelKid> + exception<Fault> / parent_fault = "idle"_s,
    }
}

#[derive(Default)]
struct TwoLevelContext {
    log: core::cell::RefCell<Vec<String>>,
    fail_parent_done: core::cell::Cell<bool>,
}

impl TwoLevelStateMachineContext for &TwoLevelContext {
    fn kid_first(&mut self, event: &Go) -> Result<(), Fault> {
        self.log.borrow_mut().push("first".into());
        if event.fail_at == 1 {
            Err(Fault(1))
        } else {
            Ok(())
        }
    }

    fn kid_first_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.log
            .borrow_mut()
            .push(format!("kid_first_fault {}", error.0));
        Ok(())
    }

    fn kid_second(&mut self, event: &Go) -> Result<(), Fault> {
        self.log.borrow_mut().push("second".into());
        if event.fail_at == 2 {
            Err(Fault(2))
        } else {
            Ok(())
        }
    }

    fn parent_done(&mut self) -> Result<(), Fault> {
        self.log.borrow_mut().push("parent_done".into());
        if self.fail_parent_done.get() {
            self.fail_parent_done.set(false);
            Err(Fault(3))
        } else {
            Ok(())
        }
    }

    fn parent_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.log
            .borrow_mut()
            .push(format!("parent_fault {}", error.0));
        Ok(())
    }
}

fn run_two_level(
    machine: &mut TwoLevelStateMachine<&TwoLevelContext>,
    context: &TwoLevelContext,
    fail_at: u8,
) -> Vec<String> {
    machine.process_event(Go { fail_at }).unwrap();
    assert!(machine.is(&TwoLevelStates::Idle));
    context.log.take()
}

#[test]
fn two_level_completion_fault_is_caught_by_the_child_exception_row() {
    let context = TwoLevelContext::default();
    let mut machine = TwoLevelStateMachine::new(&context);
    assert_eq!(
        run_two_level(&mut machine, &context, 1),
        ["first", "kid_first_fault 1", "parent_done"]
    );
    // The child is not left mid-flow: the next entry starts it from "first".
    assert_eq!(
        run_two_level(&mut machine, &context, 0),
        ["first", "second", "parent_done"]
    );
}

#[test]
fn two_level_completion_fault_walks_out_to_the_parent_exception_row() {
    let context = TwoLevelContext::default();
    let mut machine = TwoLevelStateMachine::new(&context);
    // "second" has no exception row, so state<TwoLevelKid> + exception<Fault>
    // handles it and exits the child.
    assert_eq!(
        run_two_level(&mut machine, &context, 2),
        ["first", "second", "parent_fault 2"]
    );
    assert_eq!(
        run_two_level(&mut machine, &context, 0),
        ["first", "second", "parent_done"]
    );
}

#[test]
fn two_level_anonymous_parent_completion_fault_is_caught_by_the_parent_row() {
    let context = TwoLevelContext {
        fail_parent_done: core::cell::Cell::new(true),
        ..TwoLevelContext::default()
    };
    let mut machine = TwoLevelStateMachine::new(&context);
    assert_eq!(
        run_two_level(&mut machine, &context, 0),
        ["first", "second", "parent_done", "parent_fault 3"]
    );
    assert_eq!(
        run_two_level(&mut machine, &context, 0),
        ["first", "second", "parent_done"]
    );
}

pub struct Deep {
    fail_at: u8,
}

sml! {
    DeepFaultLeaf {
        *"leaf one"_s + completion<Deep> / leaf_one = "leaf two"_s,
         "leaf one"_s + exception<Fault> / leaf_fault = X,
         "leaf two"_s + completion<Deep> / leaf_two = X,
    }

    DeepFaultMiddle {
        *state<DeepFaultLeaf> + completion<_> / middle_done = X,
    }

    DeepFaultRoot {
        *"idle"_s + event<Deep> = state<DeepFaultMiddle>,
         state<DeepFaultMiddle> + completion<_> = "idle"_s,
         state<DeepFaultMiddle> + exception<Fault> / root_fault = "idle"_s,
    }
}

#[derive(Default)]
struct DeepFaultContext {
    log: core::cell::RefCell<Vec<String>>,
    fail_middle_done: core::cell::Cell<bool>,
}

impl DeepFaultRootStateMachineContext for &DeepFaultContext {
    fn leaf_one(&mut self, event: &Deep) -> Result<(), Fault> {
        self.log.borrow_mut().push("leaf_one".into());
        if event.fail_at == 1 {
            Err(Fault(1))
        } else {
            Ok(())
        }
    }

    fn leaf_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.log
            .borrow_mut()
            .push(format!("leaf_fault {}", error.0));
        Ok(())
    }

    fn leaf_two(&mut self, event: &Deep) -> Result<(), Fault> {
        self.log.borrow_mut().push("leaf_two".into());
        if event.fail_at == 2 {
            Err(Fault(2))
        } else {
            Ok(())
        }
    }

    fn middle_done(&mut self) -> Result<(), Fault> {
        self.log.borrow_mut().push("middle_done".into());
        if self.fail_middle_done.get() {
            self.fail_middle_done.set(false);
            Err(Fault(3))
        } else {
            Ok(())
        }
    }

    fn root_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.log
            .borrow_mut()
            .push(format!("root_fault {}", error.0));
        Ok(())
    }
}

fn run_deep(
    machine: &mut DeepFaultRootStateMachine<&DeepFaultContext>,
    context: &DeepFaultContext,
    fail_at: u8,
) -> Vec<String> {
    machine.process_event(Deep { fail_at }).unwrap();
    assert!(machine.is(&DeepFaultRootStates::Idle));
    context.log.take()
}

#[test]
fn three_level_completion_fault_is_caught_by_the_innermost_exception_row() {
    let context = DeepFaultContext::default();
    let mut machine = DeepFaultRootStateMachine::new(&context);
    assert_eq!(
        run_deep(&mut machine, &context, 1),
        ["leaf_one", "leaf_fault 1", "middle_done"]
    );
    assert_eq!(
        run_deep(&mut machine, &context, 0),
        ["leaf_one", "leaf_two", "middle_done"]
    );
}

#[test]
fn three_level_completion_fault_walks_out_to_the_root_exception_row() {
    let context = DeepFaultContext::default();
    let mut machine = DeepFaultRootStateMachine::new(&context);
    // Neither "leaf two" nor the middle table has an exception row.
    assert_eq!(
        run_deep(&mut machine, &context, 2),
        ["leaf_one", "leaf_two", "root_fault 2"]
    );
    assert_eq!(
        run_deep(&mut machine, &context, 0),
        ["leaf_one", "leaf_two", "middle_done"]
    );
}

#[test]
fn three_level_anonymous_middle_completion_fault_walks_out_to_the_root() {
    let context = DeepFaultContext {
        fail_middle_done: core::cell::Cell::new(true),
        ..DeepFaultContext::default()
    };
    let mut machine = DeepFaultRootStateMachine::new(&context);
    assert_eq!(
        run_deep(&mut machine, &context, 0),
        ["leaf_one", "leaf_two", "middle_done", "root_fault 3"]
    );
    assert_eq!(
        run_deep(&mut machine, &context, 0),
        ["leaf_one", "leaf_two", "middle_done"]
    );
}

pub struct AsyncGo {
    fail: bool,
}

sml! {
    AsyncFaultKid {
        *"working"_s + completion<AsyncGo> / async kid_work = X,
    }

    AsyncFaultParent {
        *"idle"_s + event<AsyncGo> = state<AsyncFaultKid>,
         state<AsyncFaultKid> + completion<_> = "idle"_s,
         state<AsyncFaultKid> + exception<Fault> / async parent_recover = "idle"_s,
    }
}

#[derive(Default)]
struct AsyncFaultContext {
    log: support::SyncRefCell<Vec<String>>,
}

impl AsyncFaultParentStateMachineContext for &AsyncFaultContext {
    async fn kid_work(&mut self, event: &AsyncGo) -> Result<(), Fault> {
        core::future::ready(()).await;
        self.log.borrow_mut().push("kid_work".into());
        if event.fail {
            Err(Fault(4))
        } else {
            Ok(())
        }
    }

    async fn parent_recover(&mut self, error: &Fault) -> Result<(), Fault> {
        core::future::ready(()).await;
        self.log
            .borrow_mut()
            .push(format!("parent_recover {}", error.0));
        Ok(())
    }
}

#[test]
fn async_composite_completion_fault_is_caught_by_the_parent_row() {
    smol::block_on(async {
        let context = AsyncFaultContext::default();
        let mut machine = AsyncFaultParentStateMachine::new(&context);
        machine.process_event(AsyncGo { fail: true }).await.unwrap();
        assert!(machine.is(&AsyncFaultParentStates::Idle));
        assert_eq!(*context.log.borrow(), ["kid_work", "parent_recover 4"]);
        context.log.borrow_mut().clear();
        machine
            .process_event(AsyncGo { fail: false })
            .await
            .unwrap();
        assert!(machine.is(&AsyncFaultParentStates::Idle));
        assert_eq!(*context.log.borrow(), ["kid_work"]);
    });
}

pub struct Loose;

sml! {
    LooseKid {
        *"working"_s + completion<Loose> / loose_work = X,
         "elsewhere"_s + exception<Fault> = X,
    }

    LooseParent {
        *"idle"_s + event<Loose> = state<LooseKid>,
         state<LooseKid> + completion<_> = "idle"_s,
    }
}

struct LooseContext;

impl LooseParentStateMachineContext for LooseContext {
    fn loose_work(&mut self, _: &Loose) -> Result<(), Fault> {
        Err(Fault(6))
    }
}

#[test]
fn unhandled_completion_fault_still_returns_the_action_error() {
    // The only exception row is on a state that is not active, so the fault
    // is returned to the caller unchanged.
    let mut machine = LooseParentStateMachine::new(LooseContext);
    assert!(matches!(
        machine.process_event(Loose),
        Err(LooseParentError::ActionFailed(Fault(6)))
    ));
}

pub struct Halt;

pub struct Tick {
    fail: bool,
}

sml! {
    OrthogonalFault {
        *"left idle"_s + event<Tick> = "left step"_s,
         "left step"_s + completion<Tick> / left_step = "left idle"_s,
         "left step"_s + exception<Fault> / left_fault = "left idle"_s,

        *"right idle"_s + event<Tick> / right_tick,
         "right idle"_s + event<Halt> = X,
    }
}

#[derive(Default)]
struct OrthogonalFaultContext {
    log: core::cell::RefCell<Vec<String>>,
}

impl OrthogonalFaultStateMachineContext for &OrthogonalFaultContext {
    fn left_step(&mut self, event: &Tick) -> Result<(), Fault> {
        self.log.borrow_mut().push("left_step".into());
        if event.fail {
            Err(Fault(5))
        } else {
            Ok(())
        }
    }

    fn left_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.log
            .borrow_mut()
            .push(format!("left_fault {}", error.0));
        Ok(())
    }

    fn right_tick(&mut self, _: &Tick) -> Result<(), Fault> {
        self.log.borrow_mut().push("right_tick".into());
        Ok(())
    }
}

#[test]
fn orthogonal_region_completion_fault_is_caught_by_its_exception_row() {
    let context = OrthogonalFaultContext::default();
    let mut machine = OrthogonalFaultStateMachine::new(&context);
    machine.process_event(Tick { fail: true }).unwrap();
    assert!(region_is!(machine, 0, OrthogonalFaultStates::LeftIdle));
    assert_eq!(
        *context.log.borrow(),
        ["right_tick", "left_step", "left_fault 5"]
    );
    context.log.borrow_mut().clear();
    machine.process_event(Tick { fail: false }).unwrap();
    assert!(region_is!(machine, 0, OrthogonalFaultStates::LeftIdle));
    assert_eq!(*context.log.borrow(), ["right_tick", "left_step"]);
}

// A recovered completion fault ends the run-to-completion step unless the
// exception row that handled it changed the active state. An anonymous
// `completion<_>` that always faults, recovered by an internal or a
// self-targeting exception row, would otherwise retry forever; the faulting
// action counts its calls and panics past `SPIN_LIMIT` so a hang shows up as a
// test failure.

const SPIN_LIMIT: u32 = 10;

#[derive(Default)]
struct SpinContext {
    spins: u32,
    faults: u32,
}

impl SpinContext {
    fn count_spin(&mut self) -> Result<(), Fault> {
        self.spins += 1;
        assert!(
            self.spins <= SPIN_LIMIT,
            "a recovered completion fault was retried {} times",
            self.spins
        );
        Err(Fault(7))
    }

    fn count_fault(&mut self, error: Fault) {
        assert_eq!(error, Fault(7));
        self.faults += 1;
    }
}

pub struct Spin;

sml! {
    InternalSpinKid {
        *"spinning"_s + completion<_> / spin = X,
         "spinning"_s + exception<Fault> / spin_fault,
    }

    InternalSpin {
        *"idle"_s + event<Spin> = state<InternalSpinKid>,
         state<InternalSpinKid> + completion<_> = "idle"_s,
    }
}

impl InternalSpinStateMachineContext for &mut SpinContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.count_spin()
    }

    fn spin_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.count_fault(*error);
        Ok(())
    }
}

#[test]
fn completion_fault_recovered_by_an_internal_row_ends_process_event() {
    let mut context = SpinContext::default();
    let mut machine = InternalSpinStateMachine::new(&mut context);
    machine.process_event(Spin).unwrap();
    // The internal row left the child in "spinning"; the step ended there.
    assert!(machine.is(&InternalSpinStates::InternalSpinKid));
    assert!(machine.is(&InternalSpinInternalSpinKidStates::Spinning));
    assert_eq!((context.spins, context.faults), (1, 1));
}

sml! {
    InitialSpinKid {
        *"spinning"_s + completion<_> / spin = X,
         "spinning"_s + exception<Fault> / spin_fault,
    }

    InitialSpin {
        *state<InitialSpinKid> + completion<_> = "idle"_s,
         "idle"_s + event<Spin> = state<InitialSpinKid>,
    }
}

impl InitialSpinStateMachineContext for &mut SpinContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.count_spin()
    }

    fn spin_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.count_fault(*error);
        Ok(())
    }
}

#[test]
fn completion_fault_recovered_by_an_internal_row_ends_initialize() {
    let mut context = SpinContext::default();
    let mut machine = InitialSpinStateMachine::new(&mut context);
    machine.initialize().unwrap();
    assert!(machine.is(&InitialSpinStates::InitialSpinKid));
    assert!(machine.is(&InitialSpinInitialSpinKidStates::Spinning));
    assert_eq!((context.spins, context.faults), (1, 1));
}

sml! {
    SelfSpinKid {
        *"spinning"_s + completion<_> / spin = X,
         "spinning"_s + exception<Fault> / spin_fault = "spinning"_s,
    }

    SelfSpin {
        *"idle"_s + event<Spin> = state<SelfSpinKid>,
         state<SelfSpinKid> + completion<_> = "idle"_s,
    }
}

impl SelfSpinStateMachineContext for &mut SpinContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.count_spin()
    }

    fn spin_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.count_fault(*error);
        Ok(())
    }
}

#[test]
fn completion_fault_recovered_by_a_self_targeting_row_ends_process_event() {
    let mut context = SpinContext::default();
    let mut machine = SelfSpinStateMachine::new(&mut context);
    machine.process_event(Spin).unwrap();
    assert!(machine.is(&SelfSpinStates::SelfSpinKid));
    assert!(machine.is(&SelfSpinSelfSpinKidStates::Spinning));
    assert_eq!((context.spins, context.faults), (1, 1));
}

sml! {
    OrthogonalSpin {
        *"left idle"_s + event<Spin> = "left spinning"_s,
         "left spinning"_s + completion<_> / spin = "left idle"_s,
         "left spinning"_s + exception<Fault> / spin_fault,

        *"right idle"_s + event<Halt> = X,
    }
}

impl OrthogonalSpinStateMachineContext for &mut SpinContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.count_spin()
    }

    fn spin_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.count_fault(*error);
        Ok(())
    }
}

#[test]
fn orthogonal_completion_fault_recovered_by_an_internal_row_ends_process_event() {
    let mut context = SpinContext::default();
    let mut machine = OrthogonalSpinStateMachine::new(&mut context);
    machine.process_event(Spin).unwrap();
    assert!(region_is!(machine, 0, OrthogonalSpinStates::LeftSpinning));
    assert_eq!((context.spins, context.faults), (1, 1));
}

// A recovery that does not move settles only the region whose completion
// faulted: a peer region's pending completion chain still runs to the end of
// the same run-to-completion step, and the faulting region is not retried.

#[derive(Default)]
struct PeerContext {
    spin: SpinContext,
    log: Vec<&'static str>,
}

sml! {
    OrthogonalPeer {
        *"left idle"_s + event<Spin> = "left spinning"_s,
         "left spinning"_s + completion<_> / spin = "left idle"_s,
         "left spinning"_s + exception<Fault> / spin_fault,

        *"right idle"_s + event<Spin> = "right one"_s,
         "right one"_s + completion<_> / right_one = "right two"_s,
         "right two"_s + completion<_> / right_two = "right done"_s,
         "right done"_s + event<Halt> = X,
    }
}

impl OrthogonalPeerStateMachineContext for &mut PeerContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.spin.count_spin()
    }

    fn spin_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.spin.count_fault(*error);
        Ok(())
    }

    fn right_one(&mut self) -> Result<(), Fault> {
        self.log.push("right_one");
        Ok(())
    }

    fn right_two(&mut self) -> Result<(), Fault> {
        self.log.push("right_two");
        Ok(())
    }
}

#[test]
fn orthogonal_no_progress_recovery_lets_a_peer_region_finish_on_process_event() {
    let mut context = PeerContext::default();
    let mut machine = OrthogonalPeerStateMachine::new(&mut context);
    machine.process_event(Spin).unwrap();
    assert!(region_is!(machine, 0, OrthogonalPeerStates::LeftSpinning));
    assert!(region_is!(machine, 1, OrthogonalPeerStates::RightDone));
    assert_eq!((context.spin.spins, context.spin.faults), (1, 1));
    assert_eq!(context.log, ["right_one", "right_two"]);
}

sml! {
    OrthogonalInitialPeer {
        *"left spinning"_s + completion<_> / spin = "left idle"_s,
         "left spinning"_s + exception<Fault> / spin_fault,

        *"right one"_s + completion<_> / right_one = "right two"_s,
         "right two"_s + completion<_> / right_two = "right done"_s,
         "right done"_s + event<Halt> = X,
    }
}

impl OrthogonalInitialPeerStateMachineContext for &mut PeerContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.spin.count_spin()
    }

    fn spin_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.spin.count_fault(*error);
        Ok(())
    }

    fn right_one(&mut self) -> Result<(), Fault> {
        self.log.push("right_one");
        Ok(())
    }

    fn right_two(&mut self) -> Result<(), Fault> {
        self.log.push("right_two");
        Ok(())
    }
}

#[test]
fn orthogonal_no_progress_recovery_lets_a_peer_region_finish_on_initialize() {
    let mut context = PeerContext::default();
    let mut machine = OrthogonalInitialPeerStateMachine::new(&mut context);
    machine.initialize().unwrap();
    assert!(region_is!(
        machine,
        0,
        OrthogonalInitialPeerStates::LeftSpinning
    ));
    assert!(region_is!(
        machine,
        1,
        OrthogonalInitialPeerStates::RightDone
    ));
    assert_eq!((context.spin.spins, context.spin.faults), (1, 1));
    assert_eq!(context.log, ["right_one", "right_two"]);
}

sml! {
    NestedPeerKid {
        *"left spinning"_s + completion<_> / spin = "left idle"_s,
         "left spinning"_s + exception<Fault> / spin_fault,

        *"right one"_s + completion<_> / right_one = "right two"_s,
         "right two"_s + completion<_> / right_two = "right done"_s,
         "right done"_s + event<Halt> = X,
    }

    NestedPeer {
        *"idle"_s + event<Spin> = state<NestedPeerKid>,
         state<NestedPeerKid> + completion<_> = "idle"_s,
    }
}

impl NestedPeerStateMachineContext for &mut PeerContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.spin.count_spin()
    }

    fn spin_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.spin.count_fault(*error);
        Ok(())
    }

    fn right_one(&mut self) -> Result<(), Fault> {
        self.log.push("right_one");
        Ok(())
    }

    fn right_two(&mut self) -> Result<(), Fault> {
        self.log.push("right_two");
        Ok(())
    }
}

#[test]
fn nested_orthogonal_no_progress_recovery_lets_a_peer_region_finish() {
    let mut context = PeerContext::default();
    let mut machine = NestedPeerStateMachine::new(&mut context);
    machine.process_event(Spin).unwrap();
    assert!(machine.is(&NestedPeerStates::NestedPeerKid));
    assert!(machine.is(&[
        NestedPeerNestedPeerKidStates::LeftSpinning,
        NestedPeerNestedPeerKidStates::RightDone,
    ]));
    assert_eq!((context.spin.spins, context.spin.faults), (1, 1));
    assert_eq!(context.log, ["right_one", "right_two"]);
}

sml! {
    RootPeerKid {
        *"one"_s + completion<_> / right_one = "two"_s,
         "two"_s + completion<_> / right_two = X,
    }

    RootPeer {
        *"left spinning"_s + completion<_> / spin = "left idle"_s,
         "left spinning"_s + exception<Fault> / spin_fault,

        *state<RootPeerKid> + completion<_> = "right done"_s,
         "right done"_s + event<Halt> = X,
    }
}

impl RootPeerStateMachineContext for &mut PeerContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.spin.count_spin()
    }

    fn spin_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.spin.count_fault(*error);
        Ok(())
    }

    fn right_one(&mut self) -> Result<(), Fault> {
        self.log.push("right_one");
        Ok(())
    }

    fn right_two(&mut self) -> Result<(), Fault> {
        self.log.push("right_two");
        Ok(())
    }
}

#[test]
fn orthogonal_root_no_progress_recovery_lets_a_peer_region_finish_on_initialize() {
    let mut context = PeerContext::default();
    let mut machine = RootPeerStateMachine::new(&mut context);
    machine.initialize().unwrap();
    assert!(region_is!(machine, 0, RootPeerStates::LeftSpinning));
    assert!(region_is!(machine, 1, RootPeerStates::RightDone));
    assert_eq!((context.spin.spins, context.spin.faults), (1, 1));
    assert_eq!(context.log, ["right_one", "right_two"]);
}

sml! {
    AsyncOrthogonalPeer {
        *"left idle"_s + event<Spin> = "left spinning"_s,
         "left spinning"_s + completion<_> / async spin = "left idle"_s,
         "left spinning"_s + exception<Fault> / spin_fault,

        *"right idle"_s + event<Spin> = "right one"_s,
         "right one"_s + completion<_> / async right_one = "right two"_s,
         "right two"_s + completion<_> / right_two = "right done"_s,
         "right done"_s + event<Halt> = X,
    }
}

impl AsyncOrthogonalPeerStateMachineContext for &mut PeerContext {
    async fn spin(&mut self) -> Result<(), Fault> {
        core::future::ready(()).await;
        self.spin.count_spin()
    }

    fn spin_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.spin.count_fault(*error);
        Ok(())
    }

    async fn right_one(&mut self) -> Result<(), Fault> {
        core::future::ready(()).await;
        self.log.push("right_one");
        Ok(())
    }

    fn right_two(&mut self) -> Result<(), Fault> {
        self.log.push("right_two");
        Ok(())
    }
}

#[test]
fn async_orthogonal_no_progress_recovery_lets_a_peer_region_finish() {
    smol::block_on(async {
        let mut context = PeerContext::default();
        let mut machine = AsyncOrthogonalPeerStateMachine::new(&mut context);
        machine.process_event(Spin).await.unwrap();
        assert!(region_is!(
            machine,
            0,
            AsyncOrthogonalPeerStates::LeftSpinning
        ));
        assert!(region_is!(machine, 1, AsyncOrthogonalPeerStates::RightDone));
        assert_eq!((context.spin.spins, context.spin.faults), (1, 1));
        assert_eq!(context.log, ["right_one", "right_two"]);
    });
}

// A recovery settles the faulting region unless a row on that region's own
// path moved: a peer region's exception row that moves the peer is not
// progress for the faulting completion, so it is not retried.

#[derive(Default)]
struct PeerFaultContext {
    spin: SpinContext,
    peer_faults: u32,
}

impl PeerFaultContext {
    fn count_peer_fault(&mut self, error: Fault) {
        assert_eq!(error, Fault(7));
        self.peer_faults += 1;
    }
}

sml! {
    PeerMovesOnce {
        *"left idle"_s + event<Spin> = "left spinning"_s,
         "left spinning"_s + completion<_> / spin = "left idle"_s,
         "left spinning"_s + exception<Fault> / spin_fault,

        *"right idle"_s + event<Spin> = "right one"_s,
         "right one"_s + exception<Fault> / peer_fault = "right two"_s,
         "right two"_s + event<Halt> = X,
    }
}

impl PeerMovesOnceStateMachineContext for &mut PeerFaultContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.spin.count_spin()
    }

    fn spin_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.spin.count_fault(*error);
        Ok(())
    }

    fn peer_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.count_peer_fault(*error);
        Ok(())
    }
}

#[test]
fn orthogonal_peer_exception_move_does_not_retry_the_faulting_region() {
    let mut context = PeerFaultContext::default();
    let mut machine = PeerMovesOnceStateMachine::new(&mut context);
    machine.process_event(Spin).unwrap();
    assert!(region_is!(machine, 0, PeerMovesOnceStates::LeftSpinning));
    assert!(region_is!(machine, 1, PeerMovesOnceStates::RightTwo));
    assert_eq!((context.spin.spins, context.spin.faults), (1, 1));
    assert_eq!(context.peer_faults, 1);
}

sml! {
    PeerAlternates {
        *"left idle"_s + event<Spin> = "left spinning"_s,
         "left spinning"_s + completion<_> / spin = "left idle"_s,
         "left idle"_s + event<Halt> = X,
         "left spinning"_s + exception<Fault> / spin_fault,

        *"right idle"_s + event<Spin> = "right one"_s,
         "right one"_s + exception<Fault> / peer_fault = "right two"_s,
         "right two"_s + exception<Fault> / peer_fault = "right one"_s,
    }
}

impl PeerAlternatesStateMachineContext for &mut PeerFaultContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.spin.count_spin()
    }

    fn spin_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.spin.count_fault(*error);
        Ok(())
    }

    fn peer_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.count_peer_fault(*error);
        Ok(())
    }
}

#[test]
fn orthogonal_alternating_peer_exception_rows_terminate() {
    let mut context = PeerFaultContext::default();
    let mut machine = PeerAlternatesStateMachine::new(&mut context);
    machine.process_event(Spin).unwrap();
    assert!(region_is!(machine, 0, PeerAlternatesStates::LeftSpinning));
    assert!(region_is!(machine, 1, PeerAlternatesStates::RightTwo));
    assert_eq!((context.spin.spins, context.spin.faults), (1, 1));
    assert_eq!(context.peer_faults, 1);
}

sml! {
    PeerOnlyHandler {
        *"left idle"_s + event<Spin> = "left spinning"_s,
         "left spinning"_s + completion<_> / spin = "left idle"_s,
         "left idle"_s + event<Halt> = X,

        *"right idle"_s + event<Spin> = "right one"_s,
         "right one"_s + exception<Fault> / peer_fault = "right two"_s,
         "right two"_s + exception<Fault> / peer_fault = "right one"_s,
    }
}

impl PeerOnlyHandlerStateMachineContext for &mut PeerFaultContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.spin.count_spin()
    }

    fn peer_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.count_peer_fault(*error);
        Ok(())
    }
}

#[test]
fn orthogonal_fault_handled_only_by_a_moving_peer_settles_the_faulting_region() {
    let mut context = PeerFaultContext::default();
    let mut machine = PeerOnlyHandlerStateMachine::new(&mut context);
    machine.process_event(Spin).unwrap();
    assert!(region_is!(machine, 0, PeerOnlyHandlerStates::LeftSpinning));
    assert!(region_is!(machine, 1, PeerOnlyHandlerStates::RightTwo));
    assert_eq!(context.spin.spins, 1);
    assert_eq!(context.peer_faults, 1);
}

sml! {
    RegionKidFaultKid {
        *"spinning"_s + completion<_> / spin = X,
    }

    RegionKidFault {
        *state<RegionKidFaultKid> + completion<_> = "left done"_s,

        *"right one"_s + exception<Fault> / peer_fault = "right two"_s,
         "right two"_s + exception<Fault> / peer_fault = "right one"_s,
    }
}

impl RegionKidFaultStateMachineContext for &mut PeerFaultContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.spin.count_spin()
    }

    fn peer_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.count_peer_fault(*error);
        Ok(())
    }
}

#[test]
fn fault_inside_a_region_composite_is_not_retried_after_a_peer_moves() {
    let mut context = PeerFaultContext::default();
    let mut machine = RegionKidFaultStateMachine::new(&mut context);
    machine.initialize().unwrap();
    assert!(region_is!(
        machine,
        0,
        RegionKidFaultStates::RegionKidFaultKid
    ));
    assert!(region_is!(machine, 1, RegionKidFaultStates::RightTwo));
    assert_eq!(context.spin.spins, 1);
    assert_eq!(context.peer_faults, 1);
}

sml! {
    NestedPeerMovesKid {
        *"left spinning"_s + completion<_> / spin = "left idle"_s,
         "left spinning"_s + exception<Fault> / spin_fault,

        *"right one"_s + exception<Fault> / peer_fault = "right two"_s,
         "right two"_s + exception<Fault> / peer_fault = "right one"_s,
    }

    NestedPeerMoves {
        *"idle"_s + event<Spin> = state<NestedPeerMovesKid>,
    }
}

impl NestedPeerMovesStateMachineContext for &mut PeerFaultContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.spin.count_spin()
    }

    fn spin_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.spin.count_fault(*error);
        Ok(())
    }

    fn peer_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.count_peer_fault(*error);
        Ok(())
    }
}

#[test]
fn nested_orthogonal_peer_exception_move_does_not_retry_the_faulting_region() {
    let mut context = PeerFaultContext::default();
    let mut machine = NestedPeerMovesStateMachine::new(&mut context);
    machine.process_event(Spin).unwrap();
    assert!(machine.is(&[
        NestedPeerMovesNestedPeerMovesKidStates::LeftSpinning,
        NestedPeerMovesNestedPeerMovesKidStates::RightTwo,
    ]));
    assert_eq!((context.spin.spins, context.spin.faults), (1, 1));
    assert_eq!(context.peer_faults, 1);
}

sml! {
    PeerKidMovesKid {
        *"one"_s + exception<Fault> / peer_fault = "two"_s,
         "two"_s + exception<Fault> / peer_fault = "one"_s,
    }

    PeerKidMoves {
        *"left spinning"_s + completion<_> / spin = "left idle"_s,

        *state<PeerKidMovesKid> + event<Halt> = X,
    }
}

impl PeerKidMovesStateMachineContext for &mut PeerFaultContext {
    fn spin(&mut self) -> Result<(), Fault> {
        self.spin.count_spin()
    }

    fn peer_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.count_peer_fault(*error);
        Ok(())
    }
}

#[test]
fn peer_region_composite_exception_move_does_not_retry_the_faulting_region() {
    let mut context = PeerFaultContext::default();
    let mut machine = PeerKidMovesStateMachine::new(&mut context);
    machine.initialize().unwrap();
    assert!(region_is!(machine, 0, PeerKidMovesStates::LeftSpinning));
    assert!(region_is!(machine, 1, PeerKidMovesStates::PeerKidMovesKid));
    assert!(machine.is(&PeerKidMovesPeerKidMovesKidStates::Two));
    assert_eq!(context.spin.spins, 1);
    assert_eq!(context.peer_faults, 1);
}

// Settled marks belong to an occupancy: when a recovery leaves the subtree
// that holds a settled site, re-entering it within the same stabilize() runs
// that site's completions again.

#[derive(Default)]
struct ReenterContext {
    left: u32,
    right: u32,
    recoveries: u32,
}

impl ReenterContext {
    const fn attempt(&self) -> Result<(), Fault> {
        if self.recoveries == 0 {
            Err(Fault(7))
        } else {
            Ok(())
        }
    }
}

sml! {
    ReenterKid {
        *"left spinning"_s + completion<_> / left = X,
         "left spinning"_s + exception<Fault> [left_owns] / left_fault,

        *"right spinning"_s + completion<_> / right = X,
    }

    Reenter {
        *"idle"_s + event<Go> = state<ReenterKid>,
         state<ReenterKid> + exception<Fault> / root_recover = "retry"_s,
         "retry"_s + completion<_> = state<ReenterKid>,
         state<ReenterKid> + completion<_> = "done"_s,
    }
}

impl ReenterStateMachineContext for &mut ReenterContext {
    fn left(&mut self) -> Result<(), Fault> {
        self.left += 1;
        self.attempt()
    }

    // Only the left region's own first fault: right has not run yet.
    fn left_owns(&self, _: &Fault) -> Result<bool, Fault> {
        Ok(self.right == 0)
    }

    fn left_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        assert_eq!(*error, Fault(7));
        Ok(())
    }

    fn right(&mut self) -> Result<(), Fault> {
        self.right += 1;
        self.attempt()
    }

    fn root_recover(&mut self, error: &Fault) -> Result<(), Fault> {
        assert_eq!(*error, Fault(7));
        self.recoveries += 1;
        Ok(())
    }
}

#[test]
fn a_settled_site_runs_again_after_a_recovery_reenters_its_node() {
    let mut context = ReenterContext::default();
    let mut machine = ReenterStateMachine::new(&mut context);
    machine.process_event(Go { fail_at: 0 }).unwrap();
    assert!(machine.is(&ReenterStates::Done));
    assert_eq!((context.left, context.right, context.recoveries), (2, 2, 1));
}

#[derive(Default)]
struct MovedSettledContext {
    left: u32,
    right: u32,
    left_after_move: u32,
}

sml! {
    MovedSettled {
        *"left one"_s + completion<_> / left = "left two"_s,
         "left one"_s + exception<Fault> [right_idle] / left_fault,
         "left one"_s + exception<Fault> [right_ran] / left_fault = "left three"_s,
         "left three"_s + completion<_> / left_after = "left done"_s,
         "left done"_s + event<Halt> = X,

        *"right spinning"_s + completion<_> / right = "right done"_s,
         "right done"_s + event<Halt> = X,
    }
}

impl MovedSettledStateMachineContext for &mut MovedSettledContext {
    fn left(&mut self) -> Result<(), Fault> {
        self.left += 1;
        Err(Fault(7))
    }

    fn right_idle(&self, _: &Fault) -> Result<bool, Fault> {
        Ok(self.right == 0)
    }

    fn right_ran(&self, _: &Fault) -> Result<bool, Fault> {
        Ok(self.right > 0)
    }

    fn left_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        assert_eq!(*error, Fault(7));
        Ok(())
    }

    fn left_after(&mut self) -> Result<(), Fault> {
        self.left_after_move += 1;
        Ok(())
    }

    fn right(&mut self) -> Result<(), Fault> {
        self.right += 1;
        Err(Fault(7))
    }
}

#[test]
fn a_settled_region_moved_by_a_peer_fault_runs_its_new_completions() {
    let mut context = MovedSettledContext::default();
    let mut machine = MovedSettledStateMachine::new(&mut context);
    machine.initialize().unwrap();
    assert!(region_is!(machine, 0, MovedSettledStates::LeftDone));
    assert!(region_is!(machine, 1, MovedSettledStates::RightSpinning));
    assert_eq!(
        (context.left, context.right, context.left_after_move),
        (1, 1, 1)
    );
}
