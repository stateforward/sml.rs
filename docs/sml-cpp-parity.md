# sml.cpp parity

This matrix tracks behavioral parity with the sibling `sml.cpp` implementation.
The primary table spelling is intentionally identical where Rust token syntax
allows it; callback bodies and ownership remain Rust-native.

| Capability | Status | Evidence and Rust spelling |
|---|---|---|
| Typed states and events | Supported | Generated `States` and `Events` enums |
| Guards, actions, and guard expressions | Supported | Boolean guard expressions, ordered action sequences, nested `eval`, and fallible callbacks |
| Entry and exit hooks | Supported | Sync and async hooks generated per state |
| Initial entry lifecycle | Supported | Generated `initialize` enters the active initial state before stabilization |
| Async processing | Supported | Async guards, actions, and entry/exit hooks |
| Coroutine state-machine wrapper | Rust-native | Generated async processing uses Rust futures directly; no separate `co_sm` wrapper is needed |
| Event and state data | Supported in all generators | Owned/borrowed events and inferred `state<T>` payloads in flat, orthogonal, parent, and child storage, including default initialization, lifecycle injection, completion, and sync/async producing actions |
| Internal and self transitions | Supported | `tests/cpp_dsl.rs` verifies that internal transitions skip lifecycle hooks while explicit self-transitions run them |
| Wildcard and multi-state input patterns | Supported | `tests/test.rs` covers runtime behavior and `tests/compile-fail` covers ordering and pattern errors |
| Logging and transition callbacks | Supported | `tests/sml_cpp_examples.rs::logging` verifies per-guard, per-action, and state-change callbacks |
| Custom errors and dependency context | Supported in all generators | Flat, orthogonal, and composite callbacks preserve concrete error types; mutable call-scoped contexts are reborrowed through lifecycle, child-first dispatch, bubbling, and completion chains |
| Unexpected event transitions | Supported | `unexpected_event<Event>` and `unexpected_event<_>`; specific handlers take priority |
| Exception/error transitions | Supported in all generators | Typed/wildcard `exception` rows handle sync/async callback failures in flat, orthogonal, parent, and child tables; hierarchical routing is child-first and typed rows infer the concrete Rust error |
| Completion transitions with origin event | Supported | Flat, orthogonal, and composite chains preserve typed origin data; structural machines borrow the live event without `Clone` |
| Anonymous transitions | Supported | `completion<_>` stabilizes after `initialize` and after every processed event, including async actions |
| Composite/nested state machines | Recursive trees supported natively | Adjacent tables form an arbitrary ownership tree with independent typed storage per node, deepest-active-first sync/async routing, bubbling, lifecycle/unexpected/exception handlers, queues/eval, payloads, typed node APIs, completion propagation, retained history, and scalar/orthogonal roots |
| Orthogonal regions | Supported throughout composite trees | Multiple `*` rows generate sync/async broadcast regions with payloads, lifecycle/unexpected/exception handlers, queues/eval, and anonymous/origin-aware stabilization; orthogonal roots and embedded region groups may own scalar or orthogonal descendants recursively |
| History states | Supported | Marking a child's initial state as `*"idle"_s(H)` opts it into retained shallow history; unmarked children reset on re-entry |
| Deferred/processed events | Supported natively in all generators | Exact `/ defer` and `/ process(Event {})` actions use allocation-free queues and post-transition iterative dispatch in sync/async flat, orthogonal, parent, and child tables; deferred owned structural payloads require `Clone` |
| Terminal state semantics | Supported | Flat/orthogonal/composite machines recognize `X`; parent completion can depend on child termination |
| Runtime event-ID dispatch table | Supported | Allocation-free `utility::DispatchTable` with checked O(1) routing; `tests/sdl_adapter.rs` covers the SDL-style orthogonal adapter |
| State-machine pool and batch dispatch | Supported | Storage-generic `utility::SmPool`, indexed events, reset, and batch APIs |
| Testing/state override API | Supported | `new_with_state` and `set_state` enable focused transition tests and restoration |
| Visitor/introspection API | Supported | Flat, orthogonal, and composite state queries/visitors operate without allocation |
| Dispatch policy selection | Rust-native | Generated enum matches let LLVM select jump tables/branches; no runtime policy is required |
| Diagram generation | Supported for flat tables | The `graphviz` feature renders `sml_<Machine>.svg` when `dot` is available and otherwise retains DOT under `OUT_DIR` |

## Performance invariant

Parity work must retain the CD-player dispatch advantage measured by
`examples/player_benchmark.rs`. The benchmark performs 11 million event
dispatches and uses the same direct-address compiler barrier as
`sml.cpp/benchmark/simple/sml_player_sm.hpp`.

On 2026-07-11, a fresh 21-run alternating native release comparison on the
same machine produced:

| Implementation | Median for 11M events | Median per event |
|---|---:|---:|
| sml.cpp | 3.277 ms | 0.298 ns |
| sml.rs | 2.248 ms | 0.204 ns |

The Rust implementation retained a 31.4% median throughput advantage.

### Tensor actor pool invariant

`benchmarks/compare_tensor_pool.py` compares the public Rust `SmPool` and C++
`sm_pool` APIs over identical compact storage, indices, and event counts. The
2026-07-11 medians from 21 rotated native-release runs were:

| Path | Local | Random |
|---|---:|---:|
| Rust flat array | 0.312 ns/event | 0.335 ns/event |
| C++ flat array | 0.275 ns/event | 0.282 ns/event |
| Rust `SmPool` scalar | 0.421 ns/event | 0.430 ns/event |
| C++ `sm_pool` scalar | 0.629 ns/event | 0.680 ns/event |
| Rust `SmPool` batch | 0.362 ns/event | 0.370 ns/event |
| C++ `sm_pool` batch | 0.474 ns/event | 0.478 ns/event |

The Rust batch path performed zero timed allocations, beat C++ `sm_pool` by
23.6% locally and 22.6% under random access, and remained within 16.0% and
10.4% of its corresponding flat-array baselines. Pool throughput at or above
C++ and zero steady-state allocations are cutover invariants for tensor-actor
workloads.

### Async and scheduler policies

`benchmarks/compare_extended.py` builds and alternates the Rust harnesses in
`examples/async_allocator_benchmark.rs` and
`examples/thread_pool_benchmark.rs` against the C++ policy harnesses in
`benchmarks/async_allocator_cpp.cpp` and `benchmarks/thread_pool_cpp.cpp`.

On 2026-07-11, 21 requested runs produced:

| Policy path | Median | Reliability |
|---|---:|---:|
| Rust stack-polled async façade | 0.361 ns/event | 21/21 |
| C++ inline `co_sm` | 1.982 ns/event | 21/21 |
| Rust native async callbacks | 3.372 ns/event | 21/21 |
| C++ pooled coroutine allocator | 21.506 ns/event | 21/21 |
| C++ heap coroutine allocator | 49.427 ns/event | 21/21 |
| Rust fixed-lane worker pool | 259.255 ns/task | 21/21 |
| C++ fixed-ring thread pool | 1,139.283 ns/task | 13/21 |

Both Rust timed paths reported zero allocations. The C++ allocator variants
force the coroutine-frame path; the inline policy intentionally bypasses frame
allocation. The pool topologies differ and therefore measure policy tradeoffs,
not a like-for-like language primitive. The C++ completed-run median excludes
eight five-second timeouts, which are retained as part of the result.

## Final verification

<!-- enforced quality commands from scripts/quality_gates.sh and .github/workflows/*.yml -->

The cutover audit and every subsequent push use the repository gate:

```bash
./scripts/quality_gates.sh
```

CI additionally requires Linux, macOS, and Windows tests, an instrumented
AddressSanitizer runtime harness, Miri, and a bounded libFuzzer run. Coverage fails below 90% workspace line
coverage or below 100% runtime function coverage; this matches the sibling
project's line threshold while making complete runtime API execution explicit.

An exact filename reconciliation found 25 upstream `example/*.cpp` programs
and the same 25 named modules in `tests/sml_cpp_examples.rs`, with no missing
or extra translation. Searches across non-generated sources found no
pre-cutover package reference or macro surface. Cargo metadata
resolves the workspace packages as `sml` and `sml-macros`, both pointing to
the `stateforward/sml.rs` repository.
