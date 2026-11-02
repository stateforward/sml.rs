use sml::sml;

sml::sml_policies!(Testing {
    testing: sml::TestingPolicy,
});

pub struct Start;
pub struct Advance;
pub struct Kick;
pub struct Finish;

sml! {
    ProcessQueueDsl {
        *"idle"_s + event<Start> / process(Advance {}) = "queued"_s,
         "queued"_s + event<Advance> = "ready"_s,
         "ready"_s + event<Kick> / process(Finish {}),
         "ready"_s + event<Finish> = X,
    }
}

struct Context;
impl ProcessQueueDslStateMachineContext for Context {}

#[test]
fn cpp_process_action_dispatches_after_the_current_transition() {
    let mut sm = ProcessQueueDslStateMachine::new_with_policy(Context, Testing::default());

    assert_eq!(sm.process_event(Start), Ok(true));
    assert!(sm.is(&ProcessQueueDslStates::Ready));

    assert_eq!(sm.process_event(Kick), Ok(true));
    assert!(sm.is_terminated());
}

pub struct Deferred;
pub struct Unlock;

sml! {
    DeferQueueDsl {
        *"idle"_s + event<Deferred> / defer,
         "idle"_s + event<Unlock> = "unlocked"_s,
         "unlocked"_s + event<Deferred> = X,
    }
}

struct DeferContext;
impl DeferQueueDslStateMachineContext for DeferContext {}

#[test]
fn cpp_defer_action_retries_after_a_state_change() {
    let mut sm = DeferQueueDslStateMachine::new_with_policy(DeferContext, Testing::default());

    assert_eq!(sm.process_event(Deferred), Ok(true));
    assert!(sm.is(&DeferQueueDslStates::Idle));

    assert_eq!(sm.process_event(Unlock), Ok(true));
    assert!(sm.is_terminated());
}

pub struct AsyncEnable;
pub struct AsyncStart;
pub struct AsyncAdvance;

sml! {
    AsyncProcessQueueDsl {
        *"boot"_s + event<AsyncEnable> / async enable_async_process = "idle"_s,
         "idle"_s + event<AsyncStart> / (process(AsyncAdvance {}), after_process_scheduled) = "queued"_s,
         "queued"_s + event<AsyncAdvance> = X,
    }
}

struct AsyncProcessContext;

impl AsyncProcessQueueDslStateMachineContext for AsyncProcessContext {
    async fn enable_async_process(&mut self, _: &AsyncEnable) -> Result<(), ()> {
        core::future::ready(()).await;
        smol::future::yield_now().await;
        Ok(())
    }

    fn after_process_scheduled(&mut self, _: &AsyncStart) -> Result<(), ()> {
        Ok(())
    }
}

#[test]
fn async_flat_process_uses_allocation_free_iterative_dispatch() {
    smol::block_on(async {
        let mut machine = AsyncProcessQueueDslStateMachine::new_with_policy(
            AsyncProcessContext,
            Testing::default(),
        );
        assert_eq!(machine.process_event(AsyncEnable).await, Ok(true));
        assert_eq!(machine.process_event(AsyncStart).await, Ok(true));
        assert!(machine.is_terminated());
    });
}

pub struct AsyncDeferred;
pub struct AsyncUnlock;

sml! {
    AsyncDeferQueueDsl {
        *"idle"_s + event<AsyncDeferred> / defer,
         "idle"_s + event<AsyncUnlock> / async unlock_async = "unlocked"_s,
         "unlocked"_s + event<AsyncDeferred> = X,
    }
}

struct AsyncDeferContext;

impl AsyncDeferQueueDslStateMachineContext for AsyncDeferContext {
    async fn unlock_async(&mut self, _: &AsyncUnlock) -> Result<(), ()> {
        smol::future::yield_now().await;
        Ok(())
    }
}

#[test]
fn async_flat_defer_retries_after_awaited_state_change() {
    smol::block_on(async {
        let mut machine =
            AsyncDeferQueueDslStateMachine::new_with_policy(AsyncDeferContext, Testing::default());
        assert_eq!(machine.process_event(AsyncDeferred).await, Ok(true));
        assert_eq!(machine.process_event(AsyncUnlock).await, Ok(true));
        assert!(machine.is_terminated());
    });
}
