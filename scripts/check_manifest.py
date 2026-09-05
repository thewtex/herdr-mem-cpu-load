#!/usr/bin/env python3
"""Validate herdr-plugin.toml and keep its version tied to Cargo.toml.

The marketplace indexes a repository by parsing every `herdr-plugin.toml` on
its default branch, so a manifest that does not parse, or that is missing a
required key, silently drops the plugin off the listing. A version that has
drifted from `Cargo.toml` is worse: the listing advertises one number and
`cargo install` builds another.

Run from the repository root, or with the root as the only argument.
"""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path

REQUIRED_KEYS = ("id", "name", "version", "min_herdr_version")


def load(path: Path) -> dict:
    try:
        with path.open("rb") as handle:
            return tomllib.load(handle)
    except FileNotFoundError:
        sys.exit(f"{path}: missing")
    except tomllib.TOMLDecodeError as error:
        sys.exit(f"{path}: not valid TOML: {error}")


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    manifest_path = root / "herdr-plugin.toml"
    cargo_path = root / "Cargo.toml"

    manifest = load(manifest_path)
    cargo = load(cargo_path)

    problems = [
        f"{manifest_path.name}: missing required key `{key}`"
        for key in REQUIRED_KEYS
        if not isinstance(manifest.get(key), str) or not manifest[key].strip()
    ]

    manifest_version = manifest.get("version")
    cargo_version = cargo.get("package", {}).get("version")
    if not cargo_version:
        problems.append(f"{cargo_path.name}: [package] has no version")
    elif manifest_version and manifest_version != cargo_version:
        problems.append(
            f"version drift: {manifest_path.name} says {manifest_version!r}, "
            f"{cargo_path.name} says {cargo_version!r} "
            f"(run scripts/release.sh to bump them together)"
        )

    if problems:
        for problem in problems:
            print(f"error: {problem}", file=sys.stderr)
        return 1

    print(
        f"{manifest_path.name} is valid: "
        + ", ".join(f"{key}={manifest[key]}" for key in REQUIRED_KEYS)
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
