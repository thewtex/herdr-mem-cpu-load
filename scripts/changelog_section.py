#!/usr/bin/env python3
"""Print one release's section of CHANGELOG.md, for use as GitHub release notes.

    scripts/changelog_section.py 0.1.0

Matches the `## [0.1.0]` heading Keep a Changelog uses and prints everything up
to the next `## ` heading. A version with no section is not an error: the
release notes fall back to a one-line pointer at the changelog, so tagging is
never blocked by a missing entry.
"""

from __future__ import annotations

import sys
from pathlib import Path


def section(text: str, version: str) -> str | None:
    wanted = (f"## [{version}]", f"## {version}")
    lines = text.splitlines()
    body: list[str] | None = None

    for line in lines:
        if line.startswith("## "):
            if body is not None:
                break
            if line.startswith(wanted):
                body = []
            continue
        if body is not None:
            body.append(line)

    if body is None:
        return None
    return "\n".join(body).strip("\n")


def main() -> int:
    if len(sys.argv) != 2:
        sys.exit(f"usage: {Path(sys.argv[0]).name} <version>")
    version = sys.argv[1].lstrip("v")

    path = Path("CHANGELOG.md")
    text = path.read_text(encoding="utf-8") if path.exists() else ""
    body = section(text, version)

    if body:
        print(body)
    else:
        print(f"See [CHANGELOG.md](CHANGELOG.md) for the changes in {version}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
