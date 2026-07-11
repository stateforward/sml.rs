//! Tensor-actor pool benchmark matched to sml.cpp's cache-locality workload.

use std::alloc::{GlobalAlloc, Layout, System};
use std::env;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use sml::utility::SmPool;

const ACTORS: usize = 10_000;
const DISPATCHES: usize = 50_000;
const ROUNDS: usize = 1_001;

struct CountingAllocator;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

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
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Clone, Copy)]
struct Pulse;

fn ids(random: bool) -> Vec<usize> {
    if !random {
        return (0..DISPATCHES).map(|index| index % ACTORS).collect();
    }

    let mut state = 1_337_u32;
    (0..DISPATCHES)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as usize % ACTORS
        })
        .collect()
}

#[inline(always)]
fn toggle(flag: &mut u8, _: Pulse) {
    *flag ^= 1;
}

fn measure_direct(indices: &[usize], label: &str) {
    let mut flags = vec![0_u8; ACTORS];
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let started = Instant::now();
    for _ in 0..ROUNDS {
        for &index in indices {
            flags[index] ^= 1;
        }
    }
    let elapsed = started.elapsed().as_nanos();
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    black_box(&flags);
    report(label, elapsed, allocations, &flags);
}

fn measure_scalar(indices: &[usize], label: &str) {
    let mut pool = SmPool::new(vec![0_u8; ACTORS]);
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let started = Instant::now();
    for _ in 0..ROUNDS {
        for &index in indices {
            black_box(pool.process_indexed(index, Pulse, toggle));
        }
    }
    let elapsed = started.elapsed().as_nanos();
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    report(label, elapsed, allocations, pool.storage());
}

fn measure_batch(indices: &[usize], label: &str) {
    let mut pool = SmPool::new(vec![0_u8; ACTORS]);
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let started = Instant::now();
    for _ in 0..ROUNDS {
        black_box(pool.process_indexed_batch(indices.iter().copied(), Pulse, toggle));
    }
    let elapsed = started.elapsed().as_nanos();
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    report(label, elapsed, allocations, pool.storage());
}

fn report(label: &str, elapsed: u128, allocations: usize, flags: &[u8]) {
    let checksum: usize = flags.iter().map(|&flag| flag as usize).sum();
    assert_ne!(checksum, 0);
    assert_eq!(allocations, 0);
    let events = (ROUNDS * DISPATCHES) as f64;
    println!(
        "{label} {elapsed} ns total; {:.3} ns/event; {allocations} allocations; checksum {checksum}",
        elapsed as f64 / events
    );
}

fn main() {
    let mode = env::args().nth(1).unwrap_or_else(|| "all".to_owned());
    let local = ids(false);
    let random = ids(true);
    match mode.as_str() {
        "direct-local" => measure_direct(&local, "rust-direct-local"),
        "direct-random" => measure_direct(&random, "rust-direct-random"),
        "scalar-local" => measure_scalar(&local, "rust-pool-scalar-local"),
        "scalar-random" => measure_scalar(&random, "rust-pool-scalar-random"),
        "batch-local" => measure_batch(&local, "rust-pool-batch-local"),
        "batch-random" => measure_batch(&random, "rust-pool-batch-random"),
        "all" => {
            measure_direct(&local, "rust-direct-local");
            measure_direct(&random, "rust-direct-random");
            measure_scalar(&local, "rust-pool-scalar-local");
            measure_scalar(&random, "rust-pool-scalar-random");
            measure_batch(&local, "rust-pool-batch-local");
            measure_batch(&random, "rust-pool-batch-random");
        }
        _ => panic!("unknown benchmark mode: {}", mode),
    }
}
