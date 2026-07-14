use sml::utility::{
    with_id, DispatchStatus, DispatchTable, EventQueue, EventQueues, Hierarchical,
    HierarchicalDispatch, OrthogonalRegions, QueueFull, SmPool,
};
use sml::{Machine as MachineTrait, Terminated};
use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

struct TestWake;

impl Wake for TestWake {
    fn wake(self: Arc<Self>) {}
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(TestWake));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => {}
        }
    }
}

#[derive(Default)]
struct Machine {
    value: i32,
}

#[derive(Clone)]
enum Event {
    Add(i32),
    Clear,
}

fn dispatch(machine: &mut Machine, event: Event) {
    match event {
        Event::Add(value) => machine.value += value,
        Event::Clear => machine.value = 0,
    }
}

#[test]
fn pool_supports_indexed_and_batch_dispatch() {
    let mut pool = SmPool::new([Machine::default(), Machine::default(), Machine::default()]);

    pool.process_indexed(1, Event::Add(2), dispatch).unwrap();
    assert!(pool.process_indexed(9, Event::Clear, dispatch).is_none());

    let handled = pool.process_indexed_batch([0, 2, 9], Event::Add(3), dispatch);
    assert_eq!(handled, 2);

    let handled = pool.process_event_batch(
        [
            with_id(0, Event::Add(4)),
            with_id(1, Event::Clear),
            with_id(9, Event::Clear),
        ],
        dispatch,
    );
    assert_eq!(handled, 2);
    assert_eq!(pool.storage()[0].value, 7);
    assert_eq!(pool.storage()[1].value, 0);
    assert_eq!(pool.storage()[2].value, 3);

    pool.reset(|index| Machine {
        value: index as i32,
    });
    assert_eq!(pool.storage()[2].value, 2);
    pool.storage_mut()[0].value = 9;
    assert_eq!(pool.storage()[0].value, 9);
}

#[derive(Clone, Copy)]
struct RawEvent {
    value: i32,
}

fn add(machine: &mut Machine, raw: &RawEvent) -> i32 {
    machine.value += raw.value;
    machine.value
}

fn subtract(machine: &mut Machine, raw: &RawEvent) -> i32 {
    machine.value -= raw.value;
    machine.value
}

#[test]
fn dispatch_table_routes_contiguous_runtime_ids() {
    let handlers: [fn(&mut Machine, &RawEvent) -> i32; 2] = [add, subtract];
    let mut machine = Machine::default();
    let mut table = DispatchTable::new(&mut machine, 10, &handlers);

    assert_eq!(table.dispatch(&RawEvent { value: 5 }, 10), Some(5));
    assert_eq!(table.dispatch(&RawEvent { value: 2 }, 11), Some(3));
    assert_eq!(table.dispatch(&RawEvent { value: 1 }, 9), None);
    assert_eq!(table.dispatch(&RawEvent { value: 1 }, 12), None);
    assert_eq!(table.machine().value, 3);
    table.machine_mut().value = 11;
    assert_eq!(table.machine().value, 11);
}

impl MachineTrait<Event> for Machine {
    type State = i32;

    fn process_event(&mut self, event: Event) -> bool {
        dispatch(self, event);
        true
    }
}

#[test]
fn machine_trait_async_entry_point_awaits_inline_rtc() {
    block_on(async {
        let mut machine = Machine { value: 3 };
        assert!(MachineTrait::process_event_async(&mut machine, Event::Add(4)).await);
        assert_eq!(machine.value, 7);
    });
}

#[derive(Default)]
struct PendingMachine {
    value: i32,
    polls: usize,
}

impl MachineTrait<Event> for PendingMachine {
    type State = i32;

    fn process_event(&mut self, event: Event) -> bool {
        match event {
            Event::Add(value) => self.value += value,
            Event::Clear => self.value = 0,
        }
        true
    }

    fn process_event_async(&mut self, event: Event) -> impl core::future::Future<Output = bool> {
        let mut event = Some(event);
        core::future::poll_fn(move |cx| {
            self.polls += 1;
            if self.polls == 1 {
                cx.waker().wake_by_ref();
                core::task::Poll::Pending
            } else {
                core::task::Poll::Ready(
                    self.process_event(
                        event
                            .take()
                            .expect("pending future completed more than once"),
                    ),
                )
            }
        })
    }
}

async fn process_generic_async<M, E>(machine: &mut M, event: E) -> bool
where
    M: MachineTrait<E>,
{
    MachineTrait::process_event_async(machine, event).await
}

#[test]
fn machine_trait_override_can_borrow_and_remain_pending_without_boxing() {
    block_on(async {
        let mut machine = PendingMachine::default();
        assert!(process_generic_async(&mut machine, Event::Add(9)).await);
        assert_eq!(machine.polls, 2);
        assert_eq!(machine.value, 9);
    });
}

#[test]
fn orthogonal_regions_broadcast_to_every_active_machine() {
    let mut regions = OrthogonalRegions::new([
        Machine { value: 1 },
        Machine { value: 10 },
        Machine { value: 100 },
    ]);

    assert_eq!(regions.process_event(Event::Add(5)), 3);
    assert_eq!(regions.regions()[0].value, 6);
    assert_eq!(regions.regions()[1].value, 15);
    assert_eq!(regions.regions()[2].value, 105);
    regions.regions_mut()[0].value = 7;
    assert_eq!(regions.regions()[0].value, 7);
}

#[test]
fn hierarchy_accessors_reset_and_all_completion_paths_work() {
    let mut hierarchy = Hierarchical::new(Machine { value: 1 }, Child::default());
    assert!(!hierarchy.child_is_active());
    assert_eq!(hierarchy.parent().value, 1);
    hierarchy.parent_mut().value = 2;
    hierarchy.child_mut().handled = 3;
    hierarchy.activate_child();
    hierarchy.deactivate_child();
    hierarchy.reset_child(Child {
        handled: 4,
        terminal: false,
    });
    assert_eq!(hierarchy.child().handled, 4);

    let child = |_child: &mut Child, event: Event| matches!(event, Event::Add(_));
    let parent = |_parent: &mut Machine, event: Event| matches!(event, Event::Clear);
    assert_eq!(
        hierarchy.process_event(Event::Add(1), child, parent),
        HierarchicalDispatch::Child
    );
    assert_eq!(
        hierarchy.process_event(Event::Add(1), |_child, _event| false, parent),
        HierarchicalDispatch::Unhandled
    );

    hierarchy.child_mut().terminal = true;
    assert_eq!(
        hierarchy.process_event_with_completion(Event::Add(1), child, parent, |_parent| false),
        HierarchicalDispatch::ChildTerminated
    );
    hierarchy.child_mut().terminal = false;
    assert_eq!(
        hierarchy.process_event_with_completion(Event::Add(1), child, parent, |_parent| true),
        HierarchicalDispatch::Child
    );
    assert_eq!(
        hierarchy.process_event_with_completion(
            Event::Clear,
            |_child, _event| false,
            parent,
            |_parent| true,
        ),
        HierarchicalDispatch::Parent
    );
    assert_eq!(
        hierarchy.process_event_with_completion(
            Event::Add(1),
            |_child, _event| false,
            parent,
            |_parent| true,
        ),
        HierarchicalDispatch::Unhandled
    );
}

#[test]
fn bounded_queue_orders_processed_events_before_deferred_events() {
    let mut queue = EventQueue::<i32, 3>::new();
    queue.defer(1).unwrap();
    queue.defer(2).unwrap();
    queue.process(3).unwrap();

    assert_eq!(queue.len(), 3);
    assert_eq!(queue.defer(4), Err(QueueFull));
    assert_eq!(queue.pop(), Some(3));
    assert_eq!(queue.pop(), Some(1));
    assert_eq!(queue.pop(), Some(2));
    assert_eq!(queue.pop(), None);
}

#[test]
fn queue_defaults_clear_and_zero_capacity_are_total() {
    let mut queue = EventQueue::<i32, 2>::default();
    assert!(queue.is_empty());
    queue.defer(1).unwrap();
    queue.process(2).unwrap();
    assert_eq!(queue.process(3), Err(QueueFull));
    queue.clear();
    assert!(queue.is_empty());

    let mut zero = EventQueue::<i32, 0>::new();
    assert_eq!(zero.defer(1), Err(QueueFull));
    assert_eq!(zero.process(1), Err(QueueFull));
    assert_eq!(zero.pop(), None);
}

#[test]
fn event_queue_defaults_report_both_lengths() {
    let mut queues = EventQueues::<i32, 1, 1>::default();
    assert_eq!(queues.deferred_len(), 0);
    assert_eq!(queues.processed_len(), 0);
    queues.defer(1).unwrap();
    queues.process(2).unwrap();
    assert_eq!(queues.deferred_len(), 1);
    assert_eq!(queues.processed_len(), 1);
}

#[test]
fn recursive_dispatch_can_consume_an_outer_deferred_retry() {
    let mut queues = EventQueues::<i32, 2, 1>::new();
    queues.defer(1).unwrap();
    let mut machine = 0;
    let summary = queues.dispatch(&mut machine, 0, |machine, queues, event| {
        *machine += 1;
        if event == 0 {
            let nested = queues.dispatch(machine, 2, |_machine, _queues, _event| DispatchStatus {
                handled: true,
                transitioned: true,
            });
            assert_eq!(nested.dispatched, 2);
        }
        DispatchStatus {
            handled: true,
            transitioned: true,
        }
    });
    assert_eq!(summary.dispatched, 1);
    assert_eq!(machine, 1);
}

#[derive(Default)]
struct Child {
    handled: usize,
    terminal: bool,
}

impl Terminated for Child {
    fn is_terminated(&self) -> bool {
        self.terminal
    }
}

#[test]
fn hierarchy_bubbles_events_and_retains_shallow_history() {
    let mut hierarchy = Hierarchical::new_active(0usize, Child::default());

    let handled = hierarchy.process_event(
        "child",
        |child, event| {
            if event == "child" {
                child.handled += 1;
                true
            } else {
                false
            }
        },
        |parent, _| {
            *parent += 1;
            true
        },
    );
    assert_eq!(handled, HierarchicalDispatch::Child);

    let bubbled = hierarchy.process_event(
        "parent",
        |_, _| false,
        |parent, _| {
            *parent += 1;
            true
        },
    );
    assert_eq!(bubbled, HierarchicalDispatch::Parent);

    hierarchy.deactivate_child();
    hierarchy.activate_child();
    assert_eq!(hierarchy.child().handled, 1);

    hierarchy.child_mut().terminal = true;
    let terminal = hierarchy.process_event("done", |_, _| true, |_, _| false);
    assert_eq!(terminal, HierarchicalDispatch::ChildTerminated);
}

#[test]
fn hierarchy_propagates_child_terminal_completion_to_parent() {
    let child = Child {
        handled: 0,
        terminal: false,
    };
    let mut hierarchy = Hierarchical::new_active(0usize, child);

    let result = hierarchy.process_event_with_completion(
        "finish",
        |child, _| {
            child.terminal = true;
            true
        },
        |_, _| false,
        |parent| {
            *parent += 1;
            true
        },
    );

    assert_eq!(result, HierarchicalDispatch::Parent);
    assert_eq!(*hierarchy.parent(), 1);
    assert!(!hierarchy.child_is_active());

    hierarchy.activate_child();
    assert!(hierarchy.child().terminal);
}

#[derive(Clone, Copy)]
enum QueuedEvent {
    Hold,
    Advance,
    FollowUp,
}

#[test]
fn queued_dispatch_retries_deferred_events_after_transition() {
    let mut state = 0u8;
    let mut queues = EventQueues::<QueuedEvent, 4, 4>::new();

    let dispatch = |state: &mut u8,
                    queues: &mut EventQueues<QueuedEvent, 4, 4>,
                    event: QueuedEvent| match (*state, event) {
        (0, QueuedEvent::Hold) => {
            queues.defer(QueuedEvent::Hold).unwrap();
            DispatchStatus {
                handled: true,
                transitioned: false,
            }
        }
        (0, QueuedEvent::Advance) => {
            *state = 1;
            queues.process(QueuedEvent::FollowUp).unwrap();
            DispatchStatus {
                handled: true,
                transitioned: true,
            }
        }
        (1, QueuedEvent::FollowUp) => {
            *state = 2;
            DispatchStatus {
                handled: true,
                transitioned: true,
            }
        }
        (2, QueuedEvent::Hold) => {
            *state = 3;
            DispatchStatus {
                handled: true,
                transitioned: true,
            }
        }
        _ => DispatchStatus::default(),
    };

    let first = queues.dispatch(&mut state, QueuedEvent::Hold, dispatch);
    assert_eq!(first.dispatched, 1);
    assert_eq!(queues.deferred_len(), 1);

    let second = queues.dispatch(&mut state, QueuedEvent::Advance, dispatch);
    assert_eq!(second.dispatched, 3);
    assert_eq!(second.handled, 3);
    assert_eq!(state, 3);
    assert_eq!(queues.deferred_len(), 0);
}
