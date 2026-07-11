use sml::sml;

#[derive(Clone, Debug, PartialEq)]
pub struct Payload {
    value: u32,
    valid: bool,
}

sml! {
    Completion[
        states_attr: #[derive(Debug)],
        events_attr: #[derive(Debug)]
    ] {
        *Idle + Go(Payload) / origin = Step1,
        Idle + Other = Step1,
        Step1 + completion<Go>(Payload) [is_valid] / first_completion = Step2,
        Step2 + completion<Go>(Payload) / second_completion = Done,
    }
}

#[derive(Default)]
struct Context {
    calls: Vec<(&'static str, u32)>,
}

impl CompletionStateMachineContext for Context {
    fn origin(&mut self, payload: Payload) -> Result<(), ()> {
        self.calls.push(("origin", payload.value));
        Ok(())
    }

    fn is_valid(&self, payload: &Payload) -> Result<bool, ()> {
        Ok(payload.valid)
    }

    fn first_completion(&mut self, payload: Payload) -> Result<(), ()> {
        self.calls.push(("first", payload.value));
        Ok(())
    }

    fn second_completion(&mut self, payload: Payload) -> Result<(), ()> {
        self.calls.push(("second", payload.value));
        Ok(())
    }
}

#[test]
fn completion_transitions_chain_and_preserve_origin_data() {
    let mut sm = CompletionStateMachine::new(Context::default());

    let state = sm
        .process_event(CompletionEvents::Go(Payload {
            value: 42,
            valid: true,
        }))
        .unwrap();

    assert!(matches!(state, CompletionStates::Done));
    assert_eq!(
        sm.context().calls,
        [("origin", 42), ("first", 42), ("second", 42)]
    );
}

#[test]
fn completion_transition_is_specific_to_the_origin_event() {
    let mut sm = CompletionStateMachine::new(Context::default());

    let state = sm.process_event(CompletionEvents::Other).unwrap();

    assert!(matches!(state, CompletionStates::Step1));
    assert!(sm.context().calls.is_empty());
}

#[test]
fn failed_completion_guard_stops_the_chain_without_failing_origin_event() {
    let mut sm = CompletionStateMachine::new(Context::default());

    let state = sm
        .process_event(CompletionEvents::Go(Payload {
            value: 7,
            valid: false,
        }))
        .unwrap();

    assert!(matches!(state, CompletionStates::Step1));
    assert_eq!(sm.context().calls, [("origin", 7)]);
}

sml! {
    AsyncCompletion {
        *Idle + Go / async origin = Step,
        Step + completion<Go> / async complete = Done,
    }
}

#[derive(Default)]
struct AsyncContext {
    calls: Vec<&'static str>,
}

impl AsyncCompletionStateMachineContext for AsyncContext {
    async fn origin(&mut self) -> Result<(), ()> {
        self.calls.push("origin");
        Ok(())
    }

    async fn complete(&mut self) -> Result<(), ()> {
        self.calls.push("completion");
        Ok(())
    }
}

#[test]
fn completion_transitions_support_async_actions() {
    smol::block_on(async {
        let mut sm = AsyncCompletionStateMachine::new(AsyncContext::default());
        let state = sm.process_event(AsyncCompletionEvents::Go).await.unwrap();

        assert!(matches!(state, AsyncCompletionStates::Done));
        assert_eq!(sm.context().calls, ["origin", "completion"]);
    });
}

sml! {
    TemporaryCompletion[temporary_context: &mut u32] {
        *Idle + Go / origin_with_temporary = Step,
        Step + completion<Go> / completion_with_temporary = Done,
    }
}

struct TemporaryContext;

impl TemporaryCompletionStateMachineContext for TemporaryContext {
    fn origin_with_temporary(&mut self, value: &mut u32) -> Result<(), ()> {
        *value += 1;
        Ok(())
    }

    fn completion_with_temporary(&mut self, value: &mut u32) -> Result<(), ()> {
        *value += 10;
        Ok(())
    }
}

#[test]
fn completion_transitions_reborrow_mutable_temporary_context() {
    let mut sm = TemporaryCompletionStateMachine::new(TemporaryContext);
    let mut value = 0;

    let state = sm
        .process_event(&mut value, TemporaryCompletionEvents::Go)
        .unwrap();

    assert!(matches!(state, TemporaryCompletionStates::Done));
    assert_eq!(value, 11);
}

sml! {
    BorrowedCompletion {
        *Idle + Go(&'a Payload) = Step,
        Step + completion<Go>(&'a Payload) / capture_borrowed = Done,
    }
}

#[derive(Default)]
struct BorrowedContext {
    captured: Option<u32>,
}

impl BorrowedCompletionStateMachineContext for BorrowedContext {
    fn capture_borrowed(&mut self, payload: &Payload) -> Result<(), ()> {
        self.captured = Some(payload.value);
        Ok(())
    }
}

#[test]
fn completion_transitions_propagate_immutably_borrowed_origin_data() {
    let payload = Payload {
        value: 99,
        valid: true,
    };
    let mut sm = BorrowedCompletionStateMachine::new(BorrowedContext::default());

    let state = sm
        .process_event(BorrowedCompletionEvents::Go(&payload))
        .unwrap();

    assert!(matches!(state, BorrowedCompletionStates::Done));
    assert_eq!(sm.context().captured, Some(99));
}

sml! {
    AnonymousCompletion {
        *Initial + completion<_> / initialized = Ready,
        Ready + Go = Working,
        Working + completion<_> / stabilized = Done,
    }
}

#[derive(Default)]
struct AnonymousContext {
    calls: Vec<&'static str>,
}

impl AnonymousCompletionStateMachineContext for AnonymousContext {
    fn initialized(&mut self) -> Result<(), ()> {
        self.calls.push("initialized");
        Ok(())
    }

    fn stabilized(&mut self) -> Result<(), ()> {
        self.calls.push("stabilized");
        Ok(())
    }
}

#[test]
fn anonymous_completion_runs_during_initialization_and_after_events() {
    let mut sm = AnonymousCompletionStateMachine::new(AnonymousContext::default());

    assert!(matches!(
        sm.initialize().unwrap(),
        AnonymousCompletionStates::Ready
    ));
    assert!(matches!(
        sm.process_event(AnonymousCompletionEvents::Go).unwrap(),
        AnonymousCompletionStates::Done
    ));
    assert_eq!(sm.context().calls, ["initialized", "stabilized"]);
}

sml! {
    AsyncAnonymous {
        *Initial + completion<_> / async prepare = Ready,
        Ready + Go = Done,
    }
}

#[derive(Default)]
struct AsyncAnonymousContext {
    prepared: bool,
}

impl AsyncAnonymousStateMachineContext for AsyncAnonymousContext {
    async fn prepare(&mut self) -> Result<(), ()> {
        self.prepared = true;
        Ok(())
    }
}

#[test]
fn anonymous_initialization_supports_async_actions() {
    smol::block_on(async {
        let mut sm = AsyncAnonymousStateMachine::new(AsyncAnonymousContext::default());
        let state = sm.initialize().await.unwrap();

        assert!(matches!(state, AsyncAnonymousStates::Ready));
        assert!(sm.context().prepared);
    });
}
