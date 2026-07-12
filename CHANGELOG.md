# Changelog

All notable changes to this project are documented here. This project follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Changed

- Upgrade official GitHub workflow actions to their current Node 24 releases.
- Document and enforce the crates.io API-token bootstrap required before
  trusted publishing can manage later releases.
- Exclude generated semver-check artifacts from release crate packaging.

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
