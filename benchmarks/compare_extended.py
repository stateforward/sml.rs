#!/usr/bin/env python3
"""Build and alternately measure async, allocator, and thread-pool workloads."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import statistics
import subprocess


ROOT = Path(__file__).resolve().parents[1]
RESULT = re.compile(r"^(\S+) (\d+) ns total; ([0-9.]+) ns/(event|task)(?:; (\d+) allocations)?$")


def run(
    command: list[str], *, env: dict[str, str] | None = None, timeout: float = 5.0
) -> str:
    return subprocess.check_output(
        command, cwd=ROOT, env=env, text=True, timeout=timeout
    ).strip()


def build(cpp_dir: Path, thread_pool_dir: Path) -> None:
    env = os.environ.copy()
    env["RUSTFLAGS"] = "-C target-cpu=native"
    subprocess.run(
        [
            "cargo", "build", "--release", "--example", "async_allocator_benchmark",
            "--example", "thread_pool_benchmark",
        ],
        cwd=ROOT,
        env=env,
        check=True,
    )
    common = ["clang++", "-std=c++20", "-O3", "-DNDEBUG", "-march=native"]
    subprocess.run(
        common
        + [
            f"-I{cpp_dir / 'include'}",
            f"-I{cpp_dir / 'benchmark/simple'}",
            "benchmarks/async_allocator_cpp.cpp",
            "-o", "/tmp/sml_cpp_async_allocator",
        ],
        cwd=ROOT,
        check=True,
    )
    subprocess.run(
        common
        + [
            "-pthread",
            f"-I{thread_pool_dir / 'include'}",
            "benchmarks/thread_pool_cpp.cpp",
            "-o", "/tmp/sml_cpp_thread_pool",
        ],
        cwd=ROOT,
        check=True,
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=21)
    parser.add_argument("--cpp-dir", type=Path, default=ROOT.parent / "sml.cpp")
    parser.add_argument("--thread-pool-cpp-dir", type=Path, required=True)
    args = parser.parse_args()
    build(args.cpp_dir.resolve(), args.thread_pool_cpp_dir.resolve())

    rust_async = ROOT / "target/release/examples/async_allocator_benchmark"
    rust_pool = ROOT / "target/release/examples/thread_pool_benchmark"
    workloads = [
        ("rust-wrapper", [str(rust_async), "wrapper"]),
        ("cpp-inline", ["/tmp/sml_cpp_async_allocator", "inline"]),
        ("rust-native", [str(rust_async), "native"]),
        ("cpp-pooled", ["/tmp/sml_cpp_async_allocator", "pooled"]),
        ("cpp-heap", ["/tmp/sml_cpp_async_allocator", "heap"]),
        ("rust-thread-pool", [str(rust_pool)]),
        ("cpp-thread-pool", ["/tmp/sml_cpp_thread_pool"]),
    ]
    values: dict[str, list[int]] = {name: [] for name, _ in workloads}
    timeouts: dict[str, int] = {name: 0 for name, _ in workloads}
    units: dict[str, str] = {}

    for cycle in range(args.runs):
        order = workloads[cycle % len(workloads) :] + workloads[: cycle % len(workloads)]
        if cycle % 2:
            order.reverse()
        for expected, command in order:
            try:
                output = run(command)
            except subprocess.TimeoutExpired:
                timeouts[expected] += 1
                print(f"{cycle + 1:02d} {expected} TIMEOUT")
                continue
            match = RESULT.fullmatch(output)
            if not match or match.group(1) != expected:
                raise RuntimeError(f"unexpected output from {command}: {output!r}")
            if expected.startswith("rust-") and match.group(5) not in (None, "0"):
                raise RuntimeError(f"timed Rust loop allocated: {output}")
            values[expected].append(int(match.group(2)))
            units[expected] = match.group(4)
            print(f"{cycle + 1:02d} {output}")

    print("\nmedian results")
    for name, _ in workloads:
        if not values[name]:
            print(f"{name:18} no completed runs ({timeouts[name]} timeouts)")
            continue
        median = statistics.median(values[name])
        operations = 11_000_000 if units[name] == "event" else 5_000 * 8
        reliability = f"{len(values[name])}/{args.runs} completed"
        print(
            f"{name:18} {median:12.0f} ns  {median / operations:9.3f} "
            f"ns/{units[name]}  {reliability}"
        )


if __name__ == "__main__":
    main()
