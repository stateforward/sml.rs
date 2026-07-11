use sml::sml;

sml! {
    Unexpected[
        states_attr: #[derive(Debug)],
        events_attr: #[derive(Debug)]
    ] {
        *Idle + Start = Running,
        Running + Reset = Idle,
        Running + Other = Running,
        Idle + unexpected<Reset> / specific_unexpected = Error,
        Idle + unexpected<_> / wildcard_unexpected = Error,
        Error + Reset = Idle,
    }
}

#[derive(Default)]
struct Context {
    log: Vec<&'static str>,
}

impl UnexpectedStateMachineContext for Context {
    fn specific_unexpected(&mut self) -> Result<(), ()> {
        self.log.push("specific");
        Ok(())
    }

    fn wildcard_unexpected(&mut self) -> Result<(), ()> {
        self.log.push("wildcard");
        Ok(())
    }
}

#[test]
fn specific_unexpected_event_has_priority_over_wildcard() {
    let mut sm = UnexpectedStateMachine::new(Context::default());

    assert!(matches!(
        sm.process_event(UnexpectedEvents::Reset),
        Ok(UnexpectedStates::Error)
    ));
    assert_eq!(sm.context().log, ["specific"]);
}

#[test]
fn wildcard_handles_any_other_unhandled_event() {
    let mut sm = UnexpectedStateMachine::new(Context::default());

    assert!(matches!(
        sm.process_event(UnexpectedEvents::Other),
        Ok(UnexpectedStates::Error)
    ));
    assert_eq!(sm.context().log, ["wildcard"]);
}

#[test]
fn normal_transition_wins_over_unexpected_handlers() {
    let mut sm = UnexpectedStateMachine::new(Context::default());

    assert!(matches!(
        sm.process_event(UnexpectedEvents::Start),
        Ok(UnexpectedStates::Running)
    ));
    assert!(sm.context().log.is_empty());
}

#[derive(Debug, PartialEq)]
pub struct Payload(u32);

sml! {
    UnexpectedData {
        *Idle + Begin = Waiting,
        Waiting + unexpected<Data>(Payload) / capture = Error,
    }
}

#[derive(Default)]
struct DataContext {
    captured: Option<Payload>,
}

impl UnexpectedDataStateMachineContext for DataContext {
    fn capture(&mut self, payload: Payload) -> Result<(), ()> {
        self.captured = Some(payload);
        Ok(())
    }
}

#[test]
fn specific_unexpected_event_preserves_owned_event_data() {
    let mut sm = UnexpectedDataStateMachine::new(DataContext::default());
    sm.process_event(UnexpectedDataEvents::Begin).unwrap();

    assert!(matches!(
        sm.process_event(UnexpectedDataEvents::Data(Payload(42))),
        Ok(UnexpectedDataStates::Error)
    ));
    assert_eq!(sm.context().captured, Some(Payload(42)));
}
