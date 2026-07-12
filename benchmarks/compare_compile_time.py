#!/usr/bin/env python3
"""Compare clean production builds of the equivalent Rust and C++ player."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]


def first_line(command: list[str]) -> str:
    return subprocess.check_output(command, text=True).splitlines()[0]


def timed(command: list[str], *, env: dict[str, str] | None = None) -> int:
    start = time.perf_counter_ns()
    result = subprocess.run(
        command,
        cwd=ROOT,
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"command failed ({result.returncode}): {' '.join(command)}\n{result.stderr}"
        )
    return time.perf_counter_ns() - start


def rust_build(output_root: Path) -> tuple[int, int, int]:
    consumer = output_root / "consumer"
    source = consumer / "src"
    source.mkdir(parents=True)
    shutil.copyfile(ROOT / "examples/player_benchmark.rs", source / "main.rs")
    manifest = consumer / "Cargo.toml"
    manifest.write_text(
        "\n".join(
            [
                "[package]",
                'name = "compile-time-player"',
                'version = "0.0.0"',
                'edition = "2021"',
                "",
                "[dependencies]",
                f'sml = {{ package = "stateforward-sml", path = {json.dumps(str(ROOT))} }}',
                "",
                "[profile.release]",
                "codegen-units = 1",
                "lto = true",
                "",
            ]
        )
    )
    env = os.environ.copy()
    env["RUSTFLAGS"] = "-C target-cpu=native"
    subprocess.run(
        ["cargo", "generate-lockfile", "--offline", "--manifest-path", str(manifest)],
        cwd=ROOT,
        env=env,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    command = [
        "cargo",
        "build",
        "--offline",
        "--locked",
        "--release",
        "--manifest-path",
        str(manifest),
    ]
    clean = timed(command, env=env)
    with (source / "main.rs").open("a") as source_file:
        source_file.write("\n")
    rebuild = timed(command, env=env)
    binary = consumer / "target" / "release" / "compile-time-player"
    return clean, rebuild, binary.stat().st_size


def cpp_build(output_root: Path, cpp_dir: Path) -> tuple[int, int, int]:
    binary = output_root / "player_cpp"
    source = output_root / "player_cpp.cpp"
    shutil.copyfile(ROOT / "benchmarks/player_cpp.cpp", source)
    command = [
        "clang++",
        "-std=c++20",
        "-O3",
        "-DNDEBUG",
        "-march=native",
        f"-I{cpp_dir / 'include'}",
        f"-I{cpp_dir / 'benchmark/simple'}",
        str(source),
        "-o",
        str(binary),
    ]
    clean = timed(command)
    with source.open("a") as source_file:
        source_file.write("\n")
    rebuild = timed(command)
    return clean, rebuild, binary.stat().st_size


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Measure clean release builds of equivalent player programs."
    )
    parser.add_argument("--runs", type=int, default=7)
    parser.add_argument("--cpp-dir", type=Path, default=ROOT.parent / "sml.cpp")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be at least 1")

    cpp_dir = args.cpp_dir.resolve()
    required = [
        cpp_dir / "include/boost/sml.hpp",
        cpp_dir / "benchmark/simple/sml_player_sm.hpp",
    ]
    missing = [str(path) for path in required if not path.is_file()]
    if missing:
        parser.error(f"missing sml.cpp inputs: {', '.join(missing)}")

    print(f"host: {platform.platform()}")
    print(f"rust: {first_line(['rustc', '--version'])}")
    print(f"cargo: {first_line(['cargo', '--version'])}")
    print(f"c++: {first_line(['clang++', '--version'])}")
    print("mode: clean native release build and link; dependency sources cached")

    clean_durations: dict[str, list[int]] = {"rust": [], "cpp": []}
    rebuild_durations: dict[str, list[int]] = {"rust": [], "cpp": []}
    sizes: dict[str, list[int]] = {"rust": [], "cpp": []}
    builders = {
        "rust": lambda root: rust_build(root),
        "cpp": lambda root: cpp_build(root, cpp_dir),
    }

    with tempfile.TemporaryDirectory(prefix="sml-compile-time-") as temporary:
        base = Path(temporary)
        for cycle in range(args.runs):
            order = ["rust", "cpp"] if cycle % 2 == 0 else ["cpp", "rust"]
            for language in order:
                output_root = base / f"{cycle}-{language}"
                output_root.mkdir()
                clean, rebuild, size = builders[language](output_root)
                clean_durations[language].append(clean)
                rebuild_durations[language].append(rebuild)
                sizes[language].append(size)
                print(
                    f"{cycle + 1:02d} {language:4} "
                    f"clean {clean / 1_000_000_000:7.3f} s  "
                    f"rebuild {rebuild / 1_000_000_000:7.3f} s  "
                    f"{size / 1024:8.1f} KiB"
                )
                shutil.rmtree(output_root)

    rust_clean = statistics.median(clean_durations["rust"])
    cpp_clean = statistics.median(clean_durations["cpp"])
    rust_rebuild = statistics.median(rebuild_durations["rust"])
    cpp_rebuild = statistics.median(rebuild_durations["cpp"])
    print("\nmedian results")
    for language in ("rust", "cpp"):
        clean = statistics.median(clean_durations[language])
        rebuild = statistics.median(rebuild_durations[language])
        size = statistics.median(sizes[language])
        print(
            f"{language:4} clean {clean / 1_000_000_000:7.3f} s  "
            f"rebuild {rebuild / 1_000_000_000:7.3f} s  "
            f"{size / 1024:8.1f} KiB"
        )
    print(f"Rust/C++ clean-build ratio: {rust_clean / cpp_clean:.2f}x")
    print(f"Rust/C++ edit-rebuild ratio: {rust_rebuild / cpp_rebuild:.2f}x")


if __name__ == "__main__":
    main()
