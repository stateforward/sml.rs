# Migrating from 0.8 to 1.0

Version 1.0 completes the cutover to the `sml.rs` implementation and its
C++-shaped transition-table DSL. The crates.io packages are
`stateforward-sml` and `stateforward-sml-macros`. Rename the runtime dependency
to `sml` in `Cargo.toml` to retain the documented `sml::` import path:

```toml
sml = { package = "stateforward-sml", version = "1" }
```

## Toolchain

The minimum supported Rust version is 1.90. Update older toolchains before
upgrading the crate.

## Machine declarations

Existing named `sml!` declarations remain available. New code should prefer
the C++-shaped table syntax documented in `docs/dsl.md`:

```rust
sml! {
    Player {
        *"idle"_s + event<Start> [ready] / begin = "running"_s,
         "running"_s + event<Stop> / finish = X,
    }
}
```

The generated machine, context trait, state enum, event enum, and error type
are named from the table. Call `initialize()` when initial entry or anonymous
completion must run before the first external event.

## Configuration changes

- Replace removed `derive_states` and `derive_events` configuration with
  `states_attr` and `events_attr`.
- Guard callbacks return `Result<bool, Error>`.
- Action callbacks return `Result<(), Error>`, or the destination payload from
  the final action of a payload-producing transition.
- Use one concrete callback error type. Model multiple failure variants with a
  Rust enum and route them through typed or wildcard `exception` rows.
- Async guards, actions, and lifecycle hooks produce native Rust futures. No
  separate coroutine wrapper is required.

## Structural machines and utilities

Composite and orthogonal tables now use native generated storage and typed
child APIs. Runtime-indexed applications can use `DispatchTable`, `SmPool`,
`EventQueue`, and `EventQueues` from `sml::utility` without allocation.

Review the [parity matrix](sml-cpp-parity.md) and the
[complete DSL guide](dsl.md) when translating Boost.SML tables.
