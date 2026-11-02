use sml::sml;

sml::sml_policies!(Testing {
    testing: sml::TestingPolicy,
});

pub struct Run {
    enabled: bool,
}

sml! {
    EvalDsl {
        *"idle"_s + event<Run> / (before, eval [enabled] / conditional, after) = X,
    }
}

#[derive(Default)]
struct Context {
    order: Vec<&'static str>,
}

impl EvalDslStateMachineContext for Context {
    fn enabled(&self, event: &Run) -> Result<bool, ()> {
        Ok(event.enabled)
    }

    fn before(&mut self, _event: &Run) -> Result<(), ()> {
        self.order.push("before");
        Ok(())
    }

    fn conditional(&mut self, _event: &Run) -> Result<(), ()> {
        self.order.push("conditional");
        Ok(())
    }

    fn after(&mut self, _event: &Run) -> Result<(), ()> {
        self.order.push("after");
        Ok(())
    }
}

#[test]
fn cpp_eval_action_preserves_sequence_order_and_guarding() {
    let mut enabled = EvalDslStateMachine::new_with_policy(Context::default(), Testing::default());
    assert_eq!(enabled.process_event(Run { enabled: true }), Ok(true));
    assert_eq!(enabled.context().order, ["before", "conditional", "after"]);

    let mut disabled = EvalDslStateMachine::new_with_policy(Context::default(), Testing::default());
    assert_eq!(disabled.process_event(Run { enabled: false }), Ok(true));
    assert_eq!(disabled.context().order, ["before", "after"]);
}

pub struct AsyncRun;

sml! {
    AsyncEvalDsl {
        *"idle"_s + event<AsyncRun> / (async_before, eval [async async_enabled] / async async_conditional) = X,
    }
}

#[derive(Default)]
struct AsyncContext {
    actions: usize,
}

impl AsyncEvalDslStateMachineContext for AsyncContext {
    async fn async_enabled(&self, _event: &AsyncRun) -> Result<bool, ()> {
        core::future::poll_fn(|_| core::task::Poll::Ready(())).await;
        Ok(true)
    }

    fn async_before(&mut self, _event: &AsyncRun) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    async fn async_conditional(&mut self, _event: &AsyncRun) -> Result<(), ()> {
        core::future::poll_fn(|_| core::task::Poll::Ready(())).await;
        self.actions += 1;
        Ok(())
    }
}

#[test]
fn eval_supports_async_guards_and_actions() {
    smol::block_on(async {
        let mut sm =
            AsyncEvalDslStateMachine::new_with_policy(AsyncContext::default(), Testing::default());
        assert_eq!(sm.process_event(AsyncRun).await, Ok(true));
        assert_eq!(sm.context().actions, 2);
    });
}
