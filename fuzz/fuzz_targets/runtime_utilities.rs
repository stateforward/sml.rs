#![no_main]

use libfuzzer_sys::fuzz_target;
use sml::utility::{with_id, EventQueue, SmPool};

fuzz_target!(|data: &[u8]| {
    let mut queue = EventQueue::<u8, 16>::new();
    let mut pool = SmPool::new([0_u8; 32]);

    for chunk in data.chunks(2) {
        let operation = chunk[0];
        let value = chunk.get(1).copied().unwrap_or_default();
        match operation % 6 {
            0 => {
                let _ = queue.defer(value);
            }
            1 => {
                let _ = queue.process(value);
            }
            2 => {
                let _ = queue.pop();
            }
            3 => queue.clear(),
            4 => {
                let _ = pool.process_indexed(
                    value as usize,
                    operation,
                    |slot, event| *slot ^= event,
                );
            }
            _ => {
                let _ = pool.process_event_batch(
                    [with_id(value as usize, operation)],
                    |slot, event| *slot = slot.wrapping_add(event),
                );
            }
        }
        assert!(queue.len() <= 16);
    }
});
