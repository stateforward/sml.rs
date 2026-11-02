# sml.rs

`sml.rs` is a `no_std`, allocation-free state-machine library for Rust. Its
`sml!` procedural macro mirrors the [Boost.SML](https://boost-ext.github.io/sml/)
transition-table DSL closely enough that tables can usually move between
`sml.cpp` and Rust mechanically.

```text
source state + event [guard] / action = target state
```

The generated machine uses ordinary Rust enums, borrows event data, stores its
context by value, and has no runtime allocator or dynamic dispatch overhead in
the default `NoPolicy` path.

## Install

<!-- installation dependency matching Cargo.toml package name and version -->

```toml
[dependencies]
sml = { package = "stateforward-sml", version = "2.0" }
```

The crate has no default features and works on `no_std` targets. Enable
`graphviz` only when build-time diagram generation is wanted:

```toml
sml = { package = "stateforward-sml", version = "2.0", features = ["graphviz"] }
```

The runtime and procedural-macro crates both forbid unsafe Rust at the source
and Cargo target boundaries; the quality gate also builds all package targets
with warnings denied. The generated machines use ordinary ownership and
borrowing, and the default queues use bounded storage with checked indexing
and overflow-safe counters. A custom
`ThreadSafety` policy does not make a machine shareable by itself: generated
event processing still requires `&mut self`, so shared access must use an
owner-provided synchronization boundary.

## Quick start

```rust
use sml::{sml, sml_policies, Emit, StepHook};

pub struct OpenClose;
pub struct CdDetected;
pub struct Play;
pub struct Stop;

sml! {
    Player {
        *"empty"_s + event<OpenClose> / open_drawer = "open"_s,
         "open"_s + event<OpenClose> / close_drawer = "empty"_s,
         "empty"_s + event<CdDetected> = "stopped"_s,
         "stopped"_s + event<Play> / start_playback = "playing"_s,
         "playing"_s + event<Stop> / stop_playback = X,
    }
}

/// Actions record what they did; the step hook receives it as data.
#[derive(Default)]
struct Context {
    emitted: Vec<&'static str>,
}

impl PlayerStateMachineContext for Context {
    fn open_drawer(&mut self, _: &OpenClose) -> Result<(), ()> {
        self.emitted.push("drawer opened");
        Ok(())
    }

    fn close_drawer(&mut self, _: &OpenClose) -> Result<(), ()> {
        self.emitted.push("drawer closed");
        Ok(())
    }

    fn start_playback(&mut self, _: &Play) -> Result<(), ()> {
        self.emitted.push("playing");
        Ok(())
    }

    fn stop_playback(&mut self, _: &Stop) -> Result<(), ()> {
        self.emitted.push("stopped");
        Ok(())
    }
}

impl Emit<&'static str> for Context {
    fn drain_outputs(&mut self, sink: impl FnMut(&'static str)) {
        self.emitted.drain(..).for_each(sink);
    }
}

#[derive(Default)]
struct Print;

impl<L> StepHook<L, PlayerEvents> for Print {
    type Output = &'static str;

    fn output(&mut self, _logger: &mut L, output: &'static str) {
        println!("{output}");
    }
}

sml_policies!(Production { step_hook: Print });

fn main() {
    let mut player = PlayerStateMachine::new_with_policy(Context::default(), Production::default());

    assert_eq!(player.process_event(OpenClose), Ok(true));
    assert_eq!(player.process_event(OpenClose), Ok(true));
    assert_eq!(player.process_event(CdDetected), Ok(true));
    assert_eq!(player.process_event(Play), Ok(true));
    assert_eq!(player.process_event(Stop), Ok(true));
    // `X` takes no more events.
    assert_eq!(player.process_event(Play), Ok(false));
}
```

The table generates `PlayerStateMachine`, `PlayerStateMachineContext`,
`PlayerStates`, `PlayerEvents`, and `PlayerError`. Callback methods are inferred
from the guards, actions, lifecycle hooks, and error handlers used by the table.

## DSL at a glance

<!-- transition spellings accepted by the sml! parser and documented in docs/dsl.md -->

| Purpose | Spelling |
|---|---|
| Initial transition | `*"idle"_s + event<Start> = "running"_s` |
| Reverse form | `"running"_s <= *"idle"_s + event<Start>` |
| Guard and action | `state + event<E> [ready] / run = target` |
| Guard expression | `[authorized && (!expired || admin)]` |
| Action sequence | `/ (first, second, third)` |
| Internal transition | `state + event<E> / action` |
| External self-transition | `state + event<E> / action = state` |
| Anonymous completion | `*"boot"_s / initialize = "ready"_s` |
| Origin-aware completion | `state + completion<E> / finish = target` |
| Entry and exit | `state + on_entry<_> / enter` and `on_exit<_>` |
| Unexpected event | `state + unexpected_event<E> / recover` |
| Error transition | `state + exception<MyError> / recover = target` |
| Defer current event | `state + event<E> / defer` |
| Process another event | `state + event<E> / process(Next {}) = target` |
| Terminal state | `X` |
| Shallow history | `*"idle"_s(H)` inside a child table |

Named events use `"event name"_e`; typed states use `state<T>`. Prefix a guard
or action with `async` to generate an async machine, for example
`[async ready] / async send`.

See [the complete DSL guide](docs/dsl.md) for callback signatures, state data,
orthogonal regions, composite machines, exceptions, and queue behavior.

## Generated API

<!-- public methods emitted by the flat, orthogonal, and composite generators -->

A generated machine is driven, not inspected. Its public API is construction,
`initialize()`, and `process_event`; state and context can change under any
reader, so nothing that reads them is public. Anything an owner needs to know
it asks with an event and learns from what the step emits (see
[Step hooks](#step-hooks)). The inspection API exists only for tests, behind a
policy whose `testing` slot is `TestingPolicy`:

<!-- public event-processing traits from src/lib.rs -->

The inherent `process_event` returns `Result<bool, Error>`: `Ok(true)` when a
transition handled the event, `Ok(false)` when none did, and `Err` when a
guard, action, or bounded queue failed. Synchronous flat and composite
machines also implement `Machine<E>` with `process_event` and
`process_event_async`, both reporting acceptance as `bool`; the latter returns
an allocation-free ready future. Async callbacks are a separate concern from
scheduling: an async-callback machine's inherent `process_event` is itself
`async`.

| Shape | With `TestingPolicy` only |
|---|---|
| Flat | `state()`, `is(...)`, `visit_current_state(...)`, `visit_current_events(...)`, `is_terminated()`, `context()`, `logger()`, `observer()`, `set_current_states(...)` |
| Orthogonal | `states()`, `state(region)`, `is(...)`, `is_region(...)`, `visit_current_events(...)`, `is_terminated()`, `context()`, `logger()`, `observer()`, `set_current_states(...)` |
| Composite | Parent and typed child state, identity, active, and visitor queries, `is_terminated()`, `context()`, `logger()`, `observer()`, setters |

Call `initialize()` when the initial state's entry hook or anonymous completion
must run before the first external event. `process_event` automatically runs
completion stabilization after every handled event.

A test can query the static event types reachable from the current state with
`visit_current_events`:

```rust
use core::any::TypeId;
use sml::{Event, EventVisitor};

struct Visitor;

impl EventVisitor for Visitor {
    fn visit<E: Event>(&mut self) {
        let type_id = TypeId::of::<E>();
        println!("{}: {:?}", E::name(), type_id);
    }
}

machine.visit_current_events(&mut Visitor);
```

The query is read-only, allocation-free, and does not evaluate guards or
observe context. It includes transitions declared on active composite
ancestors and reports the union of active orthogonal regions, once per static
payload type. Catch-all unexpected transitions are omitted; specific typed
unexpected transitions are included. The generated `Events::EVENT_NAMES`
constant provides the corresponding deterministic variant-name table. The
visitor API is a testing facility; it is not exposed through events or
messages. Payloads containing non-static borrowed lifetimes are not reported,
because `TypeId` requires a `'static` type.

Generated callbacks return `Result`. Without a custom error type, guards use
`Result<bool, ()>` and actions use `Result<(), ()>`. A final action targeting a
payload state returns that payload value instead.

### Policies

Generated machines take an optional policy bundle:

```rust
use sml::{sml, sml_policies, Logger, TestingPolicy};

#[derive(Default)]
struct Trace;
impl Logger for Trace {}

sml_policies!(TestPolicies {
    logger: Trace,
    testing: TestingPolicy,
});

// The default constructor remains the zero-cost NoPolicy path. A custom
// bundle is supplied explicitly:
let machine = MyStateMachine::new_with_policy(context, TestPolicies::default());
```

`NoPolicy` is the default and all of its slots are zero-sized. Its generated
machines retain the allocation-free default queues and the normal dispatch
code. `sml_policies!` names only the slots a consumer changes; omitted slots
use `()`, `JumpTable`, or the built-in bounded queue factory.

The policy hooks correspond to the four `sml.cpp` logger callbacks:
`Logger::log_process_event`, `log_state_change`, `log_action`, and
`log_guard`. Generated events and states implement `EventName` and `StateName`,
so adapters can use stable static variant names without serializing machine
state. `Observer` receives the same callbacks and is useful for tracing or
OpenTelemetry adapters without adding a telemetry dependency to sml.rs.

`JumpTable` is the default dispatch strategy; `BranchStm` and `SwitchStm` are
available as parity names. A custom `Dispatch` policy can accept or reject an
event and can select among same-event transition candidates before generated
guards and actions run. It cannot replace guard, action, queue, completion, or
hierarchy semantics, so guards remain evaluated by `process_event`.

`ThreadSafe<L>` wraps a `RawMutex` and holds its guard for event processing.
The `spin` feature provides a no-std mutex adapter; the `std` feature provides
the `std::sync::Mutex<T>` adapter. The default `()` thread-safety policy is a
no-op and adds no allocation. A custom lock must keep its guard scoped to the
lock state itself; generated machines may mutate the other policy slots while
the guard is held.

`TestingPolicy` opts a machine into the inspection API (state, identity,
visitor, termination, context, and policy-slot queries) and the
`set_current_states` helper; production machines expose none of them. The
capability marker is sealed, so a custom production policy cannot opt itself
into state mutation. Flat machines pass one state; orthogonal machines pass
one state per region.
`PolicyBundle` also selects the defer and process queue factories, allowing a
bounded ring or another user-owned allocation-free container to implement
`Queue<E>`.

`Policies` and `PolicyParts` are sealed implementation traits. This is
intentional: custom slot implementations remain supported, while the generated
machine retains ownership of policy storage and keeps the thread-safety guard
separate from the mutable transition engine. Use `PolicyBundle` or
`sml_policies!` to compose policies; do not implement those integration traits
directly.

These policies are for the machine's owner to configure and query its own
machine. They are not a general introspection or control channel and must not
be made reachable through an event, message, or other domain input.

### Schedulers

The `scheduler` slot decides who runs a machine's events. The default `()`
keeps the owned machine above. A queued scheduler turns the generated machine
into a cloneable handle that many tasks can call through `&self`:

```rust
use sml::{sml_policies, TokioScheduler};

sml_policies!(Shared { scheduler: TokioScheduler });

let machine = MyStateMachine::new_with_policy(context, Shared::default());
let handle = machine.clone();
tokio::spawn(async move { handle.process_event_async(Start).await });
let accepted: bool = machine.process_event_async(Stop).await;
```

`process_event_async` queues the event before it returns and resolves to its
acceptance once the event has run to completion, including the machine's own
`process`/`defer` queues and completion transitions. Dropping the completion
does not cancel the event. `try_process_event` returns `QueueFull` instead of
waiting when a bounded queue has no room. Events run in FIFO order, one at a
time, which is `sml.cpp`'s `strict_ordering_scheduler_contract`
(`StrictOrdering`). Once the machine reaches `X`, queued and later events
resolve to `false`. A queued machine runs its initial entry when constructed,
as `sml.cpp` does.

| Scheduler | Feature | Runs an event |
|---|---|---|
| `InlineScheduler` | `std` | In the caller, under the machine's lock, before `process_event_async` returns |
| `FifoScheduler<N>` | `std` | `sml.cpp`'s draining model: a caller that finds the machine idle runs its event and drains what others queued meanwhile; a caller that finds it busy, including a re-entrant action, only queues. `N` bounds the queue (power of two) |
| `TokioScheduler` | `tokio` | On a dedicated task that owns the machine; callers only queue, and no lock is held across `.await` |

Queued schedulers drive synchronous machines without a temporary context. A
scheduled machine is a handle whose only operations are `process_event_async`,
`try_process_event`, and `Clone`.

#### Scopes

`TokioScope` groups machines under one owner, and scopes nest the way the
machines do (host → projects → each project's machines):

```rust
use sml::{sml_policies, TokioScheduler, TokioScope};

sml_policies!(Shared { scheduler: TokioScheduler });

let projects = TokioScope::new(8); // at most 8 project machines step at once
let host = HostStateMachine::new_with_policy(
    host_context,
    Shared::default().with_scheduler(TokioScheduler::default().owning(projects.clone())),
);
let children = TokioScope::new(4);
let project = ProjectStateMachine::new_with_policy(
    project_context,
    Shared::default().with_scheduler(projects.scheduler().owning(children.clone())),
);
let child = ChildStateMachine::new_with_policy(
    child_context,
    Shared::default().with_scheduler(children.scheduler()),
);
```

Each machine in a scope keeps its own task and FIFO queue, so one machine's
slow step never delays a sibling's queue. The scope caps how many of its
machines run a step at once; waiting steps get a slot in FIFO order, so a busy
machine cannot starve the others, and each scope's cap is independent of every
other scope's. When a scope's owner reaches `X`, or is itself ended by its own
scope, the scope ends: every machine in it stops, its queued and later callers
resolve to `false`, and each machine that owns a scope ends that scope in turn.
A machine placed in an ended scope never starts. `TokioScope::end` ends a scope
explicitly. Scopes are Tokio-only: `InlineScheduler` and `FifoScheduler` run
steps in their callers and have no task to end.

### Step hooks

The `step_hook` slot is how an owner learns what a step did. Actions record
what they emit in the context, which implements `Emit<Output>`; after the
event has run to completion the generated step drains those outputs and hands
each to `StepHook::output`, then calls `after_step` with a `Step` (event name,
acceptance, termination), all before a scheduler resolves the caller's
completion. `before_step` sees the input event first, so one hook can journal
the input, collect the outputs as data, and act on them. The hook never sees
the context. It is a separate slot because `Logger` only receives static
names, never event payloads or outputs.

```rust
use sml::{Emit, Step, StepHook};

impl Emit<Command> for ProjectContext {
    fn drain_outputs(&mut self, sink: impl FnMut(Command)) {
        self.commands.drain(..).for_each(sink);
    }
}

impl<L> StepHook<L, ProjectEvents> for Journal {
    type Output = Command;

    fn before_step(&mut self, _logger: &mut L, event: &ProjectEvents) { /* record the input */ }
    fn output(&mut self, _logger: &mut L, command: Command) { self.pending.push(command); }
    fn after_step(&mut self, _logger: &mut L, step: Step) { /* journal, then send `pending` */ }
}
```

Every context implements `Emit<()>` as a no-op, which the default `()` hook
uses. A context that buffers outputs needs a hook whose `Output` matches, or
its buffer is never drained.

## State and event data

`event<E>` accepts an owned Rust event at the public boundary and passes `&E`
to guards and actions. A flat `state<T>` becomes a payload-bearing state enum
variant. Initial payloads and actionless payload targets use `Default`; a
producing transition returns the destination payload from its final action.

This is the ownership-safe counterpart to `sml.cpp` callbacks that mutate a
destination state object. A machine has one callback error type; use a Rust
enum when several error variants need distinct exception routing.

Flat tables may declare dispatch-scoped event generics on the machine header:

```rust
use core::fmt::Debug;
use sml::sml;

pub struct Message<'a, T, const N: usize> {
    pub value: &'a T,
    pub bytes: [u8; N],
}

sml! {
    Generic<'event, T, const N: usize>
    where
        T: Debug + 'event,
    {
        *"idle"_s + event<Message<'event, T, N>> / inspect,
    }
}
```

This generates `GenericEvents<'event, T, N>` plus generic `inspect` and
`process_event` methods. The `GenericStateMachine` value itself does not store
the event parameters, so one machine can dispatch multiple concrete
monomorphizations. `event<&'event mut Operation<T>>` passes the mutable borrow
directly to its callbacks; the borrow lasts until dispatch completes, including
through `.await` for an asynchronous machine.
Mutable borrowed events cannot be retained as origin data for
`completion<Event>`; the macro reports this unsupported combination directly.
Bounds and `where` clauses are copied to every generated event API that needs
them. Every declared parameter must appear in at least one event payload, and
every external event in one generic table must carry every declared type and
const parameter so `process_event` can infer the single generated event family;
lifetimes may remain event-specific, including inside nested type arguments.
Higher-ranked lifetimes such as `for<'borrow> fn(&'borrow T)` remain bound inside
the payload type rather than becoming event-family parameters; the equivalent
bare-function elision `fn(&T)` remains implicitly late-bound. Generated callback
signatures retain only the event-specific lifetimes and predicates they use.
Temporary contexts may use event parameters and propagate them to every callback,
but they must carry every declared type and const parameter and do not by
themselves define an event family. See the
[generic-event guide](docs/dsl.md#generic-event-types) and the
[`generic_events` example](examples/generic_events.rs).

Because type and const parameters are dispatch-scoped, they cannot appear in
stored state data or typed exception payloads. A declared lifetime may be
shared with state data; generated dispatch methods reuse that machine lifetime.
A `where` clause requires at least one declared parameter.

## Orthogonal and composite machines

Multiple `*` rows create orthogonal regions. One borrowed event is broadcast
to every active region, and the machine terminates only after every region
reaches `X`.

Adjacent named tables form native composite machines:

```rust
use sml::sml;

pub struct Enter;
pub struct Work;

sml! {
    Child {
        *"idle"_s + event<Work> = X,
    }

    Parent {
        *"outside"_s + event<Enter> = state<Child>,
         state<Child> / child_completed = X,
    }
}
```

The parent owns its descendants. Dispatch is deepest-active-child first and
bubbles upward only when a child does not handle the event. Scalar and
orthogonal nodes can be nested recursively; lifecycle, completion, exception,
payload, async, defer/process, and history behavior participates in the same
ownership tree.

## Runtime utilities

<!-- public utility types in src/utility.rs -->

The `sml::utility` module provides allocation-free building blocks for cases
outside the static DSL:

- `EventQueue` and `EventQueues` for bounded defer/process ordering;
- `DispatchTable` for checked contiguous runtime event IDs;
- `SmPool` for indexed and batch dispatch over machine storage;
- `OrthogonalRegions` for broadcasting through a collection of machines;
- `Hierarchical` for generic parent/child bubbling and shallow history.

The SDL-style runtime-ID adapter is covered by
[`tests/sdl_adapter.rs`](tests/sdl_adapter.rs).

## Diagram generation

<!-- graphviz feature behavior implemented by macros/src/lib.rs -->

With the `graphviz` feature active, compiling a flat `sml!` table renders
`sml_<Machine>.svg` when the `dot` executable is available. If Graphviz is not
installed, the macro writes `sml_<Machine>.dot` under Cargo's `OUT_DIR`
instead. Diagram generation is a build-time feature and is not required at
runtime.

## `sml.cpp` parity

The repository includes a compiling behavioral translation for every one of
the 25 programs under `../sml.cpp/example` in
[`tests/sml_cpp_examples.rs`](tests/sml_cpp_examples.rs).

- [Capability parity matrix](docs/sml-cpp-parity.md)
- [Example-by-example translation audit](docs/sml-cpp-examples.md)
- [Migrating from 0.8 to 1.0](docs/migrating-to-1.0.md)
- [Migrating from 1.x to 2.0](docs/migrating-to-2.0.md)
- [Stability policy](docs/stability.md)

Rust-specific mappings, including trait methods instead of inline lambdas,
context fields instead of type-based dependency injection, `Result` instead of
thrown values, and policy-selectable dispatch lowering, are documented and tested
there.

## Performance

<!-- benchmark workloads and runner under examples/*_benchmark.rs and benchmarks/ -->

The CD-player benchmark performs 11 million equivalent event dispatches and
uses a direct-address compiler barrier after every dispatch in both languages.

```bash
RUSTFLAGS="-C target-cpu=native" \
  cargo run --release --example player_benchmark

clang++ -std=c++20 -O3 -DNDEBUG -march=native \
  -I../sml.cpp/include \
  benchmarks/player_cpp.cpp -o /tmp/sml_cpp_player
/tmp/sml_cpp_player
```

In 21 alternating native-release runs on 2026-07-13, the new
`Machine::process_event` path completed the workload in a 2.370 ms median versus
3.424 ms for `sml.cpp`, 30.8% lower elapsed time and 44.5% higher throughput on
the test machine. These small timing differences are sensitive to scheduling and
thermals, so compare repeated alternating runs locally.

The same player workload can select each dispatch policy by passing a mode to
the benchmark. The default invocation above remains the `NoPolicy`/`JumpTable`
baseline; these rows are intentionally measured separately because the policy
selection is a compile-time monomorphization:

| Mode | Policy | Command |
|---|---|---|
| baseline | `NoPolicy` / `JumpTable` | `cargo run --release --example player_benchmark` |
| branch | `BranchStm` | `cargo run --release --example player_benchmark -- branch` |
| switch | `SwitchStm` | `cargo run --release --example player_benchmark -- switch` |
| custom | owner `Dispatch` policy | `cargo run --release --example player_benchmark -- custom` |

Each alternate mode prints the same `ns/event` result and executes the same
11-million-event sequence. The custom row uses a trivial allow-all policy to
measure policy-hook overhead separately from owner-specific routing work.

The same Rust executable accepts `async` to measure
`Machine::process_event_async`. The matching C++ harness uses `co_sm` with its
inline scheduler:

```bash
./target/release/examples/player_benchmark async

clang++ -std=c++20 -O3 -DNDEBUG -march=native \
  -I../sml.cpp/include \
  benchmarks/co_sm_inline_cpp.cpp -o /tmp/sml_cpp_co_sm_inline
/tmp/sml_cpp_co_sm_inline
```

In 21 alternating runs, Rust completed the async RTC workload in a 2.543 ms
median versus 12.849 ms for inline C++ `co_sm`, 80.2% lower elapsed time and
405.3% higher throughput. This measures the uncontended ready path only.

### Scheduler policies

The same workload runs through each scheduler policy; every caller awaits its
event's completion before sending the next. `inline`, `fifo`, and `tokio`
modes of `player_benchmark` measure 11 million events:

```bash
cargo run --release --features tokio --example player_benchmark -- inline
cargo run --release --features tokio --example player_benchmark -- fifo
cargo run --release --features tokio --example player_benchmark -- tokio
```

[`benchmarks/scheduler_compare`](benchmarks/scheduler_compare) runs the
workload head to head with hsm.rs's queued `HSM::dispatch` (from a sibling
`hsm/hsm.rs` checkout, one no-op effect per transition, mirroring the sml.rs
actions). It is a separate workspace so hsm.rs stays out of this crate's
dependency gates:

```bash
cargo run --release --manifest-path benchmarks/scheduler_compare/Cargo.toml
```

Medians of 7 runs of 1.1 million events on an Apple M3 Max with Rust 1.98
(2026-11-01):

| Path | ns/event |
|---|---:|
| Owned machine, `Machine::process_event_async` (no scheduler) | 2.0 |
| `InlineScheduler` | 5.0 |
| `FifoScheduler<1024>` | 17.6 |
| `TokioScheduler`, current-thread runtime | 155.4 |
| `TokioScheduler`, 2 workers, caller is a task | 150.0 |
| `TokioScheduler`, 2 workers, 11 events queued then awaited | 81.0 |
| hsm.rs `dispatch`, current-thread runtime | 258.1 |
| hsm.rs `dispatch`, 2 workers, caller is a task | 251.1 |

A `TokioScheduler` event pays one channel send, one task wake-up, and one
oneshot reply; queuing several events before awaiting them amortizes the
wake-ups. hsm.rs runs the event in the dispatching caller, so it has no
hand-off, but each transition still costs more than the hand-off. A caller
blocked in `Runtime::block_on` on a multi-thread runtime, rather than running
as a task, pays a cross-thread wake-up per event (several microseconds).

Reproduce both comparisons, including alternating order, raw samples, platform,
and toolchain capture, with:

```bash
python3 benchmarks/compare_machine_rtc.py --runs 21 \
  --output benchmarks/results/machine-rtc.json
```

The recorded run behind the numbers above is stored in
`benchmarks/results/2026-07-13-machine-rtc.json`.

### Compile time

The compile-time runner alternates clean production builds of the same player
program. The Rust side uses a temporary consumer crate whose only dependency is
this repository; the C++ side compiles the equivalent translation unit against
`sml.cpp`. Both use native CPU tuning, full optimization, and linking. Lockfile
generation and dependency downloads are outside the timed region. The rebuild
case appends a newline to the player source, preserving Rust dependency
artifacts while requiring both languages to rebuild and relink the program.

```bash
python3 benchmarks/compare_compile_time.py --runs 7
```

The 2026-07-11 alternating run on Apple Silicon used Rust 1.94.0 and Apple
Clang 16.0.0:

| Toolchain | Clean release build | Player edit and rebuild |
|---|---:|---:|
| Rust `sml.rs` | 4.714 s | 1.828 s |
| C++ `sml.cpp` | 0.462 s | 0.457 s |

Rust was 10.20 times slower from a clean target and 4.00 times slower after a
player-source edit. The clean Rust build includes compiling `proc-macro2`,
`quote`, `syn`, and `stateforward-sml-macros` from locally cached sources. The C++ workload
parses its header-only dependency in the player translation unit. These are
developer-visible build times, separate from the runtime throughput results.

### `SmPool` throughput

The pool runner compares `SmPool<Vec<u8>>` with C++ `sm_pool` using the same
10,000-slot, 50,000-event workload, identical local and seeded-random indices,
and 1,001 rounds per sample. Each language is also measured against its own
flat byte-array loop. Setup storage is created before timing; the safe Rust
harness does not install a global allocator, so the comparison measures the
timed throughput path without weakening the crate's safety contract.

```bash
python3 benchmarks/compare_sm_pool.py --runs 21
```

The 2026-07-11 rotated native-release run produced:

| Path | Local | Random |
|---|---:|---:|---:|
| Rust flat array | 0.312 ns/event | 0.335 ns/event |
| C++ flat array | 0.275 ns/event | 0.282 ns/event |
| Rust `SmPool` scalar | 0.421 ns/event | 0.430 ns/event |
| C++ `sm_pool` scalar | 0.629 ns/event | 0.680 ns/event |
| Rust `SmPool` batch | 0.362 ns/event | 0.370 ns/event |
| C++ `sm_pool` batch | 0.474 ns/event | 0.478 ns/event |

Rust batch dispatch was 23.6% faster locally and 22.6% faster under random
access than C++ `sm_pool`. It stayed within 16.0% of Rust's flat-array local
baseline and 10.4% of its random baseline. The compact byte state isolates pool
dispatch and access locality from application-specific work.

### Async, allocator, and worker-pool comparison

The extended runner compares the same 11-million-event player sequence through
Rust futures and C++ `co_sm` allocator policies. It also compares bounded,
persistent eight-worker fork/join schedulers over 5,000 rounds. The safe Rust
harnesses do not install a global allocator: allocator instrumentation would
require a forbidden implementation path. Their timed loops use the prebuilt
machine and fixed caller-owned storage; the runner accepts optional allocation
counts from harnesses that provide them.

```bash
# The thread-pool policy currently lives on this sibling branch.
mkdir -p /tmp/sml-cpp-thread-pool
git -C ../sml.cpp archive origin/thread-pool-scheduler |
  tar -x -C /tmp/sml-cpp-thread-pool

python3 benchmarks/compare_extended.py \
  --runs 21 \
  --thread-pool-cpp-dir /tmp/sml-cpp-thread-pool
```

The 2026-07-11 alternating run produced:

| Workload | Median | Allocation instrumentation | Completed runs |
|---|---:|---:|---:|
| Rust async façade over synchronous actions | 0.361 ns/event | safe harness; not instrumented | 21/21 |
| C++ inline `co_sm` | 1.982 ns/event | inline fast path | 21/21 |
| Rust machine with native async callbacks | 3.372 ns/event | safe harness; not instrumented | 21/21 |
| C++ `co_sm` with pooled coroutine frames | 21.506 ns/event | pooled frame/event | 21/21 |
| C++ `co_sm` with heap coroutine frames | 49.427 ns/event | heap frame/event | 21/21 |
| Rust fixed-lane worker pool | 259.255 ns/task | 0 | 21/21 |
| C++ fixed-ring thread-pool scheduler | 1,139.283 ns/task | fixed inline slots | 13/21 |

The async-façade rows are directly comparable: both wrap synchronous actions.
The native Rust row additionally awaits actual async action futures, which the
C++ player table does not model. The worker-pool rows compare policy designs,
not identical implementations: Rust uses one fixed atomic lane per worker,
while C++ uses a shared fixed MPMC task ring. Eight C++ runs exceeded the
runner's five-second timeout; the median above includes completed runs only and
must be read together with that reliability result. The safe Rust harnesses
avoid allocator instrumentation and exercise their stack-polled paths
directly.

## Development

<!-- enforced quality commands from scripts/quality_gates.sh and .github/workflows/*.yml -->

The same gate runs locally and on every push and pull request:

```bash
./scripts/quality_gates.sh
```

It enforces formatting, warning-free Clippy across every target and feature,
the full feature matrix, rustdoc warnings, documentation links, Python harness
syntax, package construction, dependency advisories and licenses, at least 90%
runtime line coverage, and 100% runtime function coverage. The runtime and
procedural-macro implementations also deny Clippy's `pedantic` and `nursery`
groups; the macro generator has only a documented allow-list for structural
token-building heuristics. Conversion, panic, indexing, overflow, and other
high-risk checks remain denied, and every lint suppression carries an explicit
reason. Production crates also reject implicit numeric suffix formatting and
deny package-metadata lint failures. The compile-time procedural-macro
implementation is excluded from the runtime coverage report; its unit tests
remain part of the workspace test gates. Separate required jobs run the suite
on Linux, macOS, and Windows, enforce public API compatibility, execute
AddressSanitizer and Miri, and fuzz the runtime utilities.

Run the fuzz target locally with the pinned nightly toolchain, `cargo-fuzz` version,
and checked-in Cargo wrapper:

```bash
cargo install cargo-fuzz --version 0.13.2 --locked
REAL_CARGO="$(command -v cargo)"
export REAL_CARGO
export PATH="${PWD}/ci:${PATH}"
cargo +nightly-2026-08-24 check --manifest-path fuzz/Cargo.toml --locked
cargo +nightly-2026-08-24 fuzz run -v runtime_utilities -- -max_total_time=60 -timeout=10
git diff --exit-code -- fuzz/Cargo.lock
```

The crate is licensed under either Apache-2.0 or MIT.

Release maintainers should follow the [release runbook](docs/releasing.md).
