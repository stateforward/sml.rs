use sml::utility::{with_id, EventQueue, SmPool};

fn main() {
    let mut queue = EventQueue::<usize, 32>::new();
    for value in 0..32 {
        assert!(queue.defer(value).is_ok());
    }
    assert!(queue.defer(32).is_err());
    for expected in 0..32 {
        assert_eq!(queue.pop(), Some(expected));
    }

    let mut pool = SmPool::new([0_u64; 256]);
    for round in 0..10_000_u64 {
        let index = usize::try_from(round.checked_rem(256).unwrap_or_default()).unwrap_or(0);
        assert!(pool
            .process_indexed(index, round, |slot, event| *slot ^= event)
            .is_some());
    }
    let handled = pool.process_event_batch(
        [with_id(0, 1_u64), with_id(255, 2), with_id(256, 3)],
        |slot, event| *slot = slot.wrapping_add(event),
    );
    assert_eq!(handled, 2);
}
