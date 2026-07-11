//! Native Rust async-dispatch benchmark matching the C++ `co_sm` player workload.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context as TaskContext, Poll, RawWaker, RawWakerVTable, Waker};
use sml::sml;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

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

pub struct OpenClose;
pub struct CdDetected;
pub struct Play;
pub struct Pause;
pub struct EndPause;
pub struct Stop;

sml! {
    AsyncPlayer {
        *Empty + event<OpenClose> / async action_open = Open,
         Open + event<OpenClose> / async action_close = Empty,
         Empty + event<CdDetected> / async action_detect = Stopped,
         Stopped + event<Play> / async action_play = Playing,
         Playing + event<Pause> / async action_pause = Pause,
         Pause + event<EndPause> / async action_resume = Playing,
         Playing + event<Stop> / async action_stop_playing = Stopped,
         Pause + event<Stop> / async action_stop_paused = Stopped,
         Stopped + event<Stop> / async action_stop_again = Stopped,
         Stopped + event<OpenClose> / async action_open_stopped = Open,
    }
}

sml! {
    WrappedPlayer {
        *Empty + event<OpenClose> = Open,
         Open + event<OpenClose> = Empty,
         Empty + event<CdDetected> = Stopped,
         Stopped + event<Play> = Playing,
         Playing + event<Pause> = Pause,
         Pause + event<EndPause> = Playing,
         Playing + event<Stop> = Stopped,
         Pause + event<Stop> = Stopped,
         Stopped + event<Stop> = Stopped,
         Stopped + event<OpenClose> = Open,
    }
}

struct MachineContext;

impl WrappedPlayerStateMachineContext for MachineContext {}

impl AsyncPlayerStateMachineContext for MachineContext {
    async fn action_open(&mut self, _: &OpenClose) -> Result<(), ()> {
        Ok(())
    }
    async fn action_close(&mut self, _: &OpenClose) -> Result<(), ()> {
        Ok(())
    }
    async fn action_detect(&mut self, _: &CdDetected) -> Result<(), ()> {
        Ok(())
    }
    async fn action_play(&mut self, _: &Play) -> Result<(), ()> {
        Ok(())
    }
    async fn action_pause(&mut self, _: &Pause) -> Result<(), ()> {
        Ok(())
    }
    async fn action_resume(&mut self, _: &EndPause) -> Result<(), ()> {
        Ok(())
    }
    async fn action_stop_playing(&mut self, _: &Stop) -> Result<(), ()> {
        Ok(())
    }
    async fn action_stop_paused(&mut self, _: &Stop) -> Result<(), ()> {
        Ok(())
    }
    async fn action_stop_again(&mut self, _: &Stop) -> Result<(), ()> {
        Ok(())
    }
    async fn action_open_stopped(&mut self, _: &OpenClose) -> Result<(), ()> {
        Ok(())
    }
}

#[inline(always)]
fn barrier<T>(value: &mut T) {
    unsafe {
        core::arch::asm!("/* {0} */", in(reg) value, options(nostack, preserves_flags));
    }
}

fn noop_waker() -> Waker {
    unsafe fn clone(_: *const ()) -> RawWaker {
        raw_waker()
    }
    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}
    fn raw_waker() -> RawWaker {
        RawWaker::new(
            core::ptr::null(),
            &RawWakerVTable::new(clone, wake, wake_by_ref, drop),
        )
    }
    unsafe { Waker::from_raw(raw_waker()) }
}

fn block_on_ready<F: Future>(future: F) -> F::Output {
    let waker = noop_waker();
    let mut context = TaskContext::from_waker(&waker);
    let mut future = core::pin::pin!(future);
    match Pin::as_mut(&mut future).poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("benchmark callbacks must remain immediately ready"),
    }
}

async fn run(machine: &mut AsyncPlayerStateMachine<MachineContext>) {
    for _ in 0..1_000_000 {
        machine.process_event(OpenClose).await.unwrap();
        barrier(machine);
        machine.process_event(OpenClose).await.unwrap();
        barrier(machine);
        machine.process_event(CdDetected).await.unwrap();
        barrier(machine);
        machine.process_event(Play).await.unwrap();
        barrier(machine);
        machine.process_event(Pause).await.unwrap();
        barrier(machine);
        machine.process_event(EndPause).await.unwrap();
        barrier(machine);
        machine.process_event(Pause).await.unwrap();
        barrier(machine);
        machine.process_event(Stop).await.unwrap();
        barrier(machine);
        machine.process_event(Stop).await.unwrap();
        barrier(machine);
        machine.process_event(OpenClose).await.unwrap();
        barrier(machine);
        machine.process_event(OpenClose).await.unwrap();
        barrier(machine);
    }
}

async fn run_wrapped(machine: &mut WrappedPlayerStateMachine<MachineContext>) {
    for _ in 0..1_000_000 {
        machine.process_event(OpenClose).unwrap();
        barrier(machine);
        machine.process_event(OpenClose).unwrap();
        barrier(machine);
        machine.process_event(CdDetected).unwrap();
        barrier(machine);
        machine.process_event(Play).unwrap();
        barrier(machine);
        machine.process_event(Pause).unwrap();
        barrier(machine);
        machine.process_event(EndPause).unwrap();
        barrier(machine);
        machine.process_event(Pause).unwrap();
        barrier(machine);
        machine.process_event(Stop).unwrap();
        barrier(machine);
        machine.process_event(Stop).unwrap();
        barrier(machine);
        machine.process_event(OpenClose).unwrap();
        barrier(machine);
        machine.process_event(OpenClose).unwrap();
        barrier(machine);
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    if mode == "wrapper" || mode == "all" {
        let mut wrapped = WrappedPlayerStateMachine::new(MachineContext);
        let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
        let start = Instant::now();
        block_on_ready(run_wrapped(&mut wrapped));
        let elapsed = start.elapsed().as_nanos();
        let wrapper_allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations_before;
        assert!(matches!(wrapped.state(), WrappedPlayerStates::Empty));
        println!(
            "rust-wrapper {elapsed} ns total; {:.3} ns/event; {wrapper_allocations} allocations",
            elapsed as f64 / 11_000_000.0
        );
    }
    if mode == "native" || mode == "all" {
        let mut machine = AsyncPlayerStateMachine::new(MachineContext);
        let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
        let start = Instant::now();
        block_on_ready(run(&mut machine));
        let elapsed = start.elapsed().as_nanos();
        let native_allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations_before;
        assert!(matches!(machine.state(), AsyncPlayerStates::Empty));
        println!(
            "rust-native {elapsed} ns total; {:.3} ns/event; {native_allocations} allocations",
            elapsed as f64 / 11_000_000.0
        );
    }
}
