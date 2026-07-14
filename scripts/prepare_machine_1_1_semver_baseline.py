#!/usr/bin/env python3
"""Apply only the owner-authorized Machine<E> 1.0 to 1.1 API correction."""

from __future__ import annotations

import argparse
from pathlib import Path


OLD_TRAIT = """pub trait Machine<E> {
    /// Generated state enum.
    type State;
    /// Generated processing error.
    type Error;

    /// Processes one event.
    fn process(&mut self, event: E) -> Result<&Self::State, Self::Error>;
}
"""

NEW_TRAIT = """pub trait Machine<E> {
    /// Generated state enum.
    type State;

    /// Processes one event to run-to-completion and reports whether it was
    /// accepted.
    fn process_event(&mut self, event: E) -> bool;

    /// Processes one event to run-to-completion and returns its acceptance as a
    /// future. The default inline path does not allocate.
    #[inline]
    fn process_event_async(&mut self, event: E) -> impl core::future::Future<Output = bool> {
        core::future::ready(self.process_event(event))
    }
}
"""

OLD_UTILITY = "handled + usize::from(region.process(event.clone()).is_ok())"
NEW_UTILITY = "handled + usize::from(Machine::process_event(region, event.clone()))"


def replace_exact(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    if text.count(old) != 1:
        raise SystemExit(f"expected exactly one authorized pattern in {path}")
    path.write_text(text.replace(old, new, 1))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("baseline", type=Path)
    args = parser.parse_args()

    replace_exact(args.baseline / "src/lib.rs", OLD_TRAIT, NEW_TRAIT)
    replace_exact(args.baseline / "src/utility.rs", OLD_UTILITY, NEW_UTILITY)


if __name__ == "__main__":
    main()
