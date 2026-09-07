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

failures=0

source_safety() {
  python3 - "${ROOT}/src" "${ROOT}/macros" <<'PY'
import re
import sys
from pathlib import Path

unsafe_token = re.compile(r"(?<![A-Za-z0-9_])unsafe(?![A-Za-z0-9_])")
violations = []
for root_name in sys.argv[1:]:
    root = Path(root_name)
    if not root.is_dir():
        violations.append(f"{root}: source directory is missing")
        continue
    for path in sorted(root.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for line_number, line in enumerate(text.splitlines(), 1):
            if unsafe_token.search(line):
                violations.append(f"{path}:{line_number}:{line.strip()}")

if violations:
    print("Unsafe Rust token found:", file=sys.stderr)
    print("\n".join(violations), file=sys.stderr)
    raise SystemExit(1)
PY
}

run_gate() {
  local name="$1"
  shift
  if "$@"; then
    return 0
  fi
  printf 'Gate failed: %s\n' "$name" >&2
  failures=$((failures + 1))
}

section format
run_gate format cargo fmt --all -- --check

section clippy
run_gate clippy cargo clippy --workspace --all-targets --all-features -- \
  -D warnings -D unsafe_code

section rustc-strict
run_gate rustc-strict env \
  RUSTFLAGS="${RUSTFLAGS:-} -D warnings -D unsafe_code" \
  cargo check --workspace --all-targets --all-features

section source-safety
run_gate source-token-scan source_safety
run_gate runtime-unsafe-code \
  cargo clippy --package stateforward-sml --lib --all-features -- -D warnings -D unsafe_code
run_gate macro-unsafe-code \
  cargo clippy --package stateforward-sml-macros --lib --all-features -- -D warnings -D unsafe_code

section wasm32
run_gate wasm32-no-std \
  cargo check --workspace --all-features --target wasm32-unknown-unknown
run_gate wasm32-no-std-runtime \
  cargo check --package stateforward-sml --lib --no-default-features \
    --target wasm32-unknown-unknown

section tests
run_gate tests-all-features cargo test --workspace --all-features
run_gate tests-no-default-features cargo test --workspace --no-default-features
run_gate example-tests cargo test --workspace --all-features --examples

section auxiliary-harnesses
run_gate sanitizer-check cargo check --manifest-path sanitizer/Cargo.toml
run_gate fuzz-check cargo check --manifest-path fuzz/Cargo.toml

section documentation
run_gate documentation env RUSTDOCFLAGS="-D warnings" \
  cargo doc --workspace --all-features --no-deps

section scripts
run_gate scripts env PYTHONPYCACHEPREFIX="${TMPDIR:-/tmp}/sml-python-cache" \
  python3 -m py_compile benchmarks/*.py

section dependency-policy
require cargo-deny
run_gate dependency-policy cargo deny check

section package
run_gate macro-package cargo package -p stateforward-sml-macros --allow-dirty
# The runtime package depends on the macro package being published first, so a
# single-checkout dry run cannot resolve it from the registry. Validate the
# runtime package file set here; release automation publishes macros first.
run_gate runtime-package \
  cargo package -p stateforward-sml --allow-dirty --no-verify --list

section coverage
require cargo-llvm-cov
# trybuild owns a nested target directory outside cargo-llvm-cov's cleanup.
# It may omit Cargo's CACHEDIR.TAG, so remove only that generated directory.
trybuild_dir="${ROOT}/target/tests/trybuild"
if [ -d "${trybuild_dir}" ]; then
  quarantine_dir="$(mktemp -d "${TMPDIR:-/tmp}/sml-trybuild.XXXXXX")"
  rmdir -- "${quarantine_dir}"
  if ! mv -- "${trybuild_dir}" "${quarantine_dir}"; then
    echo "Unable to quarantine ${trybuild_dir}; coverage may include stale trybuild artifacts." >&2
    failures=$((failures + 1))
  elif ! rm -rf -- "${quarantine_dir}"; then
    echo "Unable to remove quarantined trybuild artifacts at ${quarantine_dir}." >&2
    failures=$((failures + 1))
  fi
fi
# Runtime coverage is the authoritative coverage measurement. Macro expansion
# tests exercise the procedural-macro generators through the runtime crate, and
# the workspace test gates still execute the macro crate's own unit tests. The
# macro crate is reported separately because its implementation is compile-time
# code and cannot be measured as runtime library behavior.
run_gate runtime-coverage-lines cargo llvm-cov --package stateforward-sml --all-features \
  --fail-under-lines 90
run_gate runtime-coverage-functions \
  cargo llvm-cov --package stateforward-sml --all-features \
    --fail-under-functions 100 --summary-only
run_gate macro-coverage-report \
  cargo llvm-cov --package stateforward-sml-macros --all-features --summary-only

echo
if [ "$failures" -ne 0 ]; then
  printf '%s quality gate(s) failed.\n' "$failures" >&2
  exit 1
fi
echo "All quality gates passed."
