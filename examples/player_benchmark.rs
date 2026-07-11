//! CD-player throughput workload matching `sml.cpp/benchmark/simple/sml_player_sm.hpp`.

use sml::sml;
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

fn run(sm: &mut PlayerStateMachine<Context>) {
    for _ in 0..1_000_000 {
        let _ = sm.process_event(OpenClose);
        barrier(sm);
        let _ = sm.process_event(OpenClose);
        barrier(sm);
        let _ = sm.process_event(CdDetected);
        barrier(sm);
        let _ = sm.process_event(Play);
        barrier(sm);
        let _ = sm.process_event(Pause);
        barrier(sm);
        let _ = sm.process_event(EndPause);
        barrier(sm);
        let _ = sm.process_event(Pause);
        barrier(sm);
        let _ = sm.process_event(Stop);
        barrier(sm);
        let _ = sm.process_event(Stop);
        barrier(sm);
        let _ = sm.process_event(OpenClose);
        barrier(sm);
        let _ = sm.process_event(OpenClose);
        barrier(sm);
    }
}

fn main() {
    let mut sm = PlayerStateMachine::new(Context);
    let start = Instant::now();
    run(&mut sm);
    let elapsed = start.elapsed();

    assert!(matches!(sm.state(), PlayerStates::Empty));
    println!(
        "{} ns total; {:.3} ns/event",
        elapsed.as_nanos(),
        elapsed.as_nanos() as f64 / 11_000_000.0
    );
}
