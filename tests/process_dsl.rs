use sml::sml;

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
    let mut sm = ProcessQueueDslStateMachine::new(Context);

    sm.process_event(Start).unwrap();
    assert!(sm.is(&ProcessQueueDslStates::Ready));

    sm.process_event(Kick).unwrap();
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
    let mut sm = DeferQueueDslStateMachine::new(DeferContext);

    sm.process_event(Deferred).unwrap();
    assert!(sm.is(&DeferQueueDslStates::Idle));

    sm.process_event(Unlock).unwrap();
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
        let mut machine = AsyncProcessQueueDslStateMachine::new(AsyncProcessContext);
        machine.process_event(AsyncEnable).await.unwrap();
        machine.process_event(AsyncStart).await.unwrap();
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
        let mut machine = AsyncDeferQueueDslStateMachine::new(AsyncDeferContext);
        machine.process_event(AsyncDeferred).await.unwrap();
        machine.process_event(AsyncUnlock).await.unwrap();
        assert!(machine.is_terminated());
    });
}
