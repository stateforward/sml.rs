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
sml = { package = "stateforward-sml", version = "1.4" }
```

The crate has no default features and works on `no_std` targets. Enable
`graphviz` only when build-time diagram generation is wanted:

```toml
sml = { package = "stateforward-sml", version = "1.4", features = ["graphviz"] }
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
use sml::sml;

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

#[derive(Default)]
struct Context {
    actions: usize,
}

impl PlayerStateMachineContext for Context {
    fn open_drawer(&mut self, _: &OpenClose) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    fn close_drawer(&mut self, _: &OpenClose) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    fn start_playback(&mut self, _: &Play) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    fn stop_playback(&mut self, _: &Stop) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }
}

fn main() -> Result<(), PlayerError> {
    let mut player = PlayerStateMachine::new(Context::default());

    assert!(player.is(&PlayerStates::Empty));
    player.process_event(OpenClose)?;
    player.process_event(OpenClose)?;
    player.process_event(CdDetected)?;
    player.process_event(Play)?;
    player.process_event(Stop)?;

    assert!(player.is_terminated());
    assert_eq!(player.context().actions, 4);
    Ok(())
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

All generated machines provide construction, context access, event processing,
and termination queries. The state API follows the machine shape:

<!-- public event-processing traits from src/lib.rs -->

Synchronous flat machines implement `Machine<E>` with `process_event` and
`process_event_async`. Both report event acceptance as `bool`, matching the
`sml.cpp` `co_sm` contract. The latter returns an allocation-free ready future
after the uncontended inline run-to-completion fast path. Detailed Rust errors
remain available from the generated machine's inherent `process_event` method.
Async callbacks are a separate concern. Orthogonal, composite, async-callback,
and temporary-context machines retain their shape-specific inherent processing
APIs.

| Shape | Main state API |
|---|---|
| Flat | `state()`, `is(...)`, `visit_current_state(...)`, `set_current_states(...)` with `TestingPolicy` |
| Orthogonal | `states()`, `state(region)`, `is_region(...)`, `set_current_states(...)` with `TestingPolicy` |
| Composite | Parent state methods plus typed child state, active, setter, visitor, and `set_current_states(...)` with `TestingPolicy` |

Call `initialize()` when the initial state's entry hook or anonymous completion
must run before the first external event. `process_event` automatically runs
completion stabilization after every handled event.

The machine owner can query the static event types reachable from its current
state with `visit_current_events`:

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
visitor API is intended for an actor or other owner to query its own machine;
it is not a general introspection facility and is not exposed through events or
messages. Payloads containing non-static borrowed lifetimes are not reported,
because `TypeId` requires a `'static` type.

Generated callbacks return `Result`. Without a custom error type, guards use
`Result<bool, ()>` and actions use `Result<(), ()>`. A final action targeting a
payload state returns that payload value instead.

### Policies

Generated machines take an optional policy bundle:

```rust
use sml::{sml, sml_policies, Logger, PolicyBundle, TestingPolicy};

struct Trace;
impl Logger for Trace {}

sml_policies!(TestPolicies {
    logger: Trace,
    testing: TestingPolicy,
});

// The default constructor remains the zero-cost NoPolicy path. A custom
// bundle is supplied explicitly:
let machine = MyStateMachine::new_with_policy(
    context,
    TestPolicies::new(
        Trace,
        (),
        (),
        TestingPolicy,
        sml::DefaultQueuePolicy,
        sml::DefaultQueuePolicy,
    ),
);
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

`TestingPolicy` opts a machine into the `set_current_states` helper;
production `NoPolicy` machines do not expose that state mutation method. The
capability marker is sealed, so a custom production policy cannot opt itself
into state mutation. Flat machines pass one state; orthogonal machines pass
one state per region.
`PolicyBundle` also selects the defer and process queue factories, allowing a
bounded ring or another user-owned allocation-free container to implement
`Queue<E>`.

These policies are for the machine's owner to configure and query its own
machine. They are not a general introspection or control channel and must not
be made reachable through an event, message, or other domain input.

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
- [1.x stability policy](docs/stability.md)

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
Scheduler queueing and suspended RTC completion require a separate benchmark
when that adapter is implemented.

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
flat byte-array loop. Rust's allocation counter covers the timed path.

```bash
python3 benchmarks/compare_sm_pool.py --runs 21
```

The 2026-07-11 rotated native-release run produced:

| Path | Local | Random | Timed allocations |
|---|---:|---:|---:|
| Rust flat array | 0.312 ns/event | 0.335 ns/event | 0 |
| C++ flat array | 0.275 ns/event | 0.282 ns/event | setup only |
| Rust `SmPool` scalar | 0.421 ns/event | 0.430 ns/event | 0 |
| C++ `sm_pool` scalar | 0.629 ns/event | 0.680 ns/event | setup only |
| Rust `SmPool` batch | 0.362 ns/event | 0.370 ns/event | 0 |
| C++ `sm_pool` batch | 0.474 ns/event | 0.478 ns/event | setup only |

Rust batch dispatch was 23.6% faster locally and 22.6% faster under random
access than C++ `sm_pool`. It stayed within 16.0% of Rust's flat-array local
baseline and 10.4% of its random baseline. The compact byte state isolates pool
dispatch and access locality from application-specific work.

### Async, allocator, and worker-pool comparison

The extended runner compares the same 11-million-event player sequence through
Rust futures and C++ `co_sm` allocator policies. It also compares bounded,
persistent eight-worker fork/join schedulers over 5,000 rounds. Rust's global
allocation counter verifies that every timed Rust loop performs zero heap
allocations after setup.

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

| Workload | Median | Timed allocations | Completed runs |
|---|---:|---:|---:|
| Rust async façade over synchronous actions | 0.361 ns/event | 0 | 21/21 |
| C++ inline `co_sm` | 1.982 ns/event | inline fast path | 21/21 |
| Rust machine with native async callbacks | 3.372 ns/event | 0 | 21/21 |
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
must be read together with that reliability result.

## Development

<!-- enforced quality commands from scripts/quality_gates.sh and .github/workflows/*.yml -->

The same gate runs locally and on every push and pull request:

```bash
./scripts/quality_gates.sh
```

It enforces formatting, warning-free Clippy across every target and feature,
the full feature matrix, rustdoc warnings, documentation links, Python harness
syntax, package construction, dependency advisories and licenses, at least 90%
runtime line coverage, and 100% runtime function coverage. The compile-time
procedural-macro implementation is excluded from the runtime coverage report;
its unit tests remain part of the workspace test gates. Separate required jobs
run the suite on Linux, macOS, and Windows, enforce public API compatibility,
execute AddressSanitizer and Miri, and fuzz the runtime utilities.

Run the fuzz target locally with a nightly toolchain and `cargo-fuzz`:

```bash
cargo fuzz run runtime_utilities
```

The crate is licensed under either Apache-2.0 or MIT.

Release maintainers should follow the [release runbook](docs/releasing.md).
