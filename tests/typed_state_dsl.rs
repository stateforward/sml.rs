use sml::sml;

#[derive(Default)]
pub struct Disconnected;

#[derive(Default)]
pub struct Connected {
    id: u32,
}

pub struct Connect {
    id: u32,
}

pub struct Disconnect;

sml! {
    TypedStateDsl {
        *state<Disconnected> + event<Connect> / make_connected = state<Connected>,
         state<Connected> + event<Disconnect> = X,
    }
}

struct Context;

impl TypedStateDslStateMachineContext for Context {
    fn make_connected(&mut self, _source: &Disconnected, event: &Connect) -> Result<Connected, ()> {
        Ok(Connected { id: event.id })
    }
}

#[test]
fn state_type_infers_payload_and_defaults_the_initial_value() {
    let mut sm = TypedStateDslStateMachine::new(Context);
    sm.process_event(Connect { id: 42 }).unwrap();

    assert!(matches!(sm.state(), TypedStateDslStates::Connected(data) if data.id == 42));
    sm.process_event(Disconnect).unwrap();
    assert!(sm.is_terminated());
}

#[derive(Default)]
pub struct Off;
#[derive(Default)]
pub struct On;
pub struct TurnOn;

sml! {
    DefaultTypedTarget {
        *state<Off> + event<TurnOn> = state<On>,
    }
}

struct DefaultContext;
impl DefaultTypedTargetStateMachineContext for DefaultContext {}

#[test]
fn target_type_without_action_is_default_constructed() {
    let mut sm = DefaultTypedTargetStateMachine::new(DefaultContext);
    sm.process_event(TurnOn).unwrap();
    assert!(matches!(sm.state(), DefaultTypedTargetStates::On(_)));
}
