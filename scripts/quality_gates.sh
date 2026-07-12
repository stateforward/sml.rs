#!/usr/bin/env bash

set -euo pipefail
IFS=$'\n\t'

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

section() {
  printf '\n[%s]\n' "$1"
}

require() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Required command '$1' is missing." >&2
    exit 1
  fi
}

require cargo
require python3

section format
cargo fmt --all -- --check

section clippy
cargo clippy --workspace --all-targets --all-features -- -D warnings

section tests
cargo test --workspace --all-features
cargo test --workspace --no-default-features
cargo test --workspace --all-features --examples

section documentation
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps

section scripts
PYTHONPYCACHEPREFIX="${TMPDIR:-/tmp}/sml-python-cache" \
  python3 -m py_compile benchmarks/*.py

section dependency-policy
require cargo-deny
cargo deny check

section package
cargo package -p stateforward-sml-macros --allow-dirty
# The runtime package depends on the macro package being published first, so a
# single-checkout dry run cannot resolve it from the registry. Validate the
# runtime package file set here; release automation publishes macros first.
cargo package -p stateforward-sml --allow-dirty --no-verify --list >/dev/null

section coverage
require cargo-llvm-cov
cargo llvm-cov --workspace --all-features --fail-under-lines 90
cargo llvm-cov --workspace --all-features --exclude stateforward-sml-macros \
  --fail-under-functions 100 --summary-only

echo
echo "All quality gates passed."
