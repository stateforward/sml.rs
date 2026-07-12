# Releasing sml.rs

Releases are produced only from the protected `main` branch through
`.github/workflows/release.yml`. The workflow publishes
`stateforward-sml-macros` first, waits for the crates.io index, publishes
`stateforward-sml`, attests both crate archives, creates the annotated
`vX.Y.Z` tag, and creates the GitHub release.

## One-time crates.io setup

Both `stateforward-sml-macros` and `stateforward-sml` must authorize a
crates.io trusted publisher with these values:

| Field | Value |
|---|---|
| GitHub owner | `stateforward` |
| Repository | `sml.rs` |
| Workflow | `release.yml` |
| Environment | `release` |

Create a protected GitHub environment named `release` and require an approver.
No long-lived crates.io token is stored in GitHub.

After the first publication, assign both packages to the StateForward `sml`
team while retaining at least one named owner who can manage owners:

```bash
cargo owner --add github:stateforward:sml stateforward-sml-macros
cargo owner --add github:stateforward:sml stateforward-sml
```

## Release checklist

1. Confirm every required check on `main` is green.
2. Update both crate versions, their dependency edge, `CHANGELOG.md`, and the
   README installation examples in the same pull request.
3. Confirm `cargo deny check`, coverage, sanitizer, Miri, fuzz, platform, MSRV,
   package, and public API compatibility jobs pass.
4. Run the `Release` workflow on `main` with the exact version, without a `v`
   prefix.
5. Verify both crates on crates.io, their docs.rs builds, the provenance
   attestations, the annotated tag, and the GitHub release assets.

The workflow refuses mismatched versions and existing releases. Publishing is
intentionally ordered because `stateforward-sml` depends on the same version
of `stateforward-sml-macros`.
