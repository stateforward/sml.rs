use sml::sml;

pub struct RootBroadcast;

sml! {
    RootRegionChild {
        *"waiting"_s + event<RootBroadcast> / child_broadcast = X,
    }

    BroadcastCompositeRoot {
        *state<RootRegionChild> / child_region_completed = X,
        *"parallel"_s + event<RootBroadcast> / sibling_broadcast = X,
    }
}

#[derive(Default)]
struct OrthogonalRootContext {
    child_events: usize,
    sibling_events: usize,
    completions: usize,
}

impl BroadcastCompositeRootStateMachineContext for OrthogonalRootContext {
    fn child_broadcast(&mut self, _event: &RootBroadcast) -> Result<(), ()> {
        self.child_events += 1;
        Ok(())
    }

    fn sibling_broadcast(&mut self, _event: &RootBroadcast) -> Result<(), ()> {
        self.sibling_events += 1;
        Ok(())
    }

    fn child_region_completed(&mut self) -> Result<(), ()> {
        self.completions += 1;
        Ok(())
    }
}

#[test]
fn orthogonal_root_broadcasts_after_child_first_dispatch_per_region() {
    let mut sm = BroadcastCompositeRootStateMachine::new(OrthogonalRootContext::default());
    sm.initialize().unwrap();

    assert!(sm.root_region_child_is_active());
    sm.process_event(RootBroadcast).unwrap();

    assert!(sm.is_terminated());
    assert!(sm.is_region(0, &BroadcastCompositeRootStates::X));
    assert!(sm.is_region(1, &BroadcastCompositeRootStates::X));
    assert_eq!(sm.context().child_events, 1);
    assert_eq!(sm.context().sibling_events, 1);
    assert_eq!(sm.context().completions, 1);
}

pub struct NestedRegionEvent;

sml! {
    NestedRegionLeaf {
        *"leaf"_s + event<NestedRegionEvent> = X,
    }

    OrthogonalOwnerWithChild {
         state<NestedRegionLeaf> + event<NestedRegionEvent> / parent_region_should_not_handle = "bad"_s,
        *state<NestedRegionLeaf> / nested_leaf_completed = X,
        *"sibling"_s + event<NestedRegionEvent> = X,
    }

    ScalarRootWithNestedOrthogonal {
        *state<OrthogonalOwnerWithChild> / nested_regions_completed = X,
    }
}

#[derive(Default)]
struct NestedRegionContext {
    leaf_completions: usize,
    root_completions: usize,
    incorrect_parent_dispatches: usize,
}

impl ScalarRootWithNestedOrthogonalStateMachineContext for NestedRegionContext {
    fn nested_leaf_completed(&mut self) -> Result<(), ()> {
        self.leaf_completions += 1;
        Ok(())
    }

    fn nested_regions_completed(&mut self) -> Result<(), ()> {
        self.root_completions += 1;
        Ok(())
    }

    fn parent_region_should_not_handle(&mut self, _: &NestedRegionEvent) -> Result<(), ()> {
        self.incorrect_parent_dispatches += 1;
        Ok(())
    }
}

#[test]
fn embedded_orthogonal_nodes_can_own_composite_children() {
    let mut sm = ScalarRootWithNestedOrthogonalStateMachine::new(NestedRegionContext::default());
    sm.initialize().unwrap();
    assert!(sm.nested_region_leaf_is_active());

    sm.process_event(NestedRegionEvent).unwrap();

    assert!(sm.is_terminated());
    assert_eq!(sm.context().leaf_completions, 1);
    assert_eq!(sm.context().root_completions, 1);
    assert_eq!(sm.context().incorrect_parent_dispatches, 0);
}

pub struct E1;
pub struct E2;
pub struct E3;
pub struct E4;
pub struct E5;
pub struct E6;
pub struct Reset;
pub struct Unknown;

sml! {
    Sub {
        *"idle"_s(H) + event<E3> / in_child = "s1"_s,
         "idle"_s + unexpected_event<Reset> / child_unexpected,
         "s1"_s + event<E4> / finish_child = X,
         "s1"_s + on_entry<_> / child_s1_entry,
         "s1"_s + on_exit<_> / child_s1_exit,
    }

    Composite {
        *"idle"_s + event<E1> = "s1"_s,
         "idle"_s + on_entry<_> / parent_initial_entry,
         "s1"_s + event<E2> / enter_child = state<Sub>,
         state<Sub> + event<E5> / exit_child = "outside"_s,
         state<Sub> + unexpected_event<Unknown> / parent_unexpected,
         state<Sub> + on_entry<_> / parent_child_entry,
         state<Sub> + on_exit<_> / parent_child_exit,
         "outside"_s + event<E2> / enter_child = state<Sub>,
         "outside"_s + event<E6> = X,
    }
}

#[derive(Default)]
struct Context {
    actions: usize,
    entries: usize,
    exits: usize,
    unexpected: usize,
}

impl CompositeStateMachineContext for Context {
    fn in_child(&mut self, _event: &E3) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    fn finish_child(&mut self, _event: &E4) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    fn enter_child(&mut self, _event: &E2) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    fn exit_child(&mut self, _event: &E5) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    fn child_unexpected(&mut self, _event: &Reset) -> Result<(), ()> {
        self.unexpected += 1;
        Ok(())
    }

    fn parent_unexpected(&mut self, _event: &Unknown) -> Result<(), ()> {
        self.unexpected += 1;
        Ok(())
    }

    fn parent_initial_entry(&mut self) -> Result<(), ()> {
        self.entries += 1;
        Ok(())
    }

    fn parent_child_entry(&mut self) -> Result<(), ()> {
        self.entries += 1;
        Ok(())
    }

    fn parent_child_exit(&mut self) -> Result<(), ()> {
        self.exits += 1;
        Ok(())
    }

    fn child_s1_entry(&mut self) -> Result<(), ()> {
        self.entries += 1;
        Ok(())
    }

    fn child_s1_exit(&mut self) -> Result<(), ()> {
        self.exits += 1;
        Ok(())
    }
}

#[test]
fn state_sub_routes_child_first_and_preserves_child_state() {
    let mut sm = CompositeStateMachine::new(Context::default());

    assert!(sm.is(&CompositeStates::Idle));
    assert!(sm.is_child(&CompositeSubStates::Idle));
    assert!(sm.visit_current_state(|state| state == &CompositeStates::Idle));
    assert!(sm.visit_child_state(|state| state == &CompositeSubStates::Idle));
    sm.initialize().unwrap();
    assert_eq!(sm.context().entries, 1);

    sm.process_event(E1).unwrap();
    sm.process_event(E2).unwrap();
    assert!(sm.is(&CompositeStates::Sub));
    assert!(sm.child_is_active());
    assert_eq!(sm.context().entries, 2);

    sm.process_event(Reset).unwrap();
    sm.process_event(Unknown).unwrap();
    assert_eq!(sm.context().unexpected, 2);

    sm.process_event(E3).unwrap();
    assert!(sm.is_child(&CompositeSubStates::S1));
    assert_eq!(sm.context().entries, 3);

    // E5 bubbles to the parent and deactivates the child without resetting it.
    sm.process_event(E5).unwrap();
    assert!(sm.is(&CompositeStates::Outside));
    assert_eq!(sm.context().exits, 2);
    assert!(sm.is_child(&CompositeSubStates::S1));

    // Re-entering state<Sub> restores the retained shallow history.
    sm.process_event(E2).unwrap();
    assert!(sm.child_is_active());
    assert!(sm.is_child(&CompositeSubStates::S1));
    assert_eq!(sm.context().entries, 5);

    sm.process_event(E4).unwrap();
    assert!(sm.is_child(&CompositeSubStates::X));
    assert_eq!(sm.context().exits, 3);

    // The child does not handle E5, so it bubbles to the parent table.
    sm.process_event(E5).unwrap();
    sm.process_event(E6).unwrap();
    assert!(sm.is_terminated());
    assert_eq!(sm.context().actions, 6);
    assert_eq!(sm.context().exits, 4);
}

pub struct AsyncEnter;
pub struct AsyncChildEvent;

sml! {
    AsyncSub {
        *"idle"_s + event<AsyncChildEvent> [async child_ready] / async async_child = X,
    }

    AsyncParent {
        *"idle"_s + event<AsyncEnter> / async async_enter = state<AsyncSub>,
         state<AsyncSub> / async parent_completed = X,
    }
}

#[derive(Default)]
struct AsyncContext {
    actions: usize,
}

impl AsyncParentStateMachineContext for AsyncContext {
    async fn child_ready(&self, _event: &AsyncChildEvent) -> Result<bool, ()> {
        Ok(true)
    }

    async fn async_child(&mut self, _event: &AsyncChildEvent) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    async fn async_enter(&mut self, _event: &AsyncEnter) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    async fn parent_completed(&mut self) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }
}

#[test]
fn async_composite_callbacks_are_awaited_child_first() {
    smol::block_on(async {
        let mut sm = AsyncParentStateMachine::new(AsyncContext::default());
        sm.process_event(AsyncEnter).await.unwrap();
        assert!(!sm.is_terminated());
        sm.process_event(AsyncChildEvent).await.unwrap();

        assert!(sm.is_child(&AsyncParentAsyncSubStates::X));
        assert!(sm.is_terminated());
        assert_eq!(sm.context().actions, 3);
    });
}

pub struct NhEnter;
pub struct NhAdvance;
pub struct NhLeave;

sml! {
    NonHistorySub {
        *"idle"_s + event<NhAdvance> = "advanced"_s,
    }

    NonHistoryParent {
        *"outside"_s + event<NhEnter> = state<NonHistorySub>,
         state<NonHistorySub> + event<NhLeave> = "outside"_s,
    }
}

struct NonHistoryContext;
impl NonHistoryParentStateMachineContext for NonHistoryContext {}

#[test]
fn child_without_h_marker_resets_when_reentered() {
    let mut sm = NonHistoryParentStateMachine::new(NonHistoryContext);
    sm.process_event(NhEnter).unwrap();
    sm.process_event(NhAdvance).unwrap();
    assert!(sm.is_child(&NonHistoryParentNonHistorySubStates::Advanced));

    sm.process_event(NhLeave).unwrap();
    sm.process_event(NhEnter).unwrap();
    assert!(sm.is_child(&NonHistoryParentNonHistorySubStates::Idle));
}

pub struct OriginEnter;
pub struct CompositeOrigin {
    id: u32,
}

sml! {
    OriginSub {
        *"idle"_s + event<CompositeOrigin> = "step"_s,
         "step"_s + completion<CompositeOrigin> / capture_child_origin = X,
    }

    OriginParent {
        *"outside"_s + event<OriginEnter> = state<OriginSub>,
         state<OriginSub> + completion<CompositeOrigin> / capture_parent_origin = X,
    }
}

#[derive(Default)]
struct OriginContext {
    ids: [u32; 2],
}

impl OriginParentStateMachineContext for OriginContext {
    fn capture_child_origin(&mut self, event: &CompositeOrigin) -> Result<(), ()> {
        self.ids[0] = event.id;
        Ok(())
    }

    fn capture_parent_origin(&mut self, event: &CompositeOrigin) -> Result<(), ()> {
        self.ids[1] = event.id;
        Ok(())
    }
}

#[test]
fn composite_completion_preserves_origin_across_child_and_parent() {
    let mut sm = OriginParentStateMachine::new(OriginContext::default());
    sm.process_event(OriginEnter).unwrap();
    sm.process_event(CompositeOrigin { id: 77 }).unwrap();

    assert!(sm.is_child(&OriginParentOriginSubStates::X));
    assert!(sm.is_terminated());
    assert_eq!(sm.context().ids, [77, 77]);
}

pub struct CompositeFailureEvent;

#[derive(Debug, PartialEq, Eq)]
enum CompositeFailure {
    Child,
}

sml! {
    FailureChild {
        *"idle"_s + event<CompositeFailureEvent> / fail_child = X,
    }

    FailureParent[custom_error] {
        *state<FailureChild> + event<Reset> = X,
    }
}

struct FailureContext;

impl FailureParentStateMachineContext for FailureContext {
    type Error = CompositeFailure;

    fn fail_child(&mut self, _: &CompositeFailureEvent) -> Result<(), Self::Error> {
        Err(CompositeFailure::Child)
    }
}

#[test]
fn composite_callbacks_preserve_custom_error_types() {
    let mut machine = FailureParentStateMachine::new(FailureContext);
    assert!(matches!(
        machine.process_event(CompositeFailureEvent),
        Err(FailureParentError::ActionFailed(CompositeFailure::Child))
    ));
    assert!(machine.child_is_active());
}

#[derive(Default)]
pub struct ParentIdle(u32);
pub struct ParentReady(u32);
#[derive(Default)]
pub struct ParentDone(u32);
#[derive(Default)]
pub struct ChildIdle(u32);
pub struct ChildLoaded(u32);

pub struct PreparePayload;
pub struct EnterPayloadChild;
pub struct FillPayloadChild;
pub struct FinishPayloadChild;

sml! {
    PayloadChild {
        *state<ChildIdle> + on_entry<_> / entered_child_idle,
         state<ChildIdle> + event<FillPayloadChild> / make_child_loaded = state<ChildLoaded>,
         state<ChildLoaded> + event<FinishPayloadChild> [child_can_finish] = X,
    }

    PayloadParent {
        *state<ParentIdle> + event<PreparePayload> / make_parent_ready = state<ParentReady>,
         state<ParentReady> + event<EnterPayloadChild> = state<PayloadChild>,
         state<PayloadChild> + completion<_> = state<ParentDone>,
         state<ParentDone> + on_entry<_> / entered_parent_done,
    }
}

#[derive(Default)]
struct PayloadCompositeContext {
    observations: Vec<u32>,
}

impl PayloadParentStateMachineContext for PayloadCompositeContext {
    fn make_parent_ready(
        &mut self,
        state: &ParentIdle,
        _: &PreparePayload,
    ) -> Result<ParentReady, ()> {
        Ok(ParentReady(state.0 + 10))
    }

    fn entered_child_idle(&mut self, state: &ChildIdle) -> Result<(), ()> {
        self.observations.push(state.0);
        Ok(())
    }

    fn make_child_loaded(
        &mut self,
        state: &ChildIdle,
        _: &FillPayloadChild,
    ) -> Result<ChildLoaded, ()> {
        Ok(ChildLoaded(state.0 + 20))
    }

    fn child_can_finish(&self, state: &ChildLoaded, _: &FinishPayloadChild) -> Result<bool, ()> {
        Ok(state.0 == 20)
    }

    fn entered_parent_done(&mut self, state: &ParentDone) -> Result<(), ()> {
        self.observations.push(state.0 + 1);
        Ok(())
    }
}

#[test]
fn composite_parent_and_child_store_and_inject_state_payloads() {
    let mut machine = PayloadParentStateMachine::new(PayloadCompositeContext::default());
    assert!(matches!(
        machine.state(),
        PayloadParentStates::ParentIdle(ParentIdle(0))
    ));
    machine.process_event(PreparePayload).unwrap();
    assert!(matches!(
        machine.state(),
        PayloadParentStates::ParentReady(ParentReady(10))
    ));
    machine.process_event(EnterPayloadChild).unwrap();
    machine.process_event(FillPayloadChild).unwrap();
    assert!(matches!(
        machine.child_state(),
        PayloadParentPayloadChildStates::ChildLoaded(ChildLoaded(20))
    ));
    machine.process_event(FinishPayloadChild).unwrap();
    assert!(matches!(
        machine.state(),
        PayloadParentStates::ParentDone(ParentDone(0))
    ));
    assert_eq!(machine.context().observations, [0, 1]);
}

pub struct AsyncCompositeBuild;
pub struct AsyncCompositeFinish;
pub struct AsyncCompositeData(u8);

sml! {
    AsyncPayloadChild {
        *"idle"_s + event<AsyncCompositeBuild> / async build_async_data = state<AsyncCompositeData>,
         state<AsyncCompositeData> + event<AsyncCompositeFinish> = X,
    }

    AsyncPayloadParent {
        *state<AsyncPayloadChild> + completion<_> = X,
    }
}

struct AsyncPayloadCompositeContext;

impl AsyncPayloadParentStateMachineContext for AsyncPayloadCompositeContext {
    async fn build_async_data(
        &mut self,
        _: &AsyncCompositeBuild,
    ) -> Result<AsyncCompositeData, ()> {
        smol::future::yield_now().await;
        Ok(AsyncCompositeData(9))
    }
}

#[test]
fn async_composite_actions_can_produce_child_state_payloads() {
    smol::block_on(async {
        let mut machine = AsyncPayloadParentStateMachine::new(AsyncPayloadCompositeContext);
        machine.process_event(AsyncCompositeBuild).await.unwrap();
        assert!(matches!(
            machine.child_state(),
            AsyncPayloadParentAsyncPayloadChildStates::AsyncCompositeData(AsyncCompositeData(9))
        ));
        machine.process_event(AsyncCompositeFinish).await.unwrap();
        assert!(machine.is_terminated());
    });
}

pub struct EnterTemporaryChild;
pub struct AdvanceTemporaryChild;

sml! {
    TemporaryChild {
        *"idle"_s + on_entry<_> / temporary_child_entry,
         "idle"_s + event<AdvanceTemporaryChild> / temporary_child_action = X,
    }

    TemporaryParent[temporary_context: &mut usize] {
        *"outside"_s + on_entry<_> / temporary_parent_entry,
         "outside"_s + event<EnterTemporaryChild> = state<TemporaryChild>,
         state<TemporaryChild> + completion<_> / temporary_parent_completion = X,
    }
}

struct TemporaryCompositeContext;

impl TemporaryParentStateMachineContext for TemporaryCompositeContext {
    fn temporary_parent_entry(&mut self, temporary: &mut usize) -> Result<(), ()> {
        *temporary += 1;
        Ok(())
    }

    fn temporary_child_entry(&mut self, temporary: &mut usize) -> Result<(), ()> {
        *temporary += 2;
        Ok(())
    }

    fn temporary_child_action(
        &mut self,
        temporary: &mut usize,
        _: &AdvanceTemporaryChild,
    ) -> Result<(), ()> {
        *temporary += 4;
        Ok(())
    }

    fn temporary_parent_completion(&mut self, temporary: &mut usize) -> Result<(), ()> {
        *temporary += 8;
        Ok(())
    }
}

#[test]
fn composite_callbacks_reborrow_mutable_temporary_context() {
    let mut machine = TemporaryParentStateMachine::new(TemporaryCompositeContext);
    let mut temporary = 0;
    machine.initialize(&mut temporary).unwrap();
    machine
        .process_event(&mut temporary, EnterTemporaryChild)
        .unwrap();
    machine
        .process_event(&mut temporary, AdvanceTemporaryChild)
        .unwrap();
    assert_eq!(temporary, 15);
    assert!(machine.is_terminated());
}

pub struct EnterEvalChild;
pub struct RunChildEval;

sml! {
    EvalChild {
        *"idle"_s + event<RunChildEval> / (child_before_eval, eval [async child_eval_enabled] / async child_conditional_eval) = X,
    }

    EvalParent {
        *"outside"_s + event<EnterEvalChild> = state<EvalChild>,
         state<EvalChild> + completion<_> / (parent_before_eval, eval [parent_eval_enabled] / parent_conditional_eval) = X,
    }
}

#[derive(Default)]
struct CompositeEvalContext {
    calls: Vec<&'static str>,
}

impl EvalParentStateMachineContext for CompositeEvalContext {
    fn child_before_eval(&mut self, _: &RunChildEval) -> Result<(), ()> {
        self.calls.push("child before");
        Ok(())
    }

    async fn child_eval_enabled(&self, _: &RunChildEval) -> Result<bool, ()> {
        Ok(true)
    }

    async fn child_conditional_eval(&mut self, _: &RunChildEval) -> Result<(), ()> {
        self.calls.push("child conditional");
        Ok(())
    }

    fn parent_before_eval(&mut self) -> Result<(), ()> {
        self.calls.push("parent before");
        Ok(())
    }

    fn parent_eval_enabled(&self) -> Result<bool, ()> {
        Ok(true)
    }

    fn parent_conditional_eval(&mut self) -> Result<(), ()> {
        self.calls.push("parent conditional");
        Ok(())
    }
}

#[test]
fn composite_eval_preserves_child_parent_order_and_async_callbacks() {
    smol::block_on(async {
        let mut machine = EvalParentStateMachine::new(CompositeEvalContext::default());
        machine.process_event(EnterEvalChild).await.unwrap();
        machine.process_event(RunChildEval).await.unwrap();
        assert_eq!(
            machine.context().calls,
            [
                "child before",
                "child conditional",
                "parent before",
                "parent conditional"
            ]
        );
        assert!(machine.is_terminated());
    });
}

pub struct CompositeQueueStart;
pub struct CompositeQueueAdvance;
#[derive(Clone)]
pub struct CompositeQueueDeferred;
pub struct CompositeQueueUnlock;

sml! {
    ProcessChild {
        *"idle"_s + event<CompositeQueueStart> / process(CompositeQueueAdvance {}) = "queued"_s,
         "queued"_s + event<CompositeQueueAdvance> = X,
    }

    ProcessParent {
        *state<ProcessChild> + completion<_> = X,
    }
}

sml! {
    DeferChild {
        *"locked"_s + event<CompositeQueueDeferred> / defer,
         "locked"_s + event<CompositeQueueUnlock> = "unlocked"_s,
         "unlocked"_s + event<CompositeQueueDeferred> = X,
    }

    DeferParent {
        *state<DeferChild> + completion<_> = X,
    }
}

struct CompositeQueueContext;
impl ProcessParentStateMachineContext for CompositeQueueContext {}
impl DeferParentStateMachineContext for CompositeQueueContext {}

#[test]
fn composite_process_dispatches_after_installing_the_child_target() {
    let mut machine = ProcessParentStateMachine::new(CompositeQueueContext);
    machine.process_event(CompositeQueueStart).unwrap();
    assert!(machine.is_terminated());
}

#[test]
fn composite_defer_retries_owned_child_events_after_state_change() {
    let mut machine = DeferParentStateMachine::new(CompositeQueueContext);
    machine.process_event(CompositeQueueDeferred).unwrap();
    assert!(machine.child_is_active());
    machine.process_event(CompositeQueueUnlock).unwrap();
    assert!(machine.is_terminated());
}

pub struct AsyncCompositeQueueStart;
pub struct AsyncCompositeQueueAdvance;
#[derive(Clone)]
pub struct AsyncCompositeQueueDeferred;
pub struct AsyncCompositeQueueUnlock;

sml! {
    AsyncProcessChild {
        *"idle"_s + event<AsyncCompositeQueueStart> / process(AsyncCompositeQueueAdvance {}) = "queued"_s,
         "queued"_s + event<AsyncCompositeQueueAdvance> = X,
    }

    AsyncProcessParent {
        *state<AsyncProcessChild> + completion<_> / async async_process_complete = X,
    }
}

sml! {
    AsyncDeferChild {
        *"locked"_s + event<AsyncCompositeQueueDeferred> / defer,
         "locked"_s + event<AsyncCompositeQueueUnlock> / async async_composite_unlock = "unlocked"_s,
         "unlocked"_s + event<AsyncCompositeQueueDeferred> = X,
    }

    AsyncDeferParent {
        *state<AsyncDeferChild> + completion<_> = X,
    }
}

struct AsyncCompositeQueueContext;

impl AsyncProcessParentStateMachineContext for AsyncCompositeQueueContext {
    async fn async_process_complete(&mut self) -> Result<(), ()> {
        smol::future::yield_now().await;
        Ok(())
    }
}

impl AsyncDeferParentStateMachineContext for AsyncCompositeQueueContext {
    async fn async_composite_unlock(&mut self, _: &AsyncCompositeQueueUnlock) -> Result<(), ()> {
        smol::future::yield_now().await;
        Ok(())
    }
}

#[test]
fn async_composite_process_uses_iterative_child_first_dispatch() {
    smol::block_on(async {
        let mut machine = AsyncProcessParentStateMachine::new(AsyncCompositeQueueContext);
        machine
            .process_event(AsyncCompositeQueueStart)
            .await
            .unwrap();
        assert!(machine.is_terminated());
    });
}

#[test]
fn async_composite_defer_retries_after_awaited_child_state_change() {
    smol::block_on(async {
        let mut machine = AsyncDeferParentStateMachine::new(AsyncCompositeQueueContext);
        machine
            .process_event(AsyncCompositeQueueDeferred)
            .await
            .unwrap();
        machine
            .process_event(AsyncCompositeQueueUnlock)
            .await
            .unwrap();
        assert!(machine.is_terminated());
    });
}

#[derive(Debug, PartialEq, Eq)]
pub struct CompositeExceptionFailure(u32);
pub struct FailInChild;

sml! {
    ExceptionChild {
        *"idle"_s + event<FailInChild> / async fail_in_child,
         "idle"_s + exception<CompositeExceptionFailure> / async recover_child_exception = X,
    }

    ExceptionParent {
        *state<ExceptionChild> + completion<_> = X,
    }
}

#[derive(Default)]
struct CompositeExceptionContext {
    code: Option<u32>,
}

impl ExceptionParentStateMachineContext for CompositeExceptionContext {
    async fn fail_in_child(&mut self, _: &FailInChild) -> Result<(), CompositeExceptionFailure> {
        Err(CompositeExceptionFailure(17))
    }

    async fn recover_child_exception(
        &mut self,
        error: &CompositeExceptionFailure,
    ) -> Result<(), CompositeExceptionFailure> {
        self.code = Some(error.0);
        Ok(())
    }
}

#[test]
fn composite_typed_exceptions_route_child_first_and_complete_parent() {
    smol::block_on(async {
        let mut machine = ExceptionParentStateMachine::new(CompositeExceptionContext::default());
        machine.process_event(FailInChild).await.unwrap();
        assert!(machine.is_terminated());
        assert_eq!(machine.context().code, Some(17));
    });
}

pub struct FailInParent;

sml! {
    ExceptionFallbackChild {
        *"idle"_s + event<AsyncCompositeQueueAdvance> = X,
    }

    ExceptionFallbackParent[custom_error] {
        *state<ExceptionFallbackChild> + event<FailInParent> / async fail_in_parent,
         state<ExceptionFallbackChild> + exception<_> / recover_parent_exception = X,
    }
}

#[derive(Default)]
struct ParentExceptionContext {
    recovered: bool,
}

impl ExceptionFallbackParentStateMachineContext for ParentExceptionContext {
    type Error = CompositeExceptionFailure;

    async fn fail_in_parent(&mut self, _: &FailInParent) -> Result<(), Self::Error> {
        Err(CompositeExceptionFailure(23))
    }

    fn recover_parent_exception(&mut self) -> Result<(), Self::Error> {
        self.recovered = true;
        Ok(())
    }
}

#[test]
fn composite_wildcard_exceptions_bubble_to_parent_when_child_has_no_handler() {
    smol::block_on(async {
        let mut machine =
            ExceptionFallbackParentStateMachine::new(ParentExceptionContext::default());
        machine.process_event(FailInParent).await.unwrap();
        assert!(machine.is_terminated());
        assert!(machine.context().recovered);
    });
}

pub struct EnterMultiA;
pub struct AdvanceMultiA;
pub struct EnterMultiB;
pub struct AdvanceMultiB;

sml! {
    MultiA {
        *"idle"_s + event<AdvanceMultiA> = X,
    }

    MultiB {
        *"idle"_s + event<AdvanceMultiB> = X,
    }

    MultipleChildren {
        *"outside"_s + event<EnterMultiA> = state<MultiA>,
         state<MultiA> + completion<_> = "between"_s,
         "between"_s + event<EnterMultiB> = state<MultiB>,
         state<MultiB> + completion<_> = X,
    }
}

struct MultipleChildrenContext;
impl MultipleChildrenStateMachineContext for MultipleChildrenContext {}

#[test]
fn multiple_direct_children_route_and_complete_independently() {
    let mut machine = MultipleChildrenStateMachine::new(MultipleChildrenContext);
    machine.process_event(EnterMultiA).unwrap();
    assert!(machine.is(&MultipleChildrenStates::MultiA));
    assert!(machine.is_multi_a(&MultipleChildrenMultiAStates::Idle));
    machine.process_event(AdvanceMultiA).unwrap();
    assert!(machine.is(&MultipleChildrenStates::Between));

    machine.process_event(EnterMultiB).unwrap();
    assert!(machine.is(&MultipleChildrenStates::MultiB));
    assert!(machine.is_multi_b(&MultipleChildrenMultiBStates::Idle));
    machine.process_event(AdvanceMultiB).unwrap();
    assert!(machine.is_terminated());
}

#[derive(Debug, PartialEq, Eq)]
pub struct MultiFailure(u8);
pub struct EnterFeatureA;
pub struct BuildFeatureA;
pub struct FinishFeatureA;
pub struct FailFeatureA;
pub struct EnterFeatureB;
#[derive(Clone)]
pub struct DeferredFeatureB;
pub struct UnlockFeatureB;
pub struct FeatureAData(u8);

sml! {
    FeatureA {
        *"idle"_s + event<BuildFeatureA> / async build_feature_a = state<FeatureAData>,
         "idle"_s + event<FailFeatureA> / async fail_feature_a,
         "idle"_s + exception<MultiFailure> / recover_feature_a = X,
         state<FeatureAData> + on_entry<_> / entered_feature_a_data,
         state<FeatureAData> + event<FinishFeatureA> = X,
    }

    FeatureB {
        *"locked"_s(H) + on_entry<_> / entered_feature_b,
         "locked"_s + event<DeferredFeatureB> / defer,
         "locked"_s + event<UnlockFeatureB> / (eval [feature_b_enabled] / feature_b_eval) = "unlocked"_s,
         "unlocked"_s + event<DeferredFeatureB> = X,
    }

    MultipleFeatures[temporary_context: &mut usize] {
        *"outside"_s + event<EnterFeatureA> = state<FeatureA>,
         state<FeatureA> + completion<_> = "between"_s,
         "between"_s + event<EnterFeatureB> = state<FeatureB>,
         state<FeatureB> + completion<_> = X,
    }
}

#[derive(Default)]
struct MultipleFeaturesContext {
    failure: Option<u8>,
}

impl MultipleFeaturesStateMachineContext for MultipleFeaturesContext {
    async fn build_feature_a(
        &mut self,
        temporary: &mut usize,
        _: &BuildFeatureA,
    ) -> Result<FeatureAData, MultiFailure> {
        *temporary += 1;
        Ok(FeatureAData(7))
    }

    async fn fail_feature_a(
        &mut self,
        _: &mut usize,
        _: &FailFeatureA,
    ) -> Result<(), MultiFailure> {
        Err(MultiFailure(9))
    }

    fn recover_feature_a(
        &mut self,
        temporary: &mut usize,
        error: &MultiFailure,
    ) -> Result<(), MultiFailure> {
        *temporary += 32;
        self.failure = Some(error.0);
        Ok(())
    }

    fn entered_feature_a_data(
        &mut self,
        temporary: &mut usize,
        state: &FeatureAData,
    ) -> Result<(), MultiFailure> {
        *temporary += usize::from(state.0 - 5);
        Ok(())
    }

    fn entered_feature_b(&mut self, temporary: &mut usize) -> Result<(), MultiFailure> {
        *temporary += 4;
        Ok(())
    }

    fn feature_b_enabled(
        &self,
        temporary: &mut usize,
        _: &UnlockFeatureB,
    ) -> Result<bool, MultiFailure> {
        *temporary += 8;
        Ok(true)
    }

    fn feature_b_eval(
        &mut self,
        temporary: &mut usize,
        _: &UnlockFeatureB,
    ) -> Result<(), MultiFailure> {
        *temporary += 16;
        Ok(())
    }
}

#[test]
fn multiple_children_compose_payload_async_context_eval_defer_and_lifecycle() {
    smol::block_on(async {
        let mut machine = MultipleFeaturesStateMachine::new(MultipleFeaturesContext::default());
        let mut temporary = 0;
        machine
            .process_event(&mut temporary, EnterFeatureA)
            .await
            .unwrap();
        machine
            .process_event(&mut temporary, BuildFeatureA)
            .await
            .unwrap();
        assert!(matches!(
            machine.feature_a_state(),
            MultipleFeaturesFeatureAStates::FeatureAData(FeatureAData(7))
        ));
        machine
            .process_event(&mut temporary, FinishFeatureA)
            .await
            .unwrap();
        machine
            .process_event(&mut temporary, EnterFeatureB)
            .await
            .unwrap();
        machine
            .process_event(&mut temporary, DeferredFeatureB)
            .await
            .unwrap();
        machine
            .process_event(&mut temporary, UnlockFeatureB)
            .await
            .unwrap();
        assert!(machine.is_terminated());
        assert_eq!(temporary, 31);
    });
}

#[test]
fn multiple_children_route_typed_exceptions_in_the_active_child() {
    smol::block_on(async {
        let mut machine = MultipleFeaturesStateMachine::new(MultipleFeaturesContext::default());
        let mut temporary = 0;
        machine
            .process_event(&mut temporary, EnterFeatureA)
            .await
            .unwrap();
        machine
            .process_event(&mut temporary, FailFeatureA)
            .await
            .unwrap();
        assert!(machine.is(&MultipleFeaturesStates::Between));
        assert_eq!(machine.context().failure, Some(9));
        assert_eq!(temporary, 32);
    });
}

pub struct DeepAdvance;

sml! {
    DeepLeaf {
        *"idle"_s + on_entry<_> / enter_deep_leaf,
         "idle"_s + event<DeepAdvance> = X,
    }

    DeepMiddle {
        *state<DeepLeaf> + on_entry<_> / enter_deep_middle,
         state<DeepLeaf> + completion<_> = X,
    }

    DeepRoot {
        *state<DeepMiddle> + on_entry<_> / enter_deep_root,
         state<DeepMiddle> + completion<_> = X,
    }
}

#[derive(Default)]
struct DeepContext {
    entries: Vec<&'static str>,
}

impl DeepRootStateMachineContext for DeepContext {
    fn enter_deep_root(&mut self) -> Result<(), ()> {
        self.entries.push("root");
        Ok(())
    }

    fn enter_deep_middle(&mut self) -> Result<(), ()> {
        self.entries.push("middle");
        Ok(())
    }

    fn enter_deep_leaf(&mut self) -> Result<(), ()> {
        self.entries.push("leaf");
        Ok(())
    }
}

#[test]
fn recursive_grandchildren_dispatch_deepest_first_and_complete_to_root() {
    let mut machine = DeepRootStateMachine::new(DeepContext::default());
    machine.initialize().unwrap();
    assert!(machine.deep_middle_is_active());
    assert!(machine.deep_leaf_is_active());
    assert_eq!(machine.context().entries, ["root", "middle", "leaf"]);

    machine.process_event(DeepAdvance).unwrap();
    assert!(machine.is_terminated());
    assert!(machine.is_deep_middle(&DeepRootDeepMiddleStates::X));
    assert!(machine.is_deep_leaf(&DeepRootDeepLeafStates::X));
}

pub struct EnterNestedOrthogonal;
pub struct FinishNestedOrthogonal;
pub struct NestedLeftData(u8);

sml! {
    NestedOrthogonal {
        *"left"_s + event<EnterNestedOrthogonal> / async make_nested_left = state<NestedLeftData>,
         state<NestedLeftData> + on_entry<_> / entered_nested_left,
         state<NestedLeftData> + event<FinishNestedOrthogonal> = X,

        *"right"_s + event<EnterNestedOrthogonal> = "right ready"_s,
         "right ready"_s + event<FinishNestedOrthogonal> = X,
    }

    OrthogonalCompositeRoot {
        *state<NestedOrthogonal> + completion<_> = X,
    }
}

#[derive(Default)]
struct NestedOrthogonalContext {
    entered: bool,
}

impl OrthogonalCompositeRootStateMachineContext for NestedOrthogonalContext {
    async fn make_nested_left(&mut self, _: &EnterNestedOrthogonal) -> Result<NestedLeftData, ()> {
        Ok(NestedLeftData(5))
    }

    fn entered_nested_left(&mut self, state: &NestedLeftData) -> Result<(), ()> {
        self.entered = state.0 == 5;
        Ok(())
    }
}

#[test]
fn composite_nodes_can_own_native_orthogonal_regions() {
    smol::block_on(async {
        let mut machine =
            OrthogonalCompositeRootStateMachine::new(NestedOrthogonalContext::default());
        machine.process_event(EnterNestedOrthogonal).await.unwrap();
        assert!(matches!(
            machine.nested_orthogonal_state(0),
            Some(OrthogonalCompositeRootNestedOrthogonalStates::NestedLeftData(NestedLeftData(5)))
        ));
        assert!(machine.context().entered);
        machine.process_event(FinishNestedOrthogonal).await.unwrap();
        assert!(machine.is_terminated());
    });
}

pub struct DeepOrthogonalAdvance;

sml! {
    DeepOrthogonalLeaf {
        *"left"_s + event<DeepOrthogonalAdvance> = X,
        *"right"_s + event<DeepOrthogonalAdvance> = X,
    }

    DeepOrthogonalMiddle {
        *state<DeepOrthogonalLeaf> + completion<_> = X,
    }

    DeepOrthogonalRoot {
        *state<DeepOrthogonalMiddle> + completion<_> = X,
    }
}

struct DeepOrthogonalContext;
impl DeepOrthogonalRootStateMachineContext for DeepOrthogonalContext {}

#[test]
fn orthogonal_region_groups_can_be_recursive_grandchildren() {
    let mut machine = DeepOrthogonalRootStateMachine::new(DeepOrthogonalContext);
    machine.initialize().unwrap();
    assert!(machine.deep_orthogonal_leaf_is_active());
    machine.process_event(DeepOrthogonalAdvance).unwrap();
    assert!(machine.is_terminated());
}
