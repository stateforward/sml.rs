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
    fn observe_second<'a, 'b, T: Clone + 'a + 'b, const N: usize>(
        &mut self,
        event: &SecondOrigin<'b, T, N>,
    ) -> Result<(), ()> {
        let _ = (event.0, event.1.len());
        Ok(())
    }

    fn finish_origin<'a, 'b, T: Clone + 'a + 'b, const N: usize>(
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
