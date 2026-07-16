use core::fmt::Debug;
use sml::{sml, Machine};

pub struct Owned<T>(T);

sml! {
    GenericOwned<T>
    where
        T: Clone + Debug,
    {
        *"idle"_s + event<Owned<T>> / accept_owned,
    }
}

#[derive(Default)]
struct OwnedContext {
    accepted: usize,
}

impl GenericOwnedStateMachineContext for OwnedContext {
    fn accept_owned<T>(&mut self, event: &Owned<T>) -> Result<(), ()>
    where
        T: Clone + Debug,
    {
        let _copy = event.0.clone();
        self.accepted += 1;
        Ok(())
    }
}

#[test]
fn generic_owned_event_is_statically_dispatched_for_multiple_monomorphizations() {
    let mut machine = GenericOwnedStateMachine::new(OwnedContext::default());

    machine.process_event(Owned(7_u32)).unwrap();
    machine.process_event(Owned(String::from("typed"))).unwrap();

    assert_eq!(machine.context().accepted, 2);

    let event = GenericOwnedEvents::from(Owned(11_u16));
    assert!(<GenericOwnedStateMachine<_> as Machine<_>>::process_event(
        &mut machine,
        event,
    ));
    assert_eq!(machine.context().accepted, 3);
}

pub struct Borrowed<'a, T: ?Sized>(&'a T);

sml! {
    GenericBorrowed<'event, T>
    where
        T: ?Sized + Debug + 'event,
    {
        *"idle"_s + event<Borrowed<'event, T>> / inspect_borrow,
    }
}

#[derive(Default)]
struct BorrowedContext {
    inspected: usize,
}

impl GenericBorrowedStateMachineContext for BorrowedContext {
    fn inspect_borrow<'event, T>(&mut self, event: &Borrowed<'event, T>) -> Result<(), ()>
    where
        T: ?Sized + Debug + 'event,
    {
        let _ = format_args!("{:?}", event.0);
        self.inspected += 1;
        Ok(())
    }
}

#[test]
fn lifetime_parameterized_event_borrows_only_for_dispatch() {
    let mut machine = GenericBorrowedStateMachine::new(BorrowedContext::default());
    let first = String::from("first");
    let second = [1_u8, 2, 3];

    machine.process_event(Borrowed(&first)).unwrap();
    machine.process_event(Borrowed(&second[..])).unwrap();

    assert_eq!(machine.context().inspected, 2);
    assert_eq!(first, "first");
}

pub struct StaticBorrow<'a>(&'a str);

sml! {
    ConcreteStatic {
        *"idle"_s + event<StaticBorrow<'static>> / inspect_static,
    }
}

#[derive(Default)]
struct StaticContext {
    inspected: usize,
}

impl ConcreteStaticStateMachineContext for StaticContext {
    fn inspect_static(&mut self, event: &StaticBorrow<'static>) -> Result<(), ()> {
        assert_eq!(event.0, "static");
        self.inspected += 1;
        Ok(())
    }
}

#[test]
fn concrete_static_lifetime_is_not_promoted_to_a_generic_parameter() {
    let mut machine = ConcreteStaticStateMachine::new(StaticContext::default());
    machine.process_event(StaticBorrow("static")).unwrap();
    assert_eq!(machine.context().inspected, 1);
}

pub struct Operation<T> {
    input: T,
    result: Option<T>,
}

sml! {
    GenericOperation<'operation, T>
    where
        T: Clone + 'operation,
    {
        *"idle"_s + event<&'operation mut Operation<T>> [operation_ready]
            / (complete_operation, observe_completed_operation),
    }
}

#[derive(Default)]
struct OperationContext {
    completed: usize,
}

impl GenericOperationStateMachineContext for OperationContext {
    fn operation_ready<'operation, T>(
        &self,
        operation: &'operation mut Operation<T>,
    ) -> Result<bool, ()>
    where
        T: Clone + 'operation,
    {
        Ok(operation.result.is_none())
    }

    fn complete_operation<'operation, T>(
        &mut self,
        operation: &'operation mut Operation<T>,
    ) -> Result<(), ()>
    where
        T: Clone + 'operation,
    {
        operation.result = Some(operation.input.clone());
        Ok(())
    }

    fn observe_completed_operation<'operation, T>(
        &mut self,
        operation: &'operation mut Operation<T>,
    ) -> Result<(), ()>
    where
        T: Clone + 'operation,
    {
        assert!(operation.result.is_some());
        self.completed += 1;
        Ok(())
    }
}

#[test]
fn mutable_borrowed_typed_operation_completes_synchronously() {
    let mut machine = GenericOperationStateMachine::new(OperationContext::default());
    let mut operation = Operation {
        input: String::from("answer"),
        result: None,
    };

    machine.process_event(&mut operation).unwrap();

    assert_eq!(operation.result.as_deref(), Some("answer"));
    assert_eq!(machine.context().completed, 1);
}

pub struct ConstPacket<T, const N: usize>([T; N]);

sml! {
    GenericConst<T: Copy, const N: usize>
    where
        [T; N]: AsRef<[T]>,
    {
        *"idle"_s + event<ConstPacket<T, N>> / accept_packet,
    }
}

struct ConstContext;

impl GenericConstStateMachineContext for ConstContext {
    fn accept_packet<T: Copy, const N: usize>(
        &mut self,
        packet: &ConstPacket<T, N>,
    ) -> Result<(), ()>
    where
        [T; N]: AsRef<[T]>,
    {
        assert_eq!(packet.0.as_ref().len(), N);
        Ok(())
    }
}

#[test]
fn const_generic_event_propagates_bounds_and_where_clause() {
    let mut machine = GenericConstStateMachine::new(ConstContext);
    machine.process_event(ConstPacket([1_u8, 2, 3, 4])).unwrap();
}

#[derive(Clone)]
pub struct CompletionEvent<T>(T);

sml! {
    GenericCompletion<T: Clone> {
        *"idle"_s + event<CompletionEvent<T>> = "finishing"_s,
         "finishing"_s + completion<CompletionEvent> / finish = X,
    }
}

#[derive(Default)]
struct CompletionContext {
    finished: usize,
}

impl GenericCompletionStateMachineContext for CompletionContext {
    fn finish<T: Clone>(&mut self, _event: &CompletionEvent<T>) -> Result<(), ()> {
        self.finished += 1;
        Ok(())
    }
}

#[test]
fn generic_owned_event_propagates_into_origin_aware_completion() {
    let mut machine = GenericCompletionStateMachine::new(CompletionContext::default());
    machine.process_event(CompletionEvent(42_u32)).unwrap();

    assert!(machine.is_terminated());
    assert_eq!(machine.context().finished, 1);
}

#[derive(Clone)]
pub struct FirstOrigin<'a, T, const N: usize>(&'a T, [u8; N]);

#[derive(Clone)]
pub struct SecondOrigin<'b, T, const N: usize>(&'b T, [u8; N]);

sml! {
    EventSpecificCompletion<'a, 'b, T, const N: usize>
    where
        T: Clone + 'a + 'b,
    {
        *Idle + event<FirstOrigin<'a, T, N>> = Completing,
         Idle + event<SecondOrigin<'b, T, N>> / observe_second,
         Completing + completion<FirstOrigin> / finish_origin = X,
    }
}

#[derive(Default)]
struct EventSpecificCompletionContext {
    finished: usize,
}

impl EventSpecificCompletionStateMachineContext for EventSpecificCompletionContext {
    fn observe_second<'b, T: Clone + 'b, const N: usize>(
        &mut self,
        event: &SecondOrigin<'b, T, N>,
    ) -> Result<(), ()> {
        let _ = (event.0, event.1.len());
        Ok(())
    }

    fn finish_origin<'a, T: Clone + 'a, const N: usize>(
        &mut self,
        event: &FirstOrigin<'a, T, N>,
    ) -> Result<(), ()> {
        let _ = (event.0, event.1.len());
        self.finished += 1;
        Ok(())
    }
}

#[test]
fn completion_origin_keeps_only_the_lifetimes_used_by_its_variants() {
    let value = String::from("origin");
    let mut machine =
        EventSpecificCompletionStateMachine::new(EventSpecificCompletionContext::default());
    machine
        .process_event(SecondOrigin(&value, [1_u8; 2]))
        .unwrap();
    machine
        .process_event(FirstOrigin(&value, [0_u8; 4]))
        .unwrap();

    assert!(machine.is_terminated());
    assert_eq!(machine.context().finished, 1);
}

pub struct TemporaryContextEvent<T>(T);

sml! {
    GenericTemporaryContext<T: Clone>[temporary_context: &mut Vec<T>] {
        *Idle + event<TemporaryContextEvent<T>> / push_value,
         Idle + Reset / clear_values,
    }
}

struct TemporaryContextCallbacks;

impl GenericTemporaryContextStateMachineContext for TemporaryContextCallbacks {
    fn push_value<T>(
        &mut self,
        values: &mut Vec<T>,
        event: &TemporaryContextEvent<T>,
    ) -> Result<(), ()>
    where
        T: Clone,
    {
        values.push(event.0.clone());
        Ok(())
    }

    fn clear_values<T>(&mut self, values: &mut Vec<T>) -> Result<(), ()> {
        values.clear();
        Ok(())
    }
}

#[test]
fn temporary_context_generics_propagate_to_non_generic_callbacks() {
    let mut machine = GenericTemporaryContextStateMachine::new(TemporaryContextCallbacks);
    let mut values = Vec::new();

    machine.initialize().unwrap();

    machine
        .process_event(&mut values, TemporaryContextEvent(String::from("value")))
        .unwrap();
    assert_eq!(values, ["value"]);

    machine
        .process_event(&mut values, GenericTemporaryContextEvents::<String>::Reset)
        .unwrap();
    assert!(values.is_empty());
}

pub struct TemporaryEntryValues<T>(Vec<T>);

sml! {
    GenericTemporaryEntry<T: Default>[temporary_context: &mut TemporaryEntryValues<T>] {
        *Idle + on_entry<_> / seed_entry,
         Idle + event<TemporaryContextEvent<T>> = X,
    }
}

struct TemporaryEntryContext;

impl GenericTemporaryEntryStateMachineContext for TemporaryEntryContext {
    fn seed_entry<T: Default>(&mut self, values: &mut TemporaryEntryValues<T>) -> Result<(), ()> {
        values.0.push(T::default());
        Ok(())
    }
}

#[test]
fn initialize_retains_generic_temporary_context_for_initial_entry_actions() {
    let mut machine = GenericTemporaryEntryStateMachine::new(TemporaryEntryContext);
    let mut values = TemporaryEntryValues(Vec::<String>::new());

    machine.initialize(&mut values).unwrap();
    assert_eq!(values.0, [String::new()]);
}

pub struct LaterEntryEvent<T>(T);

sml! {
    GenericLaterEntry<T>[temporary_context: &mut TemporaryEntryValues<T>] {
        *Idle + event<LaterEntryEvent<T>> = Ready,
         Ready + on_entry<_> / later_entry,
    }
}

struct LaterEntryContext;

impl GenericLaterEntryStateMachineContext for LaterEntryContext {
    fn later_entry<T>(&mut self, values: &mut TemporaryEntryValues<T>) -> Result<(), ()> {
        values.0.clear();
        Ok(())
    }
}

#[test]
fn initialize_runs_later_state_entry_actions_with_generic_context() {
    let mut machine = GenericLaterEntryStateMachine::new(LaterEntryContext);
    let mut values = TemporaryEntryValues(vec![String::from("value")]);

    machine.set_state(GenericLaterEntryStates::Ready);
    machine.initialize(&mut values).unwrap();
    assert!(values.0.is_empty());
}

pub struct TemporaryLifecycleEvent<T>(T);

sml! {
    GenericTemporaryLifecycle<T>[temporary_context: &mut Vec<T>] {
        *Boot + completion<_> / prepare_values = Idle,
         Idle + event<TemporaryLifecycleEvent<T>> / fail_event = X,
         Idle + exception<_> / recover_values = X,
    }
}

struct TemporaryLifecycleContext;

impl GenericTemporaryLifecycleStateMachineContext for TemporaryLifecycleContext {
    fn prepare_values<T>(&mut self, values: &mut Vec<T>) -> Result<(), ()> {
        values.clear();
        Ok(())
    }

    fn fail_event<T>(
        &mut self,
        values: &mut Vec<T>,
        event: &TemporaryLifecycleEvent<T>,
    ) -> Result<(), ()> {
        let _ = (values, &event.0);
        Err(())
    }

    fn recover_values<T>(&mut self, values: &mut Vec<T>) -> Result<(), ()> {
        values.clear();
        Ok(())
    }
}

#[test]
fn temporary_context_generics_propagate_through_initialize_and_exception_paths() {
    let mut machine = GenericTemporaryLifecycleStateMachine::new(TemporaryLifecycleContext);
    let mut values = vec![1_u32];

    machine.initialize(&mut values).unwrap();
    assert!(values.is_empty());
    assert!(machine.is(&GenericTemporaryLifecycleStates::Idle));

    machine
        .process_event(&mut values, TemporaryLifecycleEvent(7_u32))
        .unwrap();
    assert!(machine.is_terminated());
}

#[derive(Clone)]
pub struct CompletionAfterInitialization<T>(T);

sml! {
    GenericAnonymousCompletion<T: Clone> {
        *Idle + completion<_> = Ready,
         Ready + event<CompletionAfterInitialization<T>> = Finishing,
         Finishing + completion<CompletionAfterInitialization> = X,
    }
}

struct AnonymousCompletionContext;
impl GenericAnonymousCompletionStateMachineContext for AnonymousCompletionContext {}

#[test]
fn anonymous_initialization_does_not_require_event_family_inference() {
    let mut machine = GenericAnonymousCompletionStateMachine::new(AnonymousCompletionContext);

    machine.initialize().unwrap();
    assert!(machine.is(&GenericAnonymousCompletionStates::Ready));

    machine
        .process_event(CompletionAfterInitialization(7_u32))
        .unwrap();
    assert!(machine.is_terminated());
}

pub struct NestedLifetimeEvent<A, T>(A, T);

sml! {
    GenericNestedLifetime<T> {
        *Idle + event<NestedLifetimeEvent<Option<&'event u8>, T>> / inspect_nested,
    }
}

struct NestedLifetimeContext;

impl GenericNestedLifetimeStateMachineContext for NestedLifetimeContext {
    #[allow(clippy::needless_lifetimes)]
    fn inspect_nested<'event, T>(
        &mut self,
        event: &NestedLifetimeEvent<Option<&'event u8>, T>,
    ) -> Result<(), ()> {
        let _ = (&event.0, &event.1);
        Ok(())
    }
}

#[test]
fn event_specific_lifetimes_are_collected_from_nested_type_arguments() {
    let value = 9_u8;
    let mut machine = GenericNestedLifetimeStateMachine::new(NestedLifetimeContext);

    machine
        .process_event(NestedLifetimeEvent(Some(&value), String::from("nested")))
        .unwrap();
}

pub struct HigherRankedEvent<F, T>(F, std::marker::PhantomData<T>);

sml! {
    GenericHigherRanked<T> {
        *Idle + event<HigherRankedEvent<fn(&T), T>> / inspect_higher_ranked = X,
    }
}

struct HigherRankedContext;

impl GenericHigherRankedStateMachineContext for HigherRankedContext {
    fn inspect_higher_ranked<T>(&mut self, event: &HigherRankedEvent<fn(&T), T>) -> Result<(), ()> {
        let _ = (&event.0, &event.1);
        Ok(())
    }
}

#[test]
fn higher_ranked_lifetimes_remain_bound_inside_the_event_type() {
    fn observe(_: &u32) {}

    let event: HigherRankedEvent<fn(&u32), u32> =
        HigherRankedEvent(observe, std::marker::PhantomData);
    let mut machine = GenericHigherRankedStateMachine::new(HigherRankedContext);

    machine.process_event(event).unwrap();
    assert!(machine.is_terminated());
}

trait CompletionLifetimeMarker {}
impl<T: ?Sized> CompletionLifetimeMarker for &T {}

pub struct FirstLifetimeEvent<'first, T>(&'first T);

impl<T> Copy for FirstLifetimeEvent<'_, T> {}

impl<T> Clone for FirstLifetimeEvent<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

pub struct SecondLifetimeEvent<'second, T>(&'second T);

sml! {
    GenericCompletionLifetimeBound<'first, 'second, T: 'first + 'second>
    where
        &'second T: CompletionLifetimeMarker,
    {
        *Idle + event<FirstLifetimeEvent<'first, T>> = Waiting,
         Waiting + completion<FirstLifetimeEvent> / finish_first = X,
         Waiting + event<SecondLifetimeEvent<'second, T>> / observe_second_lifetime = X,
    }
}

struct CompletionLifetimeBoundContext;

impl GenericCompletionLifetimeBoundStateMachineContext for CompletionLifetimeBoundContext {
    fn finish_first<'first, T: 'first>(
        &mut self,
        event: &FirstLifetimeEvent<'first, T>,
    ) -> Result<(), ()> {
        let _ = event.0;
        Ok(())
    }

    fn observe_second_lifetime<'second, T: 'second>(
        &mut self,
        event: &SecondLifetimeEvent<'second, T>,
    ) -> Result<(), ()>
    where
        &'second T: CompletionLifetimeMarker,
    {
        let _ = event.0;
        Ok(())
    }
}

#[test]
fn completion_callbacks_drop_bounds_for_omitted_event_specific_lifetimes() {
    let value = 11_u32;
    let mut machine =
        GenericCompletionLifetimeBoundStateMachine::new(CompletionLifetimeBoundContext);

    machine.process_event(FirstLifetimeEvent(&value)).unwrap();
    assert!(machine.is_terminated());
}

pub struct InlineFirst<'short, T>(&'short T);
pub struct InlineSecond<'long, T>(&'long T);

sml! {
    GenericInlineLifetimeBound<'short: 'long, 'long, T: 'short + 'long> {
        *Idle + event<InlineFirst<'short, T>> / inspect_inline_first,
         Idle + event<InlineSecond<'long, T>> / inspect_inline_second,
    }
}

struct InlineLifetimeBoundContext;

impl GenericInlineLifetimeBoundStateMachineContext for InlineLifetimeBoundContext {
    fn inspect_inline_first<'short, T: 'short>(
        &mut self,
        event: &InlineFirst<'short, T>,
    ) -> Result<(), ()> {
        let _ = event.0;
        Ok(())
    }

    fn inspect_inline_second<'long, T: 'long>(
        &mut self,
        event: &InlineSecond<'long, T>,
    ) -> Result<(), ()> {
        let _ = event.0;
        Ok(())
    }
}

#[test]
fn callbacks_drop_inline_bounds_for_omitted_event_lifetimes() {
    let first = 1_u32;
    let second = 2_u32;
    let mut machine = GenericInlineLifetimeBoundStateMachine::new(InlineLifetimeBoundContext);

    machine.process_event(InlineFirst(&first)).unwrap();
    machine.process_event(InlineSecond(&second)).unwrap();
}
