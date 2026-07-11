use sml::sml;

pub struct E1;
pub struct E2;
pub struct E3;
pub struct Reset;

sml! {
    Orthogonal {
        *"boot"_s / initialize_region_one = "idle"_s,
         "idle"_s + event<E1> [allow] / region_one = "s1"_s,
         "s1"_s + event<E2> = X,
         "s1"_s + on_entry<_> / entered_s1,
         "s1"_s + unexpected_event<Reset> / reset_region_one,

        *"idle2"_s + event<E2> = "s2"_s,
         "idle2"_s + event<E1> / region_two,
         "idle2"_s + unexpected_event<_> / unknown_region_two,
         "s2"_s / stabilize_region_two = "s3"_s,
         "s3"_s + event<E3> = X,
    }
}

#[derive(Default)]
struct Context {
    actions: usize,
    entries: usize,
    unexpected: usize,
    completions: usize,
    initializations: usize,
}
impl OrthogonalStateMachineContext for Context {
    fn initialize_region_one(&mut self) -> Result<(), ()> {
        self.initializations += 1;
        Ok(())
    }

    fn allow(&self, _event: &E1) -> Result<bool, ()> {
        Ok(true)
    }

    fn region_one(&mut self, _event: &E1) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    fn region_two(&mut self, _event: &E1) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    fn entered_s1(&mut self) -> Result<(), ()> {
        self.entries += 1;
        Ok(())
    }

    fn reset_region_one(&mut self, _event: &Reset) -> Result<(), ()> {
        self.unexpected += 1;
        Ok(())
    }

    fn unknown_region_two(&mut self) -> Result<(), ()> {
        self.unexpected += 1;
        Ok(())
    }

    fn stabilize_region_two(&mut self) -> Result<(), ()> {
        self.completions += 1;
        Ok(())
    }
}

#[test]
fn events_are_broadcast_to_all_orthogonal_regions() {
    let mut sm = OrthogonalStateMachine::new(Context::default());

    assert!(sm.is(&[OrthogonalStates::Boot, OrthogonalStates::Idle2]));
    sm.initialize().unwrap();
    assert!(sm.is(&[OrthogonalStates::Idle, OrthogonalStates::Idle2]));
    assert_eq!(sm.context().initializations, 1);
    sm.process_event(E1).unwrap();
    assert!(sm.is(&[OrthogonalStates::S1, OrthogonalStates::Idle2]));
    assert_eq!(sm.context().actions, 2);
    assert_eq!(sm.context().entries, 1);

    // Specific and wildcard unexpected handlers are independently broadcast.
    sm.process_event(Reset).unwrap();
    assert_eq!(sm.context().unexpected, 2);
    assert!(sm.is(&[OrthogonalStates::S1, OrthogonalStates::Idle2]));

    // E2 advances both active regions, just like sml.cpp.
    sm.process_event(E2).unwrap();
    assert!(sm.is(&[OrthogonalStates::X, OrthogonalStates::S3]));
    assert_eq!(sm.context().completions, 1);

    sm.process_event(E3).unwrap();
    assert!(sm.is_terminated());
}

pub struct AsyncEvent;

sml! {
    AsyncOrthogonal {
        *"left"_s + event<AsyncEvent> [async async_allowed] / async async_left = X,
        *"right"_s + event<AsyncEvent> / async async_right = X,
    }
}

#[derive(Default)]
struct AsyncContext {
    actions: usize,
}

impl AsyncOrthogonalStateMachineContext for AsyncContext {
    async fn async_allowed(&self, _event: &AsyncEvent) -> Result<bool, ()> {
        Ok(true)
    }

    async fn async_left(&mut self, _event: &AsyncEvent) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    async fn async_right(&mut self, _event: &AsyncEvent) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }
}

#[test]
fn async_orthogonal_callbacks_are_awaited_in_every_region() {
    smol::block_on(async {
        let mut sm = AsyncOrthogonalStateMachine::new(AsyncContext::default());
        sm.process_event(AsyncEvent).await.unwrap();
        assert!(sm.is_terminated());
        assert_eq!(sm.context().actions, 2);
    });
}

pub struct Origin {
    id: u32,
}

sml! {
    OriginOrthogonal {
        *"left"_s + event<Origin> = "left step"_s,
         "left step"_s + completion<Origin> / capture_left = X,

        *"right"_s + event<Origin> = "right step"_s,
         "right step"_s + completion<Origin> / capture_right = X,
    }
}

#[derive(Default)]
struct OriginContext {
    ids: [u32; 2],
}

impl OriginOrthogonalStateMachineContext for OriginContext {
    fn capture_left(&mut self, event: &Origin) -> Result<(), ()> {
        self.ids[0] = event.id;
        Ok(())
    }

    fn capture_right(&mut self, event: &Origin) -> Result<(), ()> {
        self.ids[1] = event.id;
        Ok(())
    }
}

#[test]
fn orthogonal_completion_borrows_the_shared_origin_event() {
    let mut sm = OriginOrthogonalStateMachine::new(OriginContext::default());
    sm.process_event(Origin { id: 42 }).unwrap();

    assert!(sm.is_terminated());
    assert_eq!(sm.context().ids, [42, 42]);
}

pub struct CustomEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CustomFailure {
    Left,
    Right,
}

sml! {
    CustomErrorOrthogonal[custom_error] {
        *"left"_s + event<CustomEvent> / fail_left = X,
        *"right"_s + event<CustomEvent> / fail_right = X,
    }
}

struct CustomErrorContext;

impl CustomErrorOrthogonalStateMachineContext for CustomErrorContext {
    type Error = CustomFailure;

    fn fail_left(&mut self, _: &CustomEvent) -> Result<(), Self::Error> {
        Err(CustomFailure::Left)
    }

    fn fail_right(&mut self, _: &CustomEvent) -> Result<(), Self::Error> {
        Err(CustomFailure::Right)
    }
}

#[test]
fn orthogonal_callbacks_preserve_custom_error_types() {
    let mut machine = CustomErrorOrthogonalStateMachine::new(CustomErrorContext);
    assert!(matches!(
        machine.process_event(CustomEvent),
        Err(CustomErrorOrthogonalError::ActionFailed(
            CustomFailure::Left
        ))
    ));
    assert!(machine.is_region(0, &CustomErrorOrthogonalStates::Left));
    assert!(machine.is_region(1, &CustomErrorOrthogonalStates::Right));
}

#[derive(Debug, PartialEq, Eq)]
pub struct Loaded(u32);

pub struct Load;
pub struct Bump(pub u32);

sml! {
    PayloadOrthogonal {
        *"idle"_s + event<Load> / make_loaded = state<Loaded>,
         state<Loaded> + event<Bump> [can_bump] / bump = state<Loaded>,

        *"watch"_s + event<Load> = X,
    }
}

struct PayloadContext;

impl PayloadOrthogonalStateMachineContext for PayloadContext {
    fn make_loaded(&mut self, _: &Load) -> Result<Loaded, ()> {
        Ok(Loaded(40))
    }

    fn can_bump(&self, state: &Loaded, event: &Bump) -> Result<bool, ()> {
        Ok(state.0 + event.0 <= 42)
    }

    fn bump(&mut self, state: &Loaded, event: &Bump) -> Result<Loaded, ()> {
        Ok(Loaded(state.0 + event.0))
    }
}

#[test]
fn orthogonal_regions_store_and_inject_typed_state_payloads() {
    let mut machine = PayloadOrthogonalStateMachine::new(PayloadContext);
    machine.process_event(Load).unwrap();
    assert!(matches!(
        machine.state(0),
        Some(PayloadOrthogonalStates::Loaded(Loaded(40)))
    ));
    assert!(machine.is_region(1, &PayloadOrthogonalStates::X));

    machine.process_event(Bump(2)).unwrap();
    assert!(matches!(
        machine.state(0),
        Some(PayloadOrthogonalStates::Loaded(Loaded(42)))
    ));
}

#[derive(Default)]
pub struct InitialPayload(u32);

#[derive(Default)]
pub struct ReadyPayload(u32);

pub struct PayloadAdvance;

sml! {
    PayloadLifecycleOrthogonal {
        *state<InitialPayload> + on_entry<_> / entered_initial,
         state<InitialPayload> + on_exit<_> / exited_initial,
         state<InitialPayload> + event<PayloadAdvance> = state<ReadyPayload>,
         state<ReadyPayload> + on_entry<_> / entered_ready,

        *"other"_s + event<PayloadAdvance> = X,
    }
}

#[derive(Default)]
struct PayloadLifecycleContext {
    observations: Vec<u32>,
}

impl PayloadLifecycleOrthogonalStateMachineContext for PayloadLifecycleContext {
    fn entered_initial(&mut self, state: &InitialPayload) -> Result<(), ()> {
        self.observations.push(state.0);
        Ok(())
    }

    fn exited_initial(&mut self, state: &InitialPayload) -> Result<(), ()> {
        self.observations.push(state.0 + 1);
        Ok(())
    }

    fn entered_ready(&mut self, state: &ReadyPayload) -> Result<(), ()> {
        self.observations.push(state.0 + 2);
        Ok(())
    }
}

#[test]
fn orthogonal_payloads_support_default_initial_targets_and_lifecycle() {
    let mut machine =
        PayloadLifecycleOrthogonalStateMachine::new(PayloadLifecycleContext::default());
    machine.initialize().unwrap();
    machine.process_event(PayloadAdvance).unwrap();

    assert!(matches!(
        machine.state(0),
        Some(PayloadLifecycleOrthogonalStates::ReadyPayload(
            ReadyPayload(0)
        ))
    ));
    assert_eq!(machine.context().observations, [0, 1, 2]);
}

pub struct AsyncPayloadEvent;
pub struct AsyncPayload(u8);

sml! {
    AsyncPayloadOrthogonal {
        *"idle"_s + event<AsyncPayloadEvent> / async make_async_payload = state<AsyncPayload>,
        *"other"_s + event<AsyncPayloadEvent> = X,
    }
}

struct AsyncPayloadContext;

impl AsyncPayloadOrthogonalStateMachineContext for AsyncPayloadContext {
    async fn make_async_payload(&mut self, _: &AsyncPayloadEvent) -> Result<AsyncPayload, ()> {
        smol::future::yield_now().await;
        Ok(AsyncPayload(7))
    }
}

#[test]
fn async_orthogonal_actions_can_produce_state_payloads() {
    smol::block_on(async {
        let mut machine = AsyncPayloadOrthogonalStateMachine::new(AsyncPayloadContext);
        machine.process_event(AsyncPayloadEvent).await.unwrap();
        assert!(matches!(
            machine.state(0),
            Some(AsyncPayloadOrthogonalStates::AsyncPayload(AsyncPayload(7)))
        ));
    });
}

pub struct TemporaryEvent;

sml! {
    TemporaryOrthogonal[temporary_context: &mut usize] {
        *"left"_s + on_entry<_> / enter_left,
         "left"_s + event<TemporaryEvent> / advance_left = "left done"_s,
         "left done"_s + completion<_> / complete_left = X,

        *"right"_s + event<TemporaryEvent> [allow_right] / advance_right = X,
    }
}

struct TemporaryContext;

impl TemporaryOrthogonalStateMachineContext for TemporaryContext {
    fn enter_left(&mut self, temporary: &mut usize) -> Result<(), ()> {
        *temporary += 1;
        Ok(())
    }

    fn advance_left(&mut self, temporary: &mut usize, _: &TemporaryEvent) -> Result<(), ()> {
        *temporary += 2;
        Ok(())
    }

    fn complete_left(&mut self, temporary: &mut usize) -> Result<(), ()> {
        *temporary += 4;
        Ok(())
    }

    fn allow_right(&self, temporary: &mut usize, _: &TemporaryEvent) -> Result<bool, ()> {
        *temporary += 8;
        Ok(true)
    }

    fn advance_right(&mut self, _: &mut usize, _: &TemporaryEvent) -> Result<(), ()> {
        Ok(())
    }
}

#[test]
fn orthogonal_callbacks_reborrow_mutable_temporary_context() {
    let mut machine = TemporaryOrthogonalStateMachine::new(TemporaryContext);
    let mut temporary = 0;
    machine.initialize(&mut temporary).unwrap();
    machine
        .process_event(&mut temporary, TemporaryEvent)
        .unwrap();
    assert_eq!(temporary, 15);
    assert!(machine.is_terminated());
}

pub struct OrthogonalEvalEvent;

sml! {
    EvalOrthogonal {
        *"left"_s + event<OrthogonalEvalEvent> / (before_eval, eval [eval_enabled] / conditional_eval, after_eval) = X,
        *"right"_s + event<OrthogonalEvalEvent> / (async async_before_eval, eval [async async_eval_enabled] / async async_conditional_eval) = X,
    }
}

#[derive(Default)]
struct EvalContext {
    calls: Vec<&'static str>,
}

impl EvalOrthogonalStateMachineContext for EvalContext {
    fn before_eval(&mut self, _: &OrthogonalEvalEvent) -> Result<(), ()> {
        self.calls.push("before");
        Ok(())
    }

    fn eval_enabled(&self, _: &OrthogonalEvalEvent) -> Result<bool, ()> {
        Ok(true)
    }

    fn conditional_eval(&mut self, _: &OrthogonalEvalEvent) -> Result<(), ()> {
        self.calls.push("conditional");
        Ok(())
    }

    fn after_eval(&mut self, _: &OrthogonalEvalEvent) -> Result<(), ()> {
        self.calls.push("after");
        Ok(())
    }

    async fn async_before_eval(&mut self, _: &OrthogonalEvalEvent) -> Result<(), ()> {
        self.calls.push("async before");
        Ok(())
    }

    async fn async_eval_enabled(&self, _: &OrthogonalEvalEvent) -> Result<bool, ()> {
        Ok(true)
    }

    async fn async_conditional_eval(&mut self, _: &OrthogonalEvalEvent) -> Result<(), ()> {
        self.calls.push("async conditional");
        Ok(())
    }
}

#[test]
fn orthogonal_eval_preserves_action_position_and_async_callbacks() {
    smol::block_on(async {
        let mut machine = EvalOrthogonalStateMachine::new(EvalContext::default());
        machine.process_event(OrthogonalEvalEvent).await.unwrap();
        assert_eq!(
            machine.context().calls,
            [
                "before",
                "conditional",
                "after",
                "async before",
                "async conditional"
            ]
        );
    });
}

pub struct QueueStart;
pub struct QueueAdvance;
#[derive(Clone)]
pub struct QueueDeferred;
pub struct QueueUnlock;

sml! {
    ProcessOrthogonal {
        *"idle"_s + event<QueueStart> / process(QueueAdvance {}) = "queued"_s,
         "queued"_s + event<QueueAdvance> = X,
        *"watch"_s + event<QueueStart> = X,
    }
}

sml! {
    DeferOrthogonal {
        *"locked"_s + event<QueueDeferred> / defer,
         "locked"_s + event<QueueUnlock> = "unlocked"_s,
         "unlocked"_s + event<QueueDeferred> = X,
        *"watch"_s + event<QueueUnlock> = X,
    }
}

struct QueueContext;
impl ProcessOrthogonalStateMachineContext for QueueContext {}
impl DeferOrthogonalStateMachineContext for QueueContext {}

#[test]
fn orthogonal_process_dispatches_after_installing_the_target_state() {
    let mut machine = ProcessOrthogonalStateMachine::new(QueueContext);
    machine.process_event(QueueStart).unwrap();
    assert!(machine.is_terminated());
}

#[test]
fn orthogonal_defer_retries_owned_events_after_a_state_change() {
    let mut machine = DeferOrthogonalStateMachine::new(QueueContext);
    machine.process_event(QueueDeferred).unwrap();
    assert!(machine.is_region(0, &DeferOrthogonalStates::Locked));
    machine.process_event(QueueUnlock).unwrap();
    assert!(machine.is_terminated());
}

pub struct AsyncQueueStart;
pub struct AsyncQueueAdvance;
#[derive(Clone)]
pub struct AsyncQueueDeferred;
pub struct AsyncQueueUnlock;

sml! {
    AsyncProcessOrthogonal {
        *"idle"_s + event<AsyncQueueStart> / process(AsyncQueueAdvance {}) = "queued"_s,
         "queued"_s + event<AsyncQueueAdvance> = X,
        *"watch"_s + event<AsyncQueueStart> / async async_queue_action = X,
    }
}

sml! {
    AsyncDeferOrthogonal {
        *"locked"_s + event<AsyncQueueDeferred> / defer,
         "locked"_s + event<AsyncQueueUnlock> / async async_unlock = "unlocked"_s,
         "unlocked"_s + event<AsyncQueueDeferred> = X,
        *"watch"_s + event<AsyncQueueUnlock> = X,
    }
}

struct AsyncQueueContext;

impl AsyncProcessOrthogonalStateMachineContext for AsyncQueueContext {
    async fn async_queue_action(&mut self, _: &AsyncQueueStart) -> Result<(), ()> {
        smol::future::yield_now().await;
        Ok(())
    }
}

impl AsyncDeferOrthogonalStateMachineContext for AsyncQueueContext {
    async fn async_unlock(&mut self, _: &AsyncQueueUnlock) -> Result<(), ()> {
        smol::future::yield_now().await;
        Ok(())
    }
}

#[test]
fn async_orthogonal_process_uses_allocation_free_iterative_dispatch() {
    smol::block_on(async {
        let mut machine = AsyncProcessOrthogonalStateMachine::new(AsyncQueueContext);
        machine.process_event(AsyncQueueStart).await.unwrap();
        assert!(machine.is_terminated());
    });
}

#[test]
fn async_orthogonal_defer_retries_after_awaited_state_change() {
    smol::block_on(async {
        let mut machine = AsyncDeferOrthogonalStateMachine::new(AsyncQueueContext);
        machine.process_event(AsyncQueueDeferred).await.unwrap();
        machine.process_event(AsyncQueueUnlock).await.unwrap();
        assert!(machine.is_terminated());
    });
}

#[derive(Debug, PartialEq, Eq)]
pub struct OrthogonalFailure(u32);
pub struct OrthogonalFail;

sml! {
    ExceptionOrthogonal {
        *"left"_s + event<OrthogonalFail> / async fail_orthogonal,
         "left"_s + exception<OrthogonalFailure> / async recover_typed = "recovered"_s,
         "recovered"_s + completion<_> = X,

        *"right"_s + event<OrthogonalFail> = X,
         "right"_s + exception<_> / recover_wildcard = X,
    }
}

#[derive(Default)]
struct ExceptionOrthogonalContext {
    code: Option<u32>,
    wildcard: bool,
}

impl ExceptionOrthogonalStateMachineContext for ExceptionOrthogonalContext {
    async fn fail_orthogonal(&mut self, _: &OrthogonalFail) -> Result<(), OrthogonalFailure> {
        Err(OrthogonalFailure(42))
    }

    async fn recover_typed(&mut self, error: &OrthogonalFailure) -> Result<(), OrthogonalFailure> {
        self.code = Some(error.0);
        Ok(())
    }

    fn recover_wildcard(&mut self) -> Result<(), OrthogonalFailure> {
        self.wildcard = true;
        Ok(())
    }
}

#[test]
fn orthogonal_exception_rows_route_typed_async_and_wildcard_failures() {
    smol::block_on(async {
        let mut machine =
            ExceptionOrthogonalStateMachine::new(ExceptionOrthogonalContext::default());
        machine.process_event(OrthogonalFail).await.unwrap();
        assert!(machine.is_terminated());
        assert_eq!(machine.context().code, Some(42));
        assert!(machine.context().wildcard);
    });
}
