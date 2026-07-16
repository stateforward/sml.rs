# Changelog

All notable changes to this project are documented here. This project follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

## 1.2.0 - 2026-07-16

### Added

- Add dispatch-scoped lifetime, type, and const generics for typed events in
  flat `sml!` tables, including propagated bounds and `where` clauses, direct
  mutable-borrow handoffs, generated event enums, inherent dispatch, and
  `Machine<E>` implementations without type erasure or allocation. Every
  external event must identify all declared type and const parameters; the
  macro diagnoses parameter subsets that cannot infer the single event family,
  rejects unused declarations, recursively collects event-specific lifetimes,
  keeps higher-ranked lifetimes local to their binders, propagates
  temporary-context generics to callbacks, filters callback bounds with omitted
  event-specific lifetimes, avoids unused generic initialization parameters,
  diagnoses mutable borrowed completion origins, reuses lifetimes shared with
  stored state, and rejects dispatch-only type or const parameters in state and
  typed-exception storage.

## 1.1.0 - 2026-07-14

### Breaking

- Replace the 1.0 `Machine::process` and `Machine::Error` contract with the
  `sml.cpp`-compatible acceptance interface described below. This is an
  owner-authorized pre-adoption API correction and is not source compatible
  with manual 1.0 `Machine` implementations.

### Added

- Add `Machine::process_event` and `Machine::process_event_async` acceptance APIs
  for generated synchronous flat machines and manual implementors. This
  replaces the previous fallible `Machine::process` contract; detailed errors
  remain on each generated machine's inherent `process_event` method.

### Changed

- Upgrade official GitHub workflow actions to their current Node 24 releases.
- Document and enforce the crates.io API-token bootstrap required before
  trusted publishing can manage later releases.
- Exclude generated `cargo-semver-checks` and baseline artifacts from release
  crate packaging.

## 1.0.0 - 2026-07-11

### Added

- Add a C++-shaped transition-table DSL with native flat, composite,
  orthogonal, completion, exception, history, deferred, and processed-event
  semantics.
- Add synchronous and asynchronous guards, actions, lifecycle callbacks, and
  dispatch APIs.
- Add allocation-free runtime queues, dispatch tables, hierarchical and
  orthogonal utilities, and `SmPool` indexed and batch dispatch.
- Add translations and behavioral tests for all 25 sibling `sml.cpp` examples.
- Add matched Rust and C++ performance harnesses for synchronous dispatch,
  asynchronous execution, allocator policies, state-machine pools, and worker
  pools.
- Add a complete DSL guide, parity audit, migration guide, stability policy,
  security policy, and release runbook.
- Add enforced formatting, linting, testing, documentation, link checking,
  package validation, dependency policy, public API compatibility, coverage,
  MSRV, cross-platform, AddressSanitizer, Miri, and fuzzing gates.
- Add trusted-publishing release automation with provenance attestations.

### Changed

- Set the minimum supported Rust version to 1.90.
- Upgrade `syn` to version 2.
- Replace `derive_states` and `derive_events` with generic `states_attr` and
  `events_attr` configuration.
- Replace `log_state_change` with the more flexible `transition_callback`.
- Rename `custom_guard_error` to `custom_error` because error customization
  applies beyond guards.
- Define on-exit and on-entry lifecycle ordering consistently across flat,
  composite, and orthogonal transitions.

The public repository starts its formal release history at 1.0.0. Earlier
development history remains available in Git.
