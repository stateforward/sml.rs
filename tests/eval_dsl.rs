mod support;

use sml::sml;

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
    order: support::SyncRefCell<Vec<&'static str>>,
}

impl EvalDslStateMachineContext for &Context {
    fn enabled(&self, event: &Run) -> Result<bool, ()> {
        Ok(event.enabled)
    }

    fn before(&mut self, _event: &Run) -> Result<(), ()> {
        self.order.borrow_mut().push("before");
        Ok(())
    }

    fn conditional(&mut self, _event: &Run) -> Result<(), ()> {
        self.order.borrow_mut().push("conditional");
        Ok(())
    }

    fn after(&mut self, _event: &Run) -> Result<(), ()> {
        self.order.borrow_mut().push("after");
        Ok(())
    }
}

#[test]
fn cpp_eval_action_preserves_sequence_order_and_guarding() {
    let enabled_context = Context::default();
    let mut enabled = EvalDslStateMachine::new(&enabled_context);
    enabled.process_event(Run { enabled: true }).unwrap();
    assert_eq!(
        *enabled_context.order.borrow(),
        ["before", "conditional", "after"]
    );

    let disabled_context = Context::default();
    let mut disabled = EvalDslStateMachine::new(&disabled_context);
    disabled.process_event(Run { enabled: false }).unwrap();
    assert_eq!(*disabled_context.order.borrow(), ["before", "after"]);
}

pub struct AsyncRun;

sml! {
    AsyncEvalDsl {
        *"idle"_s + event<AsyncRun> / (async_before, eval [async async_enabled] / async async_conditional) = X,
    }
}

#[derive(Default)]
struct AsyncContext {
    actions: support::SyncCell<usize>,
}

impl AsyncEvalDslStateMachineContext for &AsyncContext {
    async fn async_enabled(&self, _event: &AsyncRun) -> Result<bool, ()> {
        core::future::poll_fn(|_| core::task::Poll::Ready(())).await;
        Ok(true)
    }

    fn async_before(&mut self, _event: &AsyncRun) -> Result<(), ()> {
        self.actions.set(self.actions.get() + 1);
        Ok(())
    }

    async fn async_conditional(&mut self, _event: &AsyncRun) -> Result<(), ()> {
        core::future::poll_fn(|_| core::task::Poll::Ready(())).await;
        self.actions.set(self.actions.get() + 1);
        Ok(())
    }
}

#[test]
fn eval_supports_async_guards_and_actions() {
    smol::block_on(async {
        let context = AsyncContext::default();
        let mut sm = AsyncEvalDslStateMachine::new(&context);
        sm.process_event(AsyncRun).await.unwrap();
        assert_eq!(context.actions.get(), 2);
    });
}
