//! CD-player throughput workload matching `sml.cpp/benchmark/simple/sml_player_sm.hpp`.

use sml::{sml, BranchStm, Dispatch, Machine, Policies, PolicyBundle, SwitchStm};
#[cfg(feature = "std")]
use sml::{sml_policies, FifoScheduler, InlineScheduler};
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

#[derive(Default)]
struct AllowAll;

impl Dispatch for AllowAll {
    fn dispatch(&self, _event: &'static str) -> bool {
        true
    }
}

type BranchPolicies = PolicyBundle<(), (), BranchStm>;
type SwitchPolicies = PolicyBundle<(), (), SwitchStm>;
type CustomPolicies = PolicyBundle<(), (), AllowAll>;

// Keep benchmark state observable to the optimizer without a compiler
// boundary that would weaken the crate's safety contract.
const fn barrier<T>(value: &mut T) {
    std::hint::black_box(value);
}

fn process(sm: &mut PlayerStateMachine<Context>, event: PlayerEvents) {
    let _ = Machine::process_event(sm, event);
}

fn process_with_policy<P: Policies<Scheduler = ()>>(sm: &mut PlayerStateMachine<Context, P>, event: PlayerEvents)
where
    PlayerStateMachine<Context, P>: Machine<PlayerEvents>,
{
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

fn run_policy<P: Policies<Scheduler = ()>>(sm: &mut PlayerStateMachine<Context, P>)
where
    PlayerStateMachine<Context, P>: Machine<PlayerEvents>,
{
    for _ in 0..1_000_000 {
        process_with_policy(sm, PlayerEvents::OpenClose(OpenClose));
        barrier(sm);
        process_with_policy(sm, PlayerEvents::OpenClose(OpenClose));
        barrier(sm);
        process_with_policy(sm, PlayerEvents::CdDetected(CdDetected));
        barrier(sm);
        process_with_policy(sm, PlayerEvents::Play(Play));
        barrier(sm);
        process_with_policy(sm, PlayerEvents::Pause(Pause));
        barrier(sm);
        process_with_policy(sm, PlayerEvents::EndPause(EndPause));
        barrier(sm);
        process_with_policy(sm, PlayerEvents::Pause(Pause));
        barrier(sm);
        process_with_policy(sm, PlayerEvents::Stop(Stop));
        barrier(sm);
        process_with_policy(sm, PlayerEvents::Stop(Stop));
        barrier(sm);
        process_with_policy(sm, PlayerEvents::OpenClose(OpenClose));
        barrier(sm);
        process_with_policy(sm, PlayerEvents::OpenClose(OpenClose));
        barrier(sm);
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "benchmark throughput is intentionally reported as a floating-point rate"
)]
fn run_policy_benchmark<P: Policies<Scheduler = ()>>(label: &str, policy: P)
where
    PlayerStateMachine<Context, P>: Machine<PlayerEvents>,
{
    let mut sm = PlayerStateMachine::new_with_policy(Context, policy);
    let start = Instant::now();
    run_policy(&mut sm);
    let elapsed = start.elapsed();

    assert!(sm.is(&PlayerStates::Empty));
    println!(
        "{label} {} ns total; {:.3} ns/event",
        elapsed.as_nanos(),
        elapsed.as_nanos() as f64 / 11_000_000.0
    );
}

#[cfg(feature = "std")]
sml_policies!(InlinePolicies { scheduler: InlineScheduler });
#[cfg(feature = "std")]
sml_policies!(FifoPolicies { scheduler: FifoScheduler<1024> });
#[cfg(feature = "tokio")]
sml_policies!(TokioPolicies { scheduler: sml::TokioScheduler });

// Awaits each event's completion before sending the next through a
// scheduled machine, so the rate includes the scheduler's per-event cost.
macro_rules! scheduled_run {
    ($sm:expr) => {{
        let sm = $sm;
        let mut accepted = 0_usize;
        for _ in 0..1_000_000 {
            for event in [
                PlayerEvents::OpenClose(OpenClose),
                PlayerEvents::OpenClose(OpenClose),
                PlayerEvents::CdDetected(CdDetected),
                PlayerEvents::Play(Play),
                PlayerEvents::Pause(Pause),
                PlayerEvents::EndPause(EndPause),
                PlayerEvents::Pause(Pause),
                PlayerEvents::Stop(Stop),
                PlayerEvents::Stop(Stop),
                PlayerEvents::OpenClose(OpenClose),
                PlayerEvents::OpenClose(OpenClose),
            ] {
                accepted += usize::from(sm.process_event_async(event).await);
            }
        }
        assert_eq!(accepted, 11_000_000);
    }};
}

#[allow(
    clippy::cast_precision_loss,
    reason = "benchmark throughput is intentionally reported as a floating-point rate"
)]
fn report(label: &str, start: Instant) {
    let elapsed = start.elapsed();
    println!(
        "{label} {} ns total; {:.3} ns/event",
        elapsed.as_nanos(),
        elapsed.as_nanos() as f64 / 11_000_000.0
    );
}

#[cfg(feature = "std")]
fn run_scheduled(mode: &str) -> bool {
    let start = Instant::now();
    match mode {
        "inline" => smol::block_on(async {
            scheduled_run!(PlayerStateMachine::new_with_policy(
                Context,
                InlinePolicies::default()
            ));
        }),
        "fifo" => smol::block_on(async {
            scheduled_run!(PlayerStateMachine::new_with_policy(
                Context,
                FifoPolicies::default()
            ));
        }),
        #[cfg(feature = "tokio")]
        "tokio" => tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap_or_else(|error| panic!("tokio runtime: {}", error))
            .block_on(async {
                scheduled_run!(PlayerStateMachine::new_with_policy(
                    Context,
                    TokioPolicies::default()
                ));
            }),
        _ => return false,
    }
    report(mode, start);
    true
}

#[cfg(not(feature = "std"))]
const fn run_scheduled(_mode: &str) -> bool {
    false
}

#[allow(
    clippy::cast_precision_loss,
    reason = "benchmark throughput is intentionally reported as a floating-point rate"
)]
fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "branch" {
        run_policy_benchmark("branch", BranchPolicies::default());
        return;
    }
    if mode == "switch" {
        run_policy_benchmark("switch", SwitchPolicies::default());
        return;
    }
    if mode == "custom" {
        run_policy_benchmark("custom", CustomPolicies::default());
        return;
    }
    if run_scheduled(&mode) {
        return;
    }

    let mut sm = PlayerStateMachine::new(Context);
    let async_mode = mode == "async";
    let start = Instant::now();
    if async_mode {
        smol::block_on(run_async(&mut sm));
    } else {
        run(&mut sm);
    }
    let elapsed = start.elapsed();

    assert!(sm.is(&PlayerStates::Empty));
    println!(
        "{} ns total; {:.3} ns/event",
        elapsed.as_nanos(),
        elapsed.as_nanos() as f64 / 11_000_000.0
    );
}
