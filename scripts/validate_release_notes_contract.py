#!/usr/bin/env python3
"""Resolve and validate AO2 versioned stable release notes."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


def load_json(path: Path) -> dict:
    def reject_duplicates(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result

    value = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicates)
    if not isinstance(value, dict):
        raise ValueError("release-train root must be an object")
    return value


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-root", required=True, type=Path)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--version")
    source.add_argument("--train")
    parser.add_argument("--release-train", type=Path)
    args = parser.parse_args()

    version = args.version
    if args.train:
        if args.release_train is None:
            parser.error("--release-train is required with --train")
        manifest = load_json(args.release_train)
        try:
            version = manifest[args.train]["ao2"]["version"]
        except (KeyError, TypeError) as error:
            raise SystemExit(f"release-train version is missing: {error}") from error

    if not isinstance(version, str) or re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version) is None:
        raise SystemExit("release version must be a three-part numeric semantic version")

    release_root = args.release_root.resolve()
    notes = release_root / f"v{version}-stable.md"
    if notes.is_symlink() or not notes.is_file():
        raise SystemExit(f"versioned stable release notes are missing: {notes}")
    if not notes.read_text(encoding="utf-8").strip():
        raise SystemExit(f"versioned stable release notes are empty: {notes}")

    try:
        display = notes.relative_to(Path.cwd().resolve())
    except ValueError:
        display = notes
    print(display)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
