//! The `player_benchmark` CD-player workload through each sml.rs scheduler
//! policy, and the same workload through hsm.rs's queued `dispatch`.
//!
//! Each round sends the 11-event sequence of `examples/player_benchmark.rs`;
//! every caller awaits its event's completion before sending the next, which
//! is the latency an owner sees per event. `tokio-burst` queues a round's 11
//! events before awaiting them, which is the throughput a busy machine sees.

use hsm::{define, effect, initial, on, state, target, transition};
use sml::{sml, sml_policies, FifoScheduler, InlineScheduler, Machine, TokioScheduler};
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

const ROUNDS: usize = 100_000;
const EVENTS: usize = 11;
const RUNS: usize = 7;

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

pub struct Context;

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

sml_policies!(Inline { scheduler: InlineScheduler });
sml_policies!(Fifo { scheduler: FifoScheduler<1024> });
sml_policies!(Tokio { scheduler: TokioScheduler });

fn round() -> [PlayerEvents; EVENTS] {
    [
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
    ]
}

const HSM_ROUND: [&str; EVENTS] = [
    "open_close",
    "open_close",
    "cd_detected",
    "play",
    "pause",
    "end_pause",
    "pause",
    "stop",
    "stop",
    "open_close",
    "open_close",
];

async fn owned() -> usize {
    let mut machine = PlayerStateMachine::new(Context);
    let mut accepted = 0;
    for _ in 0..ROUNDS {
        for event in round() {
            accepted += usize::from(Machine::process_event_async(&mut machine, event).await);
            std::hint::black_box(&mut machine);
        }
    }
    accepted
}

async fn inline() -> usize {
    let machine = PlayerStateMachine::new_with_policy(Context, Inline::default());
    let mut accepted = 0;
    for _ in 0..ROUNDS {
        for event in round() {
            accepted += usize::from(machine.process_event_async(event).await);
        }
    }
    accepted
}

async fn fifo() -> usize {
    let machine = PlayerStateMachine::new_with_policy(Context, Fifo::default());
    let mut accepted = 0;
    for _ in 0..ROUNDS {
        for event in round() {
            accepted += usize::from(machine.process_event_async(event).await);
        }
    }
    accepted
}

async fn tokio_task() -> usize {
    let machine = PlayerStateMachine::new_with_policy(Context, Tokio::default());
    let mut accepted = 0;
    for _ in 0..ROUNDS {
        for event in round() {
            accepted += usize::from(machine.process_event_async(event).await);
        }
    }
    accepted
}

async fn tokio_burst() -> usize {
    let machine = PlayerStateMachine::new_with_policy(Context, Tokio::default());
    let mut accepted = 0;
    for _ in 0..ROUNDS {
        let completions = round().map(|event| machine.process_event_async(event));
        for completion in completions {
            accepted += usize::from(completion.await);
        }
    }
    accepted
}

#[derive(Debug)]
pub struct Player;

impl hsm::Instance for Player {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

type Effect = Pin<Box<dyn Future<Output = ()> + Send>>;

// The same no-op action as each sml.rs transition.
fn act(_ctx: &hsm::Context, _player: &mut Player, _event: &hsm::Event) -> Effect {
    Box::pin(async {})
}

async fn hsm_dispatch() -> usize {
    let ctx = hsm::Context::new();
    let model = define!("Player",
        initial!(target!("empty")),
        state!("empty",
            transition!(on!("open_close"), target!("../open"), effect!(act)),
            transition!(on!("cd_detected"), target!("../stopped"), effect!(act))
        ),
        state!("open",
            transition!(on!("open_close"), target!("../empty"), effect!(act))
        ),
        state!("stopped",
            transition!(on!("play"), target!("../playing"), effect!(act)),
            transition!(on!("stop"), target!("."), effect!(act)),
            transition!(on!("open_close"), target!("../open"), effect!(act))
        ),
        state!("playing",
            transition!(on!("pause"), target!("../pause"), effect!(act)),
            transition!(on!("stop"), target!("../stopped"), effect!(act)),
            transition!(on!("open_close"), target!("../open"), effect!(act))
        ),
        state!("pause",
            transition!(on!("end_pause"), target!("../playing"), effect!(act)),
            transition!(on!("stop"), target!("../stopped"), effect!(act)),
            transition!(on!("open_close"), target!("../open"), effect!(act))
        )
    );
    let Ok(machine) = hsm::start(&ctx, Player, model) else {
        return 0;
    };
    if machine.start().await.is_err() {
        return 0;
    }
    let mut accepted = 0;
    for _ in 0..ROUNDS {
        for name in HSM_ROUND {
            let event = hsm::Event::from_static(name);
            accepted += usize::from(machine.dispatch_borrowed(&ctx, event).await.is_ok());
        }
    }
    assert_eq!(machine.state(), "/Player/empty");
    accepted
}

fn measure(label: &str, run: impl Fn() -> usize) {
    let mut samples: Vec<Duration> = (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            let accepted = run();
            let elapsed = start.elapsed();
            assert_eq!(accepted, ROUNDS * EVENTS, "{label} rejected an event");
            elapsed
        })
        .collect();
    samples.sort();
    let median = samples[RUNS / 2];
    #[allow(clippy::cast_precision_loss, reason = "benchmark rates are reported as floats")]
    let per_event = median.as_nanos() as f64 / (ROUNDS * EVENTS) as f64;
    println!("{label:<24} {per_event:>9.1} ns/event  (median of {RUNS}, {} events)", ROUNDS * EVENTS);
}

fn main() {
    let only = std::env::args().nth(1);
    let wanted = |name: &str| only.as_deref().is_none_or(|only| only == name);
    let current = || {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap_or_else(|error| panic!("tokio runtime: {error}"))
    };
    let multi = || {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .unwrap_or_else(|error| panic!("tokio runtime: {error}"))
    };
    if wanted("owned") {
        measure("owned (no scheduler)", || smol::block_on(owned()));
    }
    if wanted("inline") {
        measure("InlineScheduler", || smol::block_on(inline()));
    }
    if wanted("fifo") {
        measure("FifoScheduler", || smol::block_on(fifo()));
    }
    // On the multi-thread runtime the caller is itself a task, as an owner's
    // callers are, rather than the thread blocked in `block_on`.
    let spawned = |runtime: tokio::runtime::Runtime, work: fn() -> Pin<Box<dyn Future<Output = usize> + Send>>| {
        runtime.block_on(async move { tokio::spawn(work()).await.unwrap_or(0) })
    };
    if wanted("tokio") {
        measure("TokioScheduler 1-thread", || current().block_on(tokio_task()));
        measure("TokioScheduler 2-worker", || spawned(multi(), || Box::pin(tokio_task())));
    }
    if wanted("tokio-burst") {
        measure("TokioScheduler burst", || spawned(multi(), || Box::pin(tokio_burst())));
    }
    if wanted("hsm") {
        measure("hsm.rs dispatch 1-thread", || current().block_on(hsm_dispatch()));
        measure("hsm.rs dispatch 2-worker", || spawned(multi(), || Box::pin(hsm_dispatch())));
    }
}
