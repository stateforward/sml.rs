# sml.rs

`sml.rs` is a `no_std`, allocation-free state-machine library for Rust. Its
`sml!` procedural macro mirrors the [Boost.SML](https://boost-ext.github.io/sml/)
transition-table DSL closely enough that tables can usually move between
`sml.cpp` and Rust mechanically.

```text
source state + event [guard] / action = target state
```

The generated machine uses ordinary Rust enums, borrows event data, stores its
context by value, and has no runtime allocator or dynamic dispatch.

## Install

<!-- installation dependency matching Cargo.toml package name and version -->

```toml
[dependencies]
sml = "0.8"
```

The crate has no default features and works on `no_std` targets. Enable
`graphviz` only when build-time diagram generation is wanted:

```toml
sml = { version = "0.8", features = ["graphviz"] }
```

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

| Shape | Main state API |
|---|---|
| Flat | `state()`, `is(...)`, `set_state(...)`, `visit_current_state(...)` |
| Orthogonal | `states()`, `state(region)`, `is_region(...)` |
| Composite | Parent state methods plus typed child state, active, setter, and visitor methods |

Call `initialize()` when the initial state's entry hook or anonymous completion
must run before the first external event. `process_event` automatically runs
completion stabilization after every handled event.

Generated callbacks return `Result`. Without a custom error type, guards use
`Result<bool, ()>` and actions use `Result<(), ()>`. A final action targeting a
payload state returns that payload value instead.

## State and event data

`event<E>` accepts an owned Rust event at the public boundary and passes `&E`
to guards and actions. A flat `state<T>` becomes a payload-bearing state enum
variant. Initial payloads and actionless payload targets use `Default`; a
producing transition returns the destination payload from its final action.

This is the ownership-safe counterpart to `sml.cpp` callbacks that mutate a
destination state object. A machine has one callback error type; use a Rust
enum when several error variants need distinct exception routing.

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

With the `graphviz` feature enabled, compiling a flat `sml!` table renders
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

Rust-specific mappings—trait methods instead of inline lambdas, context fields
instead of type-based dependency injection, `Result` instead of thrown values,
and LLVM-selected dispatch lowering—are documented and tested there.

## Performance

<!-- benchmark workloads and runner under examples/*_benchmark.rs and benchmarks/ -->

The CD-player benchmark performs 11 million equivalent event dispatches and
uses a direct-address compiler barrier after every dispatch in both languages.

```bash
RUSTFLAGS="-C target-cpu=native" \
  cargo run --release --example player_benchmark

clang++ -std=c++20 -O3 -DNDEBUG -march=native \
  -I../sml.cpp/include -I../sml.cpp/benchmark/simple \
  benchmarks/player_cpp.cpp -o /tmp/sml_cpp_player
/tmp/sml_cpp_player
```

In 21 alternating native-release runs on 2026-07-11, `sml.rs` completed the
workload in a 2.248 ms median versus 3.277 ms for `sml.cpp`, a 31.4% throughput
advantage on the test machine. These sub-millisecond differences are sensitive
to scheduling and thermals, so compare repeated alternating runs locally.

### Tensor actor pool

The tensor-pool runner compares `SmPool<Vec<u8>>` with C++ `sm_pool` using the
same 10,000-actor, 50,000-event workload, identical local and seeded-random
indices, and 1,001 rounds per sample. Each language is also measured against
its own flat byte-array loop. Rust's allocation counter covers the timed path.

```bash
python3 benchmarks/compare_tensor_pool.py --runs 21
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
baseline and 10.4% of its random baseline. The benchmark uses compact byte
state to model tensor actors; real tensor payload layout and wavefront
scheduling require their own end-to-end benchmarks.

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
the full feature matrix, rustdoc warnings, Python harness syntax, package
construction, at least 90% whole-workspace line coverage, and 100% runtime
function coverage. Separate required jobs run the suite on Linux, macOS, and
Windows, execute AddressSanitizer and Miri, and fuzz the runtime utilities.

Run the fuzz target locally with a nightly toolchain and `cargo-fuzz`:

```bash
cargo fuzz run runtime_utilities
```

The crate is licensed under either Apache-2.0 or MIT.
