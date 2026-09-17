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
    entries: usize,
}

impl InspectableStateMachineContext for Context {
    fn on_entry_idle(&mut self) {
        self.entries += 1;
    }
}

#[test]
fn state_can_be_injected_for_focused_transition_testing() {
    let mut sm: InspectableStateMachine<Context, TestingPolicies> =
        InspectableStateMachine::new_with_policy(Context::default(), TestingPolicies::default());

    let previous = sm.set_current_states(InspectableStates::Running);
    assert!(matches!(previous, InspectableStates::Idle));

    sm.process_event(InspectableEvents::Stop).unwrap();
    assert!(sm.is_terminated());
    assert!(sml::Terminated::is_terminated(&sm));
}

#[test]
fn current_state_can_be_visited_without_exposing_machine_internals() {
    let sm = InspectableStateMachine::new(Context::default());
    let name = sm.visit_current_state(|state| match state {
        InspectableStates::Idle => "idle",
        InspectableStates::Running => "running",
        InspectableStates::X => "terminal",
    });

    assert_eq!(name, "idle");
    assert!(!sm.is_terminated());
}

#[test]
fn initialize_runs_the_initial_entry_hook() {
    let mut sm = InspectableStateMachine::new(Context::default());

    assert!(matches!(sm.initialize().unwrap(), InspectableStates::Idle));
    assert_eq!(sm.context().entries, 1);
}

sml! {
    InitializeInjected {
        *Idle + Start = Ready,
         Ready + on_entry<_> / record_ready,
    }
}

#[derive(Default)]
struct InitializeInjectedContext {
    ready_entries: usize,
}

impl InitializeInjectedStateMachineContext for InitializeInjectedContext {
    fn record_ready(&mut self) -> Result<(), ()> {
        self.ready_entries += 1;
        Ok(())
    }
}

#[test]
fn initialize_runs_configured_entry_action_for_injected_state() {
    let mut sm: InitializeInjectedStateMachine<InitializeInjectedContext, TestingPolicies> =
        InitializeInjectedStateMachine::new_with_policy(
            InitializeInjectedContext::default(),
            TestingPolicies::default(),
        );

    sm.set_current_states(InitializeInjectedStates::Ready);
    sm.initialize().unwrap();

    assert_eq!(sm.context().ready_entries, 1);
}
