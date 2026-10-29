# Changelog

All notable changes to this project are documented here. This project follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- Implement `Machine<Events>` for synchronous composite machines (a single
  child or a tree) without a temporary context or an orthogonal node, with
  `State` set to the parent states enum.

### Changed

- Apply a composite parent table's `states_attr` and `events_attr` to the
  generated parent states and events enums in single-child composites, as
  composite trees already did; they were silently ignored before. A
  single-child composite table that named a derive its payload types lack, or
  a `PartialEq` derive in `states_attr`, now fails to compile.
- A derived `PartialEq` in a flat table's `events_attr` replaces the generated
  variant-only event comparison (issue #21) instead of conflicting with it, so
  a journaled `Events` value compares by payload. States keep the variant-only
  comparison.
- A downstream crate that implemented `sml::Machine` for a generated composite
  machine itself now conflicts with the generated implementation; remove its
  own.

### Fixed

- Route guard and action faults raised by `completion<E>` and anonymous
  `completion<_>` rows in composite and orthogonal machines to `exception<E>`
  rows, innermost active state first, as event dispatch already did; such a
  fault no longer escapes `process_event` and leaves a child mid-flow.
- End the run-to-completion step after a completion fault is recovered by an
  internal or self-targeting `exception<E>` row, instead of rerunning the
  faulting completion forever in `process_event` or `initialize`; stabilizing
  continues only when the handling row moves to another state. In a machine
  with orthogonal regions such a recovery settles only the region that
  faulted, so peer regions still run their pending completions.
- Exit a state once per entry in flat, composite, and orthogonal machines
  when a transition's action fails after its source's `on_exit` already ran
  and an `exception<E>` row's recovery then leaves that state; the recovery
  no longer runs the same `on_exit` a second time.
- Enter and exit the composite child active in an orthogonal region when its
  orthogonal node or root is entered (including `initialize`) or left, and
  clear a state's exit-skip token on every entry, so `on_entry` and `on_exit`
  stay paired across re-entry after a recovered fault.
- Settle an orthogonal region whose completion fault was recovered without
  progress even when an exception row in a peer region (or in a composite
  child active in a peer region) moved that peer; only a row on the faulting
  region's own path counts as progress, so a moving peer can no longer rerun
  the faulting completion or loop forever. A settled region is settled only
  for its current occupancy: when a recovery's moving row leaves it (and
  re-enters it, or moves it to another state) within the same step, its
  completions run again.
- Keep each region's or nested child's completed exit recorded while an
  orthogonal node or composite subtree is torn down, so when a later exit
  fails and an ancestor's `exception<E>` row repeats the teardown, the states
  that already exited are not exited again.
- Borrow a composite child's state data per row, so several rows on one
  trigger from a data-carrying state (`Name(T)`) can all read that data, as in
  a flat table, instead of failing borrow checking (E0502).
- Keep thread-safety guards in a separate generated storage slot so policy
  logging and event processing use ordinary safe Rust borrowing with no raw
  pointer or unsafe-code path.
- Preserve context process-event callbacks and report policy actions only after
  the action succeeds.
- Seal the testing-only state override capability so production policies cannot
  opt themselves into `set_current_states`.
- Seal the generated policy-composition traits so custom slots cannot replace
  the library-owned policy storage and lock-separation boundary.

## 1.4.0 - 2026-09-04

### Added

- Add composable logger, observer, dispatch, thread-safety, testing, and queue
  policies for generated machines.
- Add static generated event/state names and owner-only policy accessors.
- Add allocation-free `visit_current_events` queries for static event payload
  types from the active flat, composite, and orthogonal states.

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
