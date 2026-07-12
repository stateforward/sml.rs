#!/usr/bin/env python3
"""Normalize the pre-1.0 Cargo package identity for API comparison."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import tomllib


PACKAGE_NAME = "stateforward-sml"
LIBRARY_NAME = "sml"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("baseline", type=Path)
    args = parser.parse_args()

    manifest = args.baseline / "Cargo.toml"
    text = manifest.read_text()
    metadata = tomllib.loads(text)
    old_package = metadata["package"]["name"]
    if old_package != PACKAGE_NAME:
        text, replacements = re.subn(
            rf'(?m)^name = "{re.escape(old_package)}"$',
            f'name = "{PACKAGE_NAME}"',
            text,
            count=1,
        )
        if replacements != 1:
            raise SystemExit(f"could not rename baseline package {old_package!r}")

    metadata = tomllib.loads(text)
    if "lib" not in metadata:
        text += f'\n[lib]\nname = "{LIBRARY_NAME}"\n'
    elif metadata["lib"].get("name", LIBRARY_NAME) != LIBRARY_NAME:
        raise SystemExit("baseline library target is not `sml`")

    manifest.write_text(text)


if __name__ == "__main__":
    main()
