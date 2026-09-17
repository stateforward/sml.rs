# Security policy

## Supported versions

The latest `1.x` release receives security fixes. Prereleases and versions
before 1.0 are unsupported after 1.0 is published.

## Reporting a vulnerability

Report vulnerabilities privately through GitHub's security-advisory interface:

<https://github.com/stateforward/sml.rs/security/advisories/new>

## Safety guarantees

The runtime and procedural-macro crates are `no_std`-capable and forbid unsafe
Rust in both crate source and Cargo targets. CI builds package targets with
warnings denied, runs strict Clippy and source checks, and exercises the
generated flat, orthogonal, and composite paths through tests, sanitizer/Miri,
fuzz, no-std, and wasm checks.

The default machine API requires exclusive `&mut self` access. A configured
`ThreadSafety` policy does not turn a machine into a shareable or `Send`/`Sync`
object; the owner must provide its own synchronization boundary. Generated
state queries are read-only, and policy hooks receive only the documented
borrowed state/event data. Custom policy implementations remain caller code
and must uphold their trait contracts.

Do not open a public issue for a suspected vulnerability. Include affected
versions, a minimal reproducer, impact, and any suggested mitigation. The
maintainers will acknowledge a report within seven days and coordinate a fix,
advisory, and release before public disclosure.
