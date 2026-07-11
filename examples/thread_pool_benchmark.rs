//! Fixed-capacity, allocation-free worker-pool fork/join benchmark.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

const WORKERS: usize = 8;
const ROUNDS: u64 = 5_000;

struct CountingAllocator;
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Default)]
struct Lane {
    generation: AtomicU64,
    completed: AtomicU64,
    stop: AtomicBool,
}

fn main() {
    let calls = Arc::new(AtomicU64::new(0));
    let lanes = (0..WORKERS)
        .map(|_| Arc::new(Lane::default()))
        .collect::<Vec<_>>();
    let mut workers = Vec::with_capacity(WORKERS);

    for lane in &lanes {
        let lane = Arc::clone(lane);
        let calls = Arc::clone(&calls);
        workers.push(thread::spawn(move || {
            let mut seen = 0;
            while !lane.stop.load(Ordering::Acquire) {
                let generation = lane.generation.load(Ordering::Acquire);
                if generation != seen {
                    calls.fetch_add(1, Ordering::Relaxed);
                    seen = generation;
                    lane.completed.store(generation, Ordering::Release);
                } else {
                    core::hint::spin_loop();
                }
            }
        }));
    }

    for lane in &lanes {
        lane.generation.store(1, Ordering::Release);
    }
    for lane in &lanes {
        while lane.completed.load(Ordering::Acquire) != 1 {
            core::hint::spin_loop();
        }
    }
    calls.store(0, Ordering::Release);

    let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
    let start = Instant::now();
    for round in 2..=ROUNDS + 1 {
        for lane in &lanes {
            lane.generation.store(round, Ordering::Release);
        }
        for lane in &lanes {
            while lane.completed.load(Ordering::Acquire) != round {
                core::hint::spin_loop();
            }
        }
    }
    let elapsed = start.elapsed().as_nanos();
    let dispatch_allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations_before;

    for lane in &lanes {
        lane.stop.store(true, Ordering::Release);
    }
    for worker in workers {
        worker.join().unwrap();
    }

    let expected = ROUNDS * WORKERS as u64;
    assert_eq!(calls.load(Ordering::Acquire), expected);
    println!(
        "rust-thread-pool {elapsed} ns total; {:.3} ns/task; {dispatch_allocations} allocations",
        elapsed as f64 / expected as f64
    );
}
