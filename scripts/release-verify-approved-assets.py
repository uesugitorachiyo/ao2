#!/usr/bin/env python3
"""Verify staged AO2 release assets against one operator-approved manifest."""

from __future__ import annotations

import argparse
import errno
import hashlib
import os
from pathlib import Path
import re
import stat
import sys


SHA256_RE = re.compile(r"[0-9a-f]{64}")
MANIFEST_LINE_RE = re.compile(r"([0-9a-f]{64})  ([^\r\n]+)")
REQUIRED_ASSET_COUNT = 23


class VerificationError(Exception):
    """A non-secret approval verification failure."""


def fail(message: str) -> None:
    raise VerificationError(message)


def open_regular_non_symlink(path: Path, label: str) -> int:
    flags = os.O_RDONLY
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        if error.errno in (errno.ELOOP, errno.EMLINK):
            fail(f"{label} must be a regular non-symlink file: {path}")
        fail(f"{label} cannot be opened: {path}")
    try:
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            fail(f"{label} must be a regular non-symlink file: {path}")
        if not hasattr(os, "O_NOFOLLOW") and stat.S_ISLNK(os.lstat(path).st_mode):
            fail(f"{label} must be a regular non-symlink file: {path}")
    except Exception:
        os.close(descriptor)
        raise
    return descriptor


def read_regular_non_symlink(path: Path, label: str) -> bytes:
    descriptor = open_regular_non_symlink(path, label)
    with os.fdopen(descriptor, "rb", closefd=True) as handle:
        return handle.read()


def hash_regular_non_symlink(path: Path, label: str) -> str:
    descriptor = open_regular_non_symlink(path, label)
    digest = hashlib.sha256()
    with os.fdopen(descriptor, "rb", closefd=True) as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def require_basename(name: str, source: str) -> None:
    if (
        not name
        or name in (".", "..")
        or "\x00" in name
        or "/" in name
        or "\\" in name
        or Path(name).is_absolute()
        or Path(name).name != name
    ):
        fail(f"{source} contains unsafe asset name: {name}")


def parse_manifest(data: bytes) -> dict[str, str]:
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError:
        fail("approved manifest must be valid UTF-8")
    entries: dict[str, str] = {}
    for line_number, line in enumerate(text.splitlines(), start=1):
        match = MANIFEST_LINE_RE.fullmatch(line)
        if match is None:
            fail(f"approved manifest line {line_number} is malformed")
        digest, name = match.groups()
        require_basename(name, "approved manifest")
        if name in entries:
            fail(f"duplicate approved asset name: {name}")
        entries[name] = digest
    if not entries:
        fail("approved manifest is empty")
    return entries


def parse_publication_list(data: bytes) -> list[str]:
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError:
        fail("staged publication list must be valid UTF-8")
    names: list[str] = []
    seen: set[str] = set()
    for line_number, name in enumerate(text.splitlines(), start=1):
        if not name:
            fail(f"staged publication list line {line_number} is empty")
        require_basename(name, "staged publication list")
        if name in seen:
            fail(f"duplicate staged publication asset name: {name}")
        seen.add(name)
        names.append(name)
    if not names:
        fail("staged publication list is empty")
    return names


def verify(args: argparse.Namespace) -> str:
    expected_manifest_digest = args.manifest_sha256
    if SHA256_RE.fullmatch(expected_manifest_digest) is None:
        fail("expected manifest SHA-256 must be 64 lowercase hexadecimal characters")

    manifest_path = Path(args.manifest)
    manifest_data = read_regular_non_symlink(manifest_path, "approved manifest")
    observed_manifest_digest = hashlib.sha256(manifest_data).hexdigest()
    if observed_manifest_digest != expected_manifest_digest:
        fail(
            "approved manifest SHA-256 mismatch: "
            f"expected {expected_manifest_digest}, observed {observed_manifest_digest}"
        )
    approved = parse_manifest(manifest_data)

    publication_dir = Path(args.publication_dir)
    try:
        publication_dir_mode = os.lstat(publication_dir).st_mode
    except OSError:
        fail(f"staged publication directory is missing: {publication_dir}")
    if stat.S_ISLNK(publication_dir_mode) or not stat.S_ISDIR(publication_dir_mode):
        fail(f"staged publication directory must be a non-symlink directory: {publication_dir}")

    publication_list_path = Path(args.publication_list)
    publication_list_data = read_regular_non_symlink(
        publication_list_path, "staged publication list"
    )
    staged_names = parse_publication_list(publication_list_data)
    staged = set(staged_names)
    approved_names = set(approved)

    missing = sorted(approved_names - staged)
    if missing:
        fail(f"staged publication set is missing approved asset: {missing[0]}")
    extra = sorted(staged - approved_names)
    if extra:
        fail(f"staged publication set has extra asset: {extra[0]}")
    if len(approved) != REQUIRED_ASSET_COUNT:
        fail(
            "approved manifest asset count mismatch: "
            f"expected {REQUIRED_ASSET_COUNT}, observed {len(approved)}"
        )
    if len(staged_names) != REQUIRED_ASSET_COUNT:
        fail(
            "staged publication asset count mismatch: "
            f"expected {REQUIRED_ASSET_COUNT}, observed {len(staged_names)}"
        )

    for name, expected_asset_digest in approved.items():
        asset_path = publication_dir / name
        try:
            asset_mode = os.lstat(asset_path).st_mode
        except OSError:
            fail(f"approved staged asset is missing: {name}")
        if stat.S_ISLNK(asset_mode) or not stat.S_ISREG(asset_mode):
            fail(
                "approved staged asset must be a regular non-symlink file: "
                f"{name}"
            )
        observed_asset_digest = hash_regular_non_symlink(
            asset_path, "approved staged asset"
        )
        if observed_asset_digest != expected_asset_digest:
            fail(f"approved asset hash mismatch: {name}")

    return observed_manifest_digest


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--publication-dir", required=True)
    parser.add_argument("--publication-list", required=True)
    return parser.parse_args()


def main() -> int:
    try:
        args = parse_args()
        manifest_digest = verify(args)
    except VerificationError as error:
        print(f"approved asset verification failed: {error}", file=sys.stderr)
        return 1
    print(f"release_approved_asset_manifest_sha256={manifest_digest}")
    print(f"release_approved_asset_count={REQUIRED_ASSET_COUNT}")
    print("release_approved_assets=passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
