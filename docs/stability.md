# Stability policy

Version 1.1 establishes the following compatibility contract for subsequent
`1.x` releases. Version 1.1 intentionally corrects the newly introduced
`Machine<E>` contract from 1.0 before known downstream adoption; see the
[1.1 migration guide](migrating-to-1.1.md).

## Stable

- Public items exported by `sml` and `sml::utility`.
- Accepted `sml!` table spellings documented in `docs/dsl.md`.
- Generated public machine, context, state, event, error, visitor, and typed
  child APIs documented in the README and DSL guide.
- Feature names and their documented behavior.
- `no_std` operation without default features and the Rust 1.90 MSRV.

Removing or incompatibly changing these requires a new major version. Every
pull request is checked against its protected base revision with
`cargo-semver-checks`; generated API and DSL behavior also remain covered by
compile-pass, compile-fail, and runtime tests.

The compatibility workflow contains one exact exception for the owner-approved
1.0.0 to 1.1.0 `Machine<E>` correction. It synthesizes a baseline containing
only that authorized trait delta, applies patch-level enforcement to every
other public API, and verifies that an unrelated public removal still fails.

## Not covered by semantic versioning

- Exact compiler diagnostics and source locations.
- Private generated identifiers and macro implementation details.
- Diagram formatting beyond valid DOT/SVG output.
- Benchmark numbers, which vary by compiler, processor, and operating system.
- Files under `benchmarks/`, `fuzz/`, `sanitizer/`, and `scripts/` as reusable
  library APIs.

Security fixes may reject previously accepted unsound behavior. MSRV increases
are announced in the changelog and require a minor release during the 1.x
series.
