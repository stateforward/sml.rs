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

clippy_base_flags=(
  -D warnings
  -D unsafe_code
  -D clippy::cargo
  -D clippy::pedantic
  -D clippy::nursery
  -D clippy::allow-attributes-without-reason
)

clippy_safety_flags=(
  -D clippy::arithmetic_side_effects
  -D clippy::as_conversions
  -D clippy::cast_possible_truncation
  -D clippy::cast_possible_wrap
  -D clippy::cast_precision_loss
  -D clippy::cast_sign_loss
  -D clippy::dbg_macro
  -D clippy::exit
  -D clippy::expect_used
  -D clippy::float_cmp
  -D clippy::float_arithmetic
  -D clippy::get_unwrap
  -D clippy::infinite_loop
  -D clippy::indexing_slicing
  -D clippy::integer_division
  -D clippy::integer_division_remainder_used
  -D clippy::large_stack_arrays
  -D clippy::let_underscore_must_use
  -D clippy::lossy_float_literal
  -D clippy::mem_forget
  -D clippy::mixed_read_write_in_expression
  -D clippy::modulo_arithmetic
  -D clippy::option_env_unwrap
  -D clippy::panicking_overflow_checks
  -D clippy::panic
  -D clippy::panic_in_result_fn
  -D clippy::rc_buffer
  -D clippy::rc_mutex
  -D clippy::string_slice
  -D clippy::todo
  -D clippy::unimplemented
  -D clippy::unreachable
  -D clippy::unseparated_literal_suffix
  -D clippy::transmute_ptr_to_ptr
  -D clippy::transmute_undefined_repr
  -D clippy::uninit_assumed_init
  -D clippy::unwrap_in_result
  -D clippy::unwrap_used
)

source_safety() {
  python3 - "${ROOT}" <<'PY'
import re
import sys
from pathlib import Path

unsafe_token = re.compile(r"(?<![A-Za-z0-9_])unsafe(?![A-Za-z0-9_])")
root = Path(sys.argv[1])
violations = []
for path in sorted(root.rglob("*.rs")):
    if {".git", "target"}.intersection(path.parts):
        continue
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
run_gate clippy cargo clippy --workspace --all-targets --all-features --locked -- \
  "${clippy_base_flags[@]}"
run_gate clippy-pedantic-runtime cargo clippy --package stateforward-sml --lib \
  --no-deps --all-features --locked -- \
  "${clippy_base_flags[@]}" "${clippy_safety_flags[@]}"
run_gate clippy-pedantic-macros cargo clippy --package stateforward-sml-macros --lib \
  --no-deps --all-features --locked -- \
  "${clippy_base_flags[@]}" "${clippy_safety_flags[@]}"

section rustc-strict
run_gate rustc-strict env \
  RUSTFLAGS="${RUSTFLAGS:-} -D warnings -D unsafe_code" \
  cargo check --workspace --all-targets --all-features --locked

section source-safety
run_gate source-token-scan source_safety
run_gate runtime-unsafe-code \
  cargo clippy --package stateforward-sml --lib --all-features --locked -- -D warnings -D unsafe_code
run_gate macro-unsafe-code \
  cargo clippy --package stateforward-sml-macros --lib --all-features --locked -- -D warnings -D unsafe_code

section wasm32
run_gate wasm32-no-std \
  cargo check --workspace --all-features --target wasm32-unknown-unknown --locked
run_gate wasm32-no-std-runtime \
  cargo check --package stateforward-sml --lib --no-default-features \
    --target wasm32-unknown-unknown --locked

section tests
run_gate tests-all-features cargo test --workspace --all-features --locked
run_gate tests-no-default-features cargo test --workspace --no-default-features --locked
run_gate example-tests cargo test --workspace --all-features --examples --locked

section auxiliary-harnesses
run_gate sanitizer-check cargo check --manifest-path sanitizer/Cargo.toml --locked
run_gate fuzz-check cargo check --manifest-path fuzz/Cargo.toml --locked
run_gate sanitizer-clippy cargo clippy --manifest-path sanitizer/Cargo.toml --locked -- \
  "${clippy_base_flags[@]}" "${clippy_safety_flags[@]}"
run_gate fuzz-clippy cargo clippy --manifest-path fuzz/Cargo.toml --locked -- \
  "${clippy_base_flags[@]}" "${clippy_safety_flags[@]}"

section documentation
run_gate documentation env RUSTDOCFLAGS="-D warnings" \
  cargo doc --workspace --all-features --no-deps --locked

section scripts
run_gate scripts env PYTHONPYCACHEPREFIX="${TMPDIR:-/tmp}/sml-python-cache" \
  python3 -m py_compile benchmarks/*.py

section dependency-policy
require cargo-deny
run_gate dependency-policy cargo deny check

section package
run_gate macro-package cargo package -p stateforward-sml-macros --allow-dirty --locked
# The runtime package depends on the macro package being published first, so a
# single-checkout dry run cannot resolve it from the registry. Validate the
# runtime package file set here; release automation publishes macros first.
run_gate runtime-package \
  cargo package -p stateforward-sml --allow-dirty --no-verify --list --locked

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
run_gate runtime-coverage-lines cargo llvm-cov --package stateforward-sml --all-features --locked \
  --fail-under-lines 90
run_gate runtime-coverage-functions \
  cargo llvm-cov --package stateforward-sml --all-features --locked \
    --fail-under-functions 100 --summary-only
run_gate macro-coverage-report \
  cargo llvm-cov --package stateforward-sml-macros --all-features --locked --summary-only

echo
if [ "$failures" -ne 0 ]; then
  printf '%s quality gate(s) failed.\n' "$failures" >&2
  exit 1
fi
echo "All quality gates passed."
