#!/usr/bin/env python3
"""Build, alternate, report, and optionally persist Machine RTC benchmarks."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess


ROOT = Path(__file__).resolve().parents[1]
RESULT = re.compile(r"^(\d+) ns total; ([0-9.]+) ns/event$")
EVENTS = 11_000_000
RUST_BUILD = ["cargo", "build", "--release", "--example", "player_benchmark"]


def output(command: list[str]) -> str:
    return subprocess.check_output(command, cwd=ROOT, text=True).strip()


def sha256_files(root: Path, files: list[Path]) -> str:
    digest = hashlib.sha256()
    for path in sorted(files):
        relative = path.relative_to(root).as_posix()
        digest.update(relative.encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def tracked_files(root: Path, patterns: list[str]) -> list[Path]:
    names = output(["git", "-C", str(root), "ls-files", *patterns]).splitlines()
    return [root / name for name in names]


def relevant_status(root: Path, patterns: list[str]) -> list[str]:
    status = subprocess.check_output(
        ["git", "-C", str(root), "status", "--short", "--", *patterns],
        cwd=ROOT,
        text=True,
    )
    return status.rstrip("\n").splitlines() if status else []


def build_commands(cpp_dir: Path) -> tuple[list[str], list[str]]:
    common = [
        "clang++",
        "-std=c++20",
        "-O3",
        "-DNDEBUG",
        "-march=native",
        f"-I{cpp_dir / 'include'}",
    ]
    return (
        common + ["benchmarks/player_cpp.cpp", "-o", "/tmp/sml_cpp_player"],
        common
        + [
            "benchmarks/co_sm_inline_cpp.cpp",
            "-o",
            "/tmp/sml_cpp_co_sm_inline",
        ],
    )


def build(cpp_dir: Path) -> tuple[list[str], list[str]]:
    env = os.environ.copy()
    env["RUSTFLAGS"] = "-C target-cpu=native"
    subprocess.run(RUST_BUILD, cwd=ROOT, env=env, check=True)
    cpp_sync_build, cpp_async_build = build_commands(cpp_dir)
    subprocess.run(cpp_sync_build, cwd=ROOT, check=True)
    subprocess.run(cpp_async_build, cwd=ROOT, check=True)
    return cpp_sync_build, cpp_async_build


def sanitize_command(command: list[str], cpp_dir: Path) -> list[str]:
    return [part.replace(str(ROOT), "${SML_RS}").replace(str(cpp_dir), "${SML_CPP}") for part in command]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=21)
    parser.add_argument("--cpp-dir", type=Path, default=ROOT.parent / "sml.cpp")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")

    cpp_dir = args.cpp_dir.resolve()
    cpp_sync_build, cpp_async_build = build(cpp_dir)
    rust = ROOT / "target/release/examples/player_benchmark"
    workloads = [
        ("rust-sync", [str(rust)]),
        ("cpp-sync", ["/tmp/sml_cpp_player"]),
        ("rust-async-ready", [str(rust), "async"]),
        ("cpp-co-sm-inline", ["/tmp/sml_cpp_co_sm_inline"]),
    ]
    samples: dict[str, list[int]] = {name: [] for name, _ in workloads}

    for cycle in range(args.runs):
        order = workloads[cycle % len(workloads) :] + workloads[: cycle % len(workloads)]
        if cycle % 2:
            order.reverse()
        for name, command in order:
            measured = output(command)
            match = RESULT.fullmatch(measured)
            if not match:
                raise RuntimeError(f"unexpected output from {name}: {measured!r}")
            elapsed = int(match.group(1))
            samples[name].append(elapsed)
            print(f"{cycle + 1:02d} {name:19} {measured}")

    medians = {name: int(statistics.median(values)) for name, values in samples.items()}
    print("\nmedian results")
    for name, _ in workloads:
        median = medians[name]
        print(f"{name:19} {median:12d} ns  {median / EVENTS:9.3f} ns/event")

    if args.output:
        rust_patterns = [
            "Cargo.toml",
            "Cargo.lock",
            "src",
            "macros/Cargo.toml",
            "macros/src",
            "examples/player_benchmark.rs",
        ]
        cpp_patterns = ["include"]
        benchmark_inputs = [
            ROOT / "benchmarks/compare_machine_rtc.py",
            ROOT / "benchmarks/player_cpp.cpp",
            ROOT / "benchmarks/co_sm_inline_cpp.cpp",
            ROOT / "benchmarks/sml_player_sm.hpp",
            ROOT / "examples/player_benchmark.rs",
        ]
        derived = {
            "sync_elapsed_time_reduction_percent": round(
                (1 - medians["rust-sync"] / medians["cpp-sync"]) * 100, 1
            ),
            "sync_throughput_increase_percent": round(
                (medians["cpp-sync"] / medians["rust-sync"] - 1) * 100, 1
            ),
            "async_ready_elapsed_time_reduction_percent": round(
                (1 - medians["rust-async-ready"] / medians["cpp-co-sm-inline"])
                * 100,
                1,
            ),
            "async_ready_throughput_increase_percent": round(
                (medians["cpp-co-sm-inline"] / medians["rust-async-ready"] - 1)
                * 100,
                1,
            ),
        }
        record = {
            "schema": 2,
            "workload": "player-11m-machine-rtc",
            "runs": args.runs,
            "events_per_run": EVENTS,
            "system": {
                "platform": platform.platform(),
                "machine": platform.machine(),
            },
            "toolchains": {
                "rustc": output(["rustc", "--version"]),
                "cargo": output(["cargo", "--version"]),
                "clang": output(["clang++", "--version"]).splitlines()[0],
            },
            "source_identity": {
                "sml_rs_head": output(["git", "rev-parse", "HEAD"]),
                "sml_rs_relevant_status": relevant_status(ROOT, rust_patterns),
                "sml_rs_compilation_tree_sha256": sha256_files(
                    ROOT, tracked_files(ROOT, rust_patterns)
                ),
                "benchmark_inputs_sha256": sha256_files(ROOT, benchmark_inputs),
                "sml_cpp_head": output(["git", "-C", str(cpp_dir), "rev-parse", "HEAD"]),
                "sml_cpp_include_status": relevant_status(cpp_dir, cpp_patterns),
                "sml_cpp_include_tree_sha256": sha256_files(
                    cpp_dir, tracked_files(cpp_dir, cpp_patterns)
                ),
            },
            "commands": {
                "rust_build_env": {"RUSTFLAGS": "-C target-cpu=native"},
                "rust_build": RUST_BUILD,
                "cpp_sync_build": sanitize_command(cpp_sync_build, cpp_dir),
                "cpp_async_build": sanitize_command(cpp_async_build, cpp_dir),
                "workloads": {
                    name: sanitize_command(command, cpp_dir)
                    for name, command in workloads
                },
            },
            "samples_ns": samples,
            "medians_ns": medians,
            "derived": derived,
        }
        destination = args.output.resolve()
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(json.dumps(record, indent=2) + "\n")
        print(f"\nwrote {destination}")


if __name__ == "__main__":
    main()
