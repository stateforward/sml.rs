#![no_main]

use libfuzzer_sys::fuzz_target;
use sml::utility::{with_id, EventQueue, SmPool};

fuzz_target!(|data: &[u8]| {
    let mut queue = EventQueue::<u8, 16>::new();
    let mut pool = SmPool::new([0_u8; 32]);

    for chunk in data.chunks(2) {
        let Some(&operation) = chunk.first() else {
            continue;
        };
        let value = chunk.get(1).copied().unwrap_or_default();
        let operation_kind = operation.checked_rem(6).unwrap_or_default();
        match operation_kind {
            0 => {
                let _deferred = queue.defer(value).is_ok();
            }
            1 => {
                let _processed = queue.process(value).is_ok();
            }
            2 => {
                let _popped = queue.pop().is_some();
            }
            3 => queue.clear(),
            4 => {
                let _dispatched = pool
                    .process_indexed(
                    usize::from(value),
                    operation,
                    |slot, event| *slot ^= event,
                )
                .is_some();
            }
            _ => {
                let _dispatched = pool
                    .process_event_batch(
                        [with_id(usize::from(value), operation)],
                        |slot, event| *slot = slot.wrapping_add(event),
                    )
                    > 0;
            }
        }
        assert!(queue.len() <= 16);
    }
});
