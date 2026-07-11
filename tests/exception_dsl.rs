use sml::sml;

pub struct Run;
pub struct Finish;

sml! {
    ExceptionDsl {
        *"idle"_s + event<Run> / fail_action,
         "idle"_s + exception<_> / recover = "recovered"_s,
         "recovered"_s + event<Finish> = X,
    }
}

#[derive(Default)]
struct Context {
    recovered: bool,
}

impl ExceptionDslStateMachineContext for Context {
    fn fail_action(&mut self, _event: &Run) -> Result<(), ()> {
        Err(())
    }

    fn recover(&mut self) -> Result<(), ()> {
        self.recovered = true;
        Ok(())
    }
}

#[test]
fn wildcard_exception_transition_handles_action_result_error() {
    let mut sm = ExceptionDslStateMachine::new(Context::default());
    sm.process_event(Run).unwrap();

    assert!(sm.is(&ExceptionDslStates::Recovered));
    assert!(sm.context().recovered);
    sm.process_event(Finish).unwrap();
    assert!(sm.is_terminated());
}

pub struct AsyncRun;

sml! {
    AsyncExceptionDsl {
        *"idle"_s + event<AsyncRun> [async fail_guard] = X,
         "idle"_s + exception<_> / async async_recover = "recovered"_s,
    }
}

#[derive(Default)]
struct AsyncContext {
    recovered: bool,
}

impl AsyncExceptionDslStateMachineContext for AsyncContext {
    async fn fail_guard(&self, _event: &AsyncRun) -> Result<bool, ()> {
        Err(())
    }

    async fn async_recover(&mut self) -> Result<(), ()> {
        self.recovered = true;
        Ok(())
    }
}

#[test]
fn async_exception_transition_handles_guard_result_error() {
    smol::block_on(async {
        let mut sm = AsyncExceptionDslStateMachine::new(AsyncContext::default());
        sm.process_event(AsyncRun).await.unwrap();
        assert!(sm.is(&AsyncExceptionDslStates::Recovered));
        assert!(sm.context().recovered);
    });
}

#[derive(Debug, PartialEq)]
pub struct Failure {
    code: u32,
}

pub struct TypedRun;

sml! {
    TypedExceptionDsl {
        *"idle"_s + event<TypedRun> / typed_fail,
         "idle"_s + exception<Failure> / capture_failure = "recovered"_s,
    }
}

#[derive(Default)]
struct TypedContext {
    code: Option<u32>,
}

impl TypedExceptionDslStateMachineContext for TypedContext {
    fn typed_fail(&mut self, _event: &TypedRun) -> Result<(), Failure> {
        Err(Failure { code: 42 })
    }

    fn capture_failure(&mut self, error: &Failure) -> Result<(), Failure> {
        self.code = Some(error.code);
        Ok(())
    }
}

#[test]
fn typed_exception_infers_and_injects_callback_error() {
    let mut sm = TypedExceptionDslStateMachine::new(TypedContext::default());
    sm.process_event(TypedRun).unwrap();

    assert!(sm.is(&TypedExceptionDslStates::Recovered));
    assert_eq!(sm.context().code, Some(42));
}
