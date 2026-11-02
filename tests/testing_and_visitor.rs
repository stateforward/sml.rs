use sml::{sml, sml_policies};

sml_policies!(TestingPolicies {
    testing: sml::TestingPolicy,
});

sml! {
    Inspectable[states_attr: #[derive(Debug)]] {
        *Idle + Start = Running,
        Running + Stop = X,
    }
}

#[derive(Default)]
struct Context {
    entries: core::cell::Cell<usize>,
}

impl InspectableStateMachineContext for &Context {
    fn on_entry_idle(&mut self) {
        self.entries.set(self.entries.get() + 1);
    }
}

#[test]
fn state_can_be_injected_for_focused_transition_testing() {
    let context = Context::default();
    let mut sm: InspectableStateMachine<&Context, TestingPolicies> =
        InspectableStateMachine::new_with_policy(&context, TestingPolicies::default());

    let previous = sm.set_current_states(InspectableStates::Running);
    assert!(matches!(previous, InspectableStates::Idle));

    sm.process_event(InspectableEvents::Stop).unwrap();
    assert!(sm.is(&InspectableStates::X));
}

#[test]
fn current_state_can_be_visited_without_exposing_machine_internals() {
    let context = Context::default();
    let sm = InspectableStateMachine::new(&context);
    let mut names = Vec::new();
    sm.visit_current_states(|state| {
        names.push(match state {
            InspectableStates::Idle => "idle",
            InspectableStates::Running => "running",
            InspectableStates::X => "terminal",
        });
    });

    assert_eq!(names, ["idle"]);
    assert!(!sm.is(&InspectableStates::X));
}

#[test]
fn initialize_runs_the_initial_entry_hook() {
    let context = Context::default();
    let mut sm = InspectableStateMachine::new(&context);

    assert!(matches!(sm.initialize().unwrap(), InspectableStates::Idle));
    assert_eq!(context.entries.get(), 1);
}

sml! {
    InitializeInjected {
        *Idle + Start = Ready,
         Ready + on_entry<_> / record_ready,
    }
}

#[derive(Default)]
struct InitializeInjectedContext {
    ready_entries: core::cell::Cell<usize>,
}

impl InitializeInjectedStateMachineContext for &InitializeInjectedContext {
    fn record_ready(&mut self) -> Result<(), ()> {
        self.ready_entries.set(self.ready_entries.get() + 1);
        Ok(())
    }
}

#[test]
fn initialize_runs_configured_entry_action_for_injected_state() {
    let context = InitializeInjectedContext::default();
    let mut sm: InitializeInjectedStateMachine<&InitializeInjectedContext, TestingPolicies> =
        InitializeInjectedStateMachine::new_with_policy(&context, TestingPolicies::default());

    sm.set_current_states(InitializeInjectedStates::Ready);
    sm.initialize().unwrap();

    assert_eq!(context.ready_entries.get(), 1);
}
