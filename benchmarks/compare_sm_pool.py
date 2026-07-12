#!/usr/bin/env python3
"""Build and compare matched Rust and C++ state-machine pools."""

import argparse
import os
import re
import statistics
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RESULT = re.compile(r"^(\S+) (\d+) ns total; ([0-9.]+) ns/event;")


def run(command: list[str]) -> str:
    return subprocess.run(command, cwd=ROOT, check=True, text=True, capture_output=True).stdout


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=21)
    parser.add_argument("--cpp-dir", type=Path, default=Path("../sml.cpp"))
    args = parser.parse_args()

    rust_env = os.environ.copy()
    rust_env["RUSTFLAGS"] = (rust_env.get("RUSTFLAGS", "") + " -C target-cpu=native").strip()
    subprocess.run(
        ["cargo", "build", "--release", "--example", "sm_pool_benchmark"],
        cwd=ROOT,
        check=True,
        env=rust_env,
    )
    subprocess.run(
        [
            "clang++", "-std=c++20", "-O3", "-DNDEBUG", "-march=native",
            "-Wall", "-Wextra", "-Werror", f"-I{args.cpp_dir.resolve() / 'include'}",
            "benchmarks/sm_pool_cpp.cpp", "-o", "/tmp/sml_cpp_sm_pool",
        ],
        cwd=ROOT,
        check=True,
    )

    rust = ROOT / "target/release/examples/sm_pool_benchmark"
    workloads = []
    for mode in ("direct-local", "direct-random"):
        workloads.extend(((f"rust-{mode}", [str(rust), mode]),
                          (f"cpp-{mode}", ["/tmp/sml_cpp_sm_pool", mode])))
    for mode in ("scalar-local", "scalar-random", "batch-local", "batch-random"):
        workloads.extend(((f"rust-pool-{mode}", [str(rust), mode]),
                          (f"cpp-pool-{mode}", ["/tmp/sml_cpp_sm_pool", mode])))

    samples: dict[str, list[float]] = {name: [] for name, _ in workloads}
    for cycle in range(args.runs):
        ordered = workloads if cycle % 2 == 0 else list(reversed(workloads))
        shift = cycle % len(ordered)
        for expected, command in ordered[shift:] + ordered[:shift]:
            output = run(command).strip()
            match = RESULT.match(output)
            if not match:
                raise RuntimeError(f"unexpected output: {output}")
            if match.group(1) != expected:
                raise RuntimeError(f"expected {expected}, got {match.group(1)}")
            if expected.startswith("rust-") and "0 allocations" not in output:
                raise RuntimeError(f"timed allocation detected: {output}")
            samples[expected].append(float(match.group(3)))

    print(f"median of {args.runs} rotated runs")
    for name, _ in workloads:
        print(f"{name:22} {statistics.median(samples[name]):9.3f} ns/event")


if __name__ == "__main__":
    main()
