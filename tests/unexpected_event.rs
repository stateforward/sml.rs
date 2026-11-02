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
    log: core::cell::RefCell<Vec<&'static str>>,
}

impl UnexpectedStateMachineContext for &Context {
    fn specific_unexpected(&mut self) -> Result<(), ()> {
        self.log.borrow_mut().push("specific");
        Ok(())
    }

    fn wildcard_unexpected(&mut self) -> Result<(), ()> {
        self.log.borrow_mut().push("wildcard");
        Ok(())
    }
}

#[test]
fn specific_unexpected_event_has_priority_over_wildcard() {
    let context = Context::default();
    let mut sm = UnexpectedStateMachine::new(&context);

    assert!(matches!(
        sm.process_event(UnexpectedEvents::Reset),
        Ok(UnexpectedStates::Error)
    ));
    assert_eq!(*context.log.borrow(), ["specific"]);
}

#[test]
fn wildcard_handles_any_other_unhandled_event() {
    let context = Context::default();
    let mut sm = UnexpectedStateMachine::new(&context);

    assert!(matches!(
        sm.process_event(UnexpectedEvents::Other),
        Ok(UnexpectedStates::Error)
    ));
    assert_eq!(*context.log.borrow(), ["wildcard"]);
}

#[test]
fn normal_transition_wins_over_unexpected_handlers() {
    let context = Context::default();
    let mut sm = UnexpectedStateMachine::new(&context);

    assert!(matches!(
        sm.process_event(UnexpectedEvents::Start),
        Ok(UnexpectedStates::Running)
    ));
    assert!(context.log.borrow().is_empty());
}

#[derive(Debug, PartialEq, Eq)]
pub struct Payload(u32);

sml! {
    UnexpectedData {
        *Idle + Begin = Waiting,
        Waiting + unexpected<Data>(Payload) / capture = Error,
    }
}

#[derive(Default)]
struct DataContext {
    captured: core::cell::RefCell<Option<Payload>>,
}

impl UnexpectedDataStateMachineContext for &DataContext {
    fn capture(&mut self, payload: Payload) -> Result<(), ()> {
        *self.captured.borrow_mut() = Some(payload);
        Ok(())
    }
}

#[test]
fn specific_unexpected_event_preserves_owned_event_data() {
    let context = DataContext::default();
    let mut sm = UnexpectedDataStateMachine::new(&context);
    sm.process_event(UnexpectedDataEvents::Begin).unwrap();

    assert!(matches!(
        sm.process_event(UnexpectedDataEvents::Data(Payload(42))),
        Ok(UnexpectedDataStates::Error)
    ));
    assert_eq!(*context.captured.borrow(), Some(Payload(42)));
}
