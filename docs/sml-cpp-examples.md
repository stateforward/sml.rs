# sml.cpp example audit

This audit compares each source under `../sml.cpp/example` with the primary
`sml!` surface. “Direct” means the transition table can be moved mechanically;
Rust callback bodies still live on the generated context trait instead of as
inline C++ lambdas.

| sml.cpp example | Status | Rust mapping and evidence |
|---|---|---|
| `hello_world` | Compiling translation | `tests/sml_cpp_examples.rs::hello_world` preserves the four-event guarded/action sequence and terminal assertion |
| `transitions` | Compiling translation | `tests/sml_cpp_examples.rs::transitions` checks anonymous, internal, external self, entry/exit, and terminal transitions |
| `events` | Compiling translation | `tests/sml_cpp_examples.rs::events` checks `event<T>`, named `"event"_e`, guard payloads, and action payload borrowing |
| `actions_guards` | Compiling translation | `tests/sml_cpp_examples.rs::actions_guards` preserves conjunction, negation, disjunction, ordered actions, and context callbacks |
| `states` | Compiling translation | `tests/sml_cpp_examples.rs::states` checks string and typed state identities plus entry/exit lifecycle rows |
| `composite` | Compiling translation | `tests/sml_cpp_examples.rs::composite` uses adjacent tables and checks native `state<Sub>`, child-first routing, child state, and parent exit |
| `history` | Compiling translation | `tests/sml_cpp_examples.rs::history` exits and re-enters a native child and proves `(H)` retains its active substate |
| `orthogonal_regions` | Compiling translation | `tests/sml_cpp_examples.rs::orthogonal_regions` preserves both regions and proves event broadcast through joint termination |
| `defer_and_process` | Compiling translation | `tests/sml_cpp_examples.rs::defer_and_process` preserves deferred replay and post-transition processed-event dispatch with bounded allocation-free queues |
| `testing` | Compiling translation | `tests/sml_cpp_examples.rs::testing` uses `set_state` to isolate the transition under test and verifies context mutation |
| `visitor` | Compiling translation | `tests/sml_cpp_examples.rs::visitor` visits generated typed states before and after transitions |
| `logging` | Compiling translation | `tests/sml_cpp_examples.rs::logging` proves every guard leaf, action, and state change invokes its context logging hook |
| `dependencies` | Compiling Rust-native translation | `tests/sml_cpp_examples.rs::dependencies` stores the injected dependency in context and combines it with borrowed event payloads |
| `dependency_injection` | Compiling Rust-native translation | `tests/sml_cpp_examples.rs::dependency_injection` injects values through context and lets a borrowing controller own the dispatch workflow |
| `dispatch_policy` | Compiling Rust-native translation | `tests/sml_cpp_examples.rs::dispatch_policy` runs the policy workload; generated enum matching deliberately leaves branch/jump-table selection to LLVM |
| `dispatch_table` | Compiling translation | `tests/sml_cpp_examples.rs::dispatch_table` routes contiguous runtime IDs through allocation-free `utility::DispatchTable` |
| `sdl2` | Compiling ownership-safe adapter | `tests/sml_cpp_examples.rs::sdl2` translates the two-region table and dispatches SDL-style runtime IDs into borrowed typed wrappers |
| `plant_uml` | Compiling platform translation | `tests/sml_cpp_examples.rs::plant_uml` compiles the source table with the `graphviz` feature, which emits DOT/SVG rather than PlantUML text |
| `arduino` | Compiling ownership-safe translation | `tests/sml_cpp_examples.rs::arduino` models button input and LED output without allocation; the repository-wide no-default-features gate proves `no_std` generation |
| `in_place` | Compiling translation | `tests/sml_cpp_examples.rs::in_place` directly constructs and runs the single-row generated machine |
| `nested` | Compiling Rust-native translation | `tests/sml_cpp_examples.rs::nested` stores a generated machine directly inside an owning controller type |
| `data` | Compiling ownership-safe translation | `tests/sml_cpp_examples.rs::data` preserves per-state IDs through producing actions that return destination payload values |
| `error_handling` | Compiling ownership-safe translation | `tests/sml_cpp_examples.rs::error_handling` maps thrown values to a Rust error enum and checks guarded typed/wildcard exception recovery plus specific unexpected-event routing |
| `eval` | Compiling translation | `tests/sml_cpp_examples.rs::eval` preserves ordered `action, eval [guard] / action, action` execution and verifies all three mutations |
| `euml_emulation` | Compiling translation | `tests/sml_cpp_examples.rs::euml_emulation` preserves the typed-event/state sequence with named Rust guard/action methods replacing callable objects |

## Combination coverage

- Origin-aware `completion<Event>` is implemented for flat, orthogonal, and
  composite machines, including child-to-parent terminal completion chains.
- State payloads, including inferred `state<T>`, are implemented in flat,
  orthogonal, parent, and child storage with sync/async producing actions.
- Composite expansion supports arbitrary recursive child trees with independent
  typed storage, deepest-active-first routing, and full callback features;
  scalar and orthogonal nodes may alternate at any tested tree depth.
- Custom callback errors and mutable temporary contexts are supported by flat,
  orthogonal, and composite tables, including lifecycle and completion chains.
