# Releasing sml.rs

Releases are produced from the protected `main` branch through
`.github/workflows/release.yml`. After the one-time 1.0 bootstrap, the workflow
publishes `stateforward-sml-macros` first, waits for the crates.io index,
publishes `stateforward-sml`, attests both crate archives, creates the annotated
`vX.Y.Z` tag, and creates the GitHub release.

## Bootstrap the first publication

crates.io does not allow a trusted publisher to create a new package. The
initial publication therefore requires a local crates.io API token with the
`publish-new` scope. From a clean checkout of the protected `main` branch:

```bash
cargo publish -p stateforward-sml-macros
cargo info stateforward-sml-macros@1.0.0
cargo publish -p stateforward-sml
```

Do not publish the runtime until `cargo info` can see the macro package. The
release workflow recognizes that both 1.0.0 packages already exist, packages
and attests the same sources, and then creates the tag and GitHub release. It
refuses to bootstrap an unpublished 1.0.0 or continue a partial publication.

## Configure trusted publishing

After both packages exist, configure this trusted publisher on each package:

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
4. For 1.0.0 only, perform the bootstrap publication and configure trusted
   publishing. For later versions, confirm trusted publishing remains active.
5. Run the `Release` workflow on `main` with the exact version, without a `v`
   prefix.
6. Verify both crates on crates.io, their docs.rs builds, the provenance
   attestations, the annotated tag, and the GitHub release assets.

The workflow refuses mismatched versions and existing GitHub releases. It
skips publication only when both crate versions already exist, which makes the
1.0 bootstrap resumable without weakening later releases. Publishing is
intentionally ordered because `stateforward-sml` depends on the same version
of `stateforward-sml-macros`.
