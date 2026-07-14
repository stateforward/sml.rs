#!/usr/bin/env python3
"""Create a checker-visible unrelated break to prove the semver gate fails."""

from __future__ import annotations

import argparse
from pathlib import Path


PUBLIC_STORAGE_ACCESSOR = """    /// Returns the underlying storage.
    pub fn storage(&self) -> &S {
        &self.storage
    }
"""


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("checkout", type=Path)
    args = parser.parse_args()

    utility = args.checkout / "src/utility.rs"
    text = utility.read_text()
    if text.count(PUBLIC_STORAGE_ACCESSOR) != 1:
        raise SystemExit("expected exactly one public SmPool::storage accessor")
    utility.write_text(text.replace(PUBLIC_STORAGE_ACCESSOR, "", 1))


if __name__ == "__main__":
    main()
