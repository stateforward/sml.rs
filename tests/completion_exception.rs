//! Faults raised while a composite or orthogonal machine stabilizes
//! (`completion<E>` and anonymous `completion<_>` actions and guards) are routed
//! to `exception<E>` rows exactly like faults raised during event dispatch: the
//! innermost active state with a matching row handles it, walking outward to
//! the parents, and the machine never stays mid-flow in a child.

use sml::sml;

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
    log: Vec<String>,
    fail_parent_done: bool,
}

impl TwoLevelStateMachineContext for TwoLevelContext {
    fn kid_first(&mut self, event: &Go) -> Result<(), Fault> {
        self.log.push("first".into());
        if event.fail_at == 1 {
            Err(Fault(1))
        } else {
            Ok(())
        }
    }

    fn kid_first_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.log.push(format!("kid_first_fault {}", error.0));
        Ok(())
    }

    fn kid_second(&mut self, event: &Go) -> Result<(), Fault> {
        self.log.push("second".into());
        if event.fail_at == 2 {
            Err(Fault(2))
        } else {
            Ok(())
        }
    }

    fn parent_done(&mut self) -> Result<(), Fault> {
        self.log.push("parent_done".into());
        if self.fail_parent_done {
            self.fail_parent_done = false;
            Err(Fault(3))
        } else {
            Ok(())
        }
    }

    fn parent_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.log.push(format!("parent_fault {}", error.0));
        Ok(())
    }
}

fn run_two_level(machine: &mut TwoLevelStateMachine<TwoLevelContext>, fail_at: u8) -> Vec<String> {
    machine.process_event(Go { fail_at }).unwrap();
    assert!(machine.is(&TwoLevelStates::Idle));
    core::mem::take(&mut machine.context_mut().log)
}

#[test]
fn two_level_completion_fault_is_caught_by_the_child_exception_row() {
    let mut machine = TwoLevelStateMachine::new(TwoLevelContext::default());
    assert_eq!(
        run_two_level(&mut machine, 1),
        ["first", "kid_first_fault 1", "parent_done"]
    );
    // The child is not left mid-flow: the next entry starts it from "first".
    assert_eq!(
        run_two_level(&mut machine, 0),
        ["first", "second", "parent_done"]
    );
}

#[test]
fn two_level_completion_fault_walks_out_to_the_parent_exception_row() {
    let mut machine = TwoLevelStateMachine::new(TwoLevelContext::default());
    // "second" has no exception row, so state<TwoLevelKid> + exception<Fault>
    // handles it and exits the child.
    assert_eq!(
        run_two_level(&mut machine, 2),
        ["first", "second", "parent_fault 2"]
    );
    assert_eq!(
        run_two_level(&mut machine, 0),
        ["first", "second", "parent_done"]
    );
}

#[test]
fn two_level_anonymous_parent_completion_fault_is_caught_by_the_parent_row() {
    let mut machine = TwoLevelStateMachine::new(TwoLevelContext {
        fail_parent_done: true,
        ..TwoLevelContext::default()
    });
    assert_eq!(
        run_two_level(&mut machine, 0),
        ["first", "second", "parent_done", "parent_fault 3"]
    );
    assert_eq!(
        run_two_level(&mut machine, 0),
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
    log: Vec<String>,
    fail_middle_done: bool,
}

impl DeepFaultRootStateMachineContext for DeepFaultContext {
    fn leaf_one(&mut self, event: &Deep) -> Result<(), Fault> {
        self.log.push("leaf_one".into());
        if event.fail_at == 1 {
            Err(Fault(1))
        } else {
            Ok(())
        }
    }

    fn leaf_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.log.push(format!("leaf_fault {}", error.0));
        Ok(())
    }

    fn leaf_two(&mut self, event: &Deep) -> Result<(), Fault> {
        self.log.push("leaf_two".into());
        if event.fail_at == 2 {
            Err(Fault(2))
        } else {
            Ok(())
        }
    }

    fn middle_done(&mut self) -> Result<(), Fault> {
        self.log.push("middle_done".into());
        if self.fail_middle_done {
            self.fail_middle_done = false;
            Err(Fault(3))
        } else {
            Ok(())
        }
    }

    fn root_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.log.push(format!("root_fault {}", error.0));
        Ok(())
    }
}

fn run_deep(machine: &mut DeepFaultRootStateMachine<DeepFaultContext>, fail_at: u8) -> Vec<String> {
    machine.process_event(Deep { fail_at }).unwrap();
    assert!(machine.is(&DeepFaultRootStates::Idle));
    core::mem::take(&mut machine.context_mut().log)
}

#[test]
fn three_level_completion_fault_is_caught_by_the_innermost_exception_row() {
    let mut machine = DeepFaultRootStateMachine::new(DeepFaultContext::default());
    assert_eq!(
        run_deep(&mut machine, 1),
        ["leaf_one", "leaf_fault 1", "middle_done"]
    );
    assert_eq!(
        run_deep(&mut machine, 0),
        ["leaf_one", "leaf_two", "middle_done"]
    );
}

#[test]
fn three_level_completion_fault_walks_out_to_the_root_exception_row() {
    let mut machine = DeepFaultRootStateMachine::new(DeepFaultContext::default());
    // Neither "leaf two" nor the middle table has an exception row.
    assert_eq!(
        run_deep(&mut machine, 2),
        ["leaf_one", "leaf_two", "root_fault 2"]
    );
    assert_eq!(
        run_deep(&mut machine, 0),
        ["leaf_one", "leaf_two", "middle_done"]
    );
}

#[test]
fn three_level_anonymous_middle_completion_fault_walks_out_to_the_root() {
    let mut machine = DeepFaultRootStateMachine::new(DeepFaultContext {
        fail_middle_done: true,
        ..DeepFaultContext::default()
    });
    assert_eq!(
        run_deep(&mut machine, 0),
        ["leaf_one", "leaf_two", "middle_done", "root_fault 3"]
    );
    assert_eq!(
        run_deep(&mut machine, 0),
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
    log: Vec<String>,
}

impl AsyncFaultParentStateMachineContext for AsyncFaultContext {
    async fn kid_work(&mut self, event: &AsyncGo) -> Result<(), Fault> {
        core::future::ready(()).await;
        self.log.push("kid_work".into());
        if event.fail {
            Err(Fault(4))
        } else {
            Ok(())
        }
    }

    async fn parent_recover(&mut self, error: &Fault) -> Result<(), Fault> {
        core::future::ready(()).await;
        self.log.push(format!("parent_recover {}", error.0));
        Ok(())
    }
}

#[test]
fn async_composite_completion_fault_is_caught_by_the_parent_row() {
    smol::block_on(async {
        let mut machine = AsyncFaultParentStateMachine::new(AsyncFaultContext::default());
        machine.process_event(AsyncGo { fail: true }).await.unwrap();
        assert!(machine.is(&AsyncFaultParentStates::Idle));
        assert_eq!(machine.context().log, ["kid_work", "parent_recover 4"]);
        machine.context_mut().log.clear();
        machine
            .process_event(AsyncGo { fail: false })
            .await
            .unwrap();
        assert!(machine.is(&AsyncFaultParentStates::Idle));
        assert_eq!(machine.context().log, ["kid_work"]);
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
    log: Vec<String>,
}

impl OrthogonalFaultStateMachineContext for OrthogonalFaultContext {
    fn left_step(&mut self, event: &Tick) -> Result<(), Fault> {
        self.log.push("left_step".into());
        if event.fail {
            Err(Fault(5))
        } else {
            Ok(())
        }
    }

    fn left_fault(&mut self, error: &Fault) -> Result<(), Fault> {
        self.log.push(format!("left_fault {}", error.0));
        Ok(())
    }

    fn right_tick(&mut self, _: &Tick) -> Result<(), Fault> {
        self.log.push("right_tick".into());
        Ok(())
    }
}

#[test]
fn orthogonal_region_completion_fault_is_caught_by_its_exception_row() {
    let mut machine = OrthogonalFaultStateMachine::new(OrthogonalFaultContext::default());
    machine.process_event(Tick { fail: true }).unwrap();
    assert!(matches!(
        machine.state(0),
        Some(OrthogonalFaultStates::LeftIdle)
    ));
    assert_eq!(
        machine.context().log,
        ["right_tick", "left_step", "left_fault 5"]
    );
    machine.context_mut().log.clear();
    machine.process_event(Tick { fail: false }).unwrap();
    assert!(matches!(
        machine.state(0),
        Some(OrthogonalFaultStates::LeftIdle)
    ));
    assert_eq!(machine.context().log, ["right_tick", "left_step"]);
}
