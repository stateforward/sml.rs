//! CD-player throughput workload matching `sml.cpp/benchmark/simple/sml_player_sm.hpp`.

use sml::{sml, Machine};
use std::time::Instant;

pub struct OpenClose;
pub struct CdDetected;
pub struct Play;
pub struct Pause;
pub struct EndPause;
pub struct Stop;

sml! {
    Player {
        *Empty + event<OpenClose> / open_drawer = Open,
         Open + event<OpenClose> / close_drawer = Empty,
         Empty + event<CdDetected> / store_cd_info = Stopped,
         Stopped + event<Play> / start_playback = Playing,
         Playing + event<Pause> / pause_playback = Pause,
         Pause + event<EndPause> / resume_playback = Playing,
         Playing + event<Stop> / stop_playback = Stopped,
         Pause + event<Stop> / stop_playback = Stopped,
         Stopped + event<Stop> / stopped_again = Stopped,
         Stopped + event<OpenClose> / open_drawer = Open,
         Pause + event<OpenClose> / stop_and_open = Open,
         Playing + event<OpenClose> / stop_and_open = Open,
    }
}

struct Context;

impl PlayerStateMachineContext for Context {
    fn open_drawer(&mut self, _event: &OpenClose) -> Result<(), ()> {
        Ok(())
    }
    fn close_drawer(&mut self, _event: &OpenClose) -> Result<(), ()> {
        Ok(())
    }
    fn store_cd_info(&mut self, _event: &CdDetected) -> Result<(), ()> {
        Ok(())
    }
    fn start_playback(&mut self, _event: &Play) -> Result<(), ()> {
        Ok(())
    }
    fn pause_playback(&mut self, _event: &Pause) -> Result<(), ()> {
        Ok(())
    }
    fn resume_playback(&mut self, _event: &EndPause) -> Result<(), ()> {
        Ok(())
    }
    fn stop_playback(&mut self, _event: &Stop) -> Result<(), ()> {
        Ok(())
    }
    fn stopped_again(&mut self, _event: &Stop) -> Result<(), ()> {
        Ok(())
    }
    fn stop_and_open(&mut self, _event: &OpenClose) -> Result<(), ()> {
        Ok(())
    }
}

// Equivalent to sml.cpp's barrier: expose the machine address and clobber
// memory without adding std::hint::black_box's pointer-to-pointer temporary.
#[inline(always)]
fn barrier<T>(value: &mut T) {
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    unsafe {
        core::arch::asm!("/* {0} */", in(reg) value, options(nostack, preserves_flags));
    }

    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    std::hint::black_box(value);
}

#[inline(always)]
fn process(sm: &mut PlayerStateMachine<Context>, event: PlayerEvents) {
    let _ = Machine::process_event(sm, event);
}

fn run(sm: &mut PlayerStateMachine<Context>) {
    for _ in 0..1_000_000 {
        process(sm, PlayerEvents::OpenClose(OpenClose));
        barrier(sm);
        process(sm, PlayerEvents::OpenClose(OpenClose));
        barrier(sm);
        process(sm, PlayerEvents::CdDetected(CdDetected));
        barrier(sm);
        process(sm, PlayerEvents::Play(Play));
        barrier(sm);
        process(sm, PlayerEvents::Pause(Pause));
        barrier(sm);
        process(sm, PlayerEvents::EndPause(EndPause));
        barrier(sm);
        process(sm, PlayerEvents::Pause(Pause));
        barrier(sm);
        process(sm, PlayerEvents::Stop(Stop));
        barrier(sm);
        process(sm, PlayerEvents::Stop(Stop));
        barrier(sm);
        process(sm, PlayerEvents::OpenClose(OpenClose));
        barrier(sm);
        process(sm, PlayerEvents::OpenClose(OpenClose));
        barrier(sm);
    }
}

async fn run_async(sm: &mut PlayerStateMachine<Context>) {
    for _ in 0..1_000_000 {
        let _ = Machine::process_event_async(sm, PlayerEvents::OpenClose(OpenClose)).await;
        barrier(sm);
        let _ = Machine::process_event_async(sm, PlayerEvents::OpenClose(OpenClose)).await;
        barrier(sm);
        let _ = Machine::process_event_async(sm, PlayerEvents::CdDetected(CdDetected)).await;
        barrier(sm);
        let _ = Machine::process_event_async(sm, PlayerEvents::Play(Play)).await;
        barrier(sm);
        let _ = Machine::process_event_async(sm, PlayerEvents::Pause(Pause)).await;
        barrier(sm);
        let _ = Machine::process_event_async(sm, PlayerEvents::EndPause(EndPause)).await;
        barrier(sm);
        let _ = Machine::process_event_async(sm, PlayerEvents::Pause(Pause)).await;
        barrier(sm);
        let _ = Machine::process_event_async(sm, PlayerEvents::Stop(Stop)).await;
        barrier(sm);
        let _ = Machine::process_event_async(sm, PlayerEvents::Stop(Stop)).await;
        barrier(sm);
        let _ = Machine::process_event_async(sm, PlayerEvents::OpenClose(OpenClose)).await;
        barrier(sm);
        let _ = Machine::process_event_async(sm, PlayerEvents::OpenClose(OpenClose)).await;
        barrier(sm);
    }
}

fn main() {
    let mut sm = PlayerStateMachine::new(Context);
    let async_mode = std::env::args().nth(1).as_deref() == Some("async");
    let start = Instant::now();
    if async_mode {
        smol::block_on(run_async(&mut sm));
    } else {
        run(&mut sm);
    }
    let elapsed = start.elapsed();

    assert!(matches!(sm.state(), PlayerStates::Empty));
    println!(
        "{} ns total; {:.3} ns/event",
        elapsed.as_nanos(),
        elapsed.as_nanos() as f64 / 11_000_000.0
    );
}
