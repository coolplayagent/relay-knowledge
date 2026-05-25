#!/usr/bin/env python3
"""Verify Linux GNU release binaries do not exceed the supported glibc ABI."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path
from typing import Callable


DEFAULT_MAX_GLIBC = "2.31"
GLIBC_VERSION_PATTERN = re.compile(r"\bGLIBC_(\d+)\.(\d+)(?:\.(\d+))?\b")


Version = tuple[int, int, int]
Runner = Callable[..., subprocess.CompletedProcess[str]]


def parse_version(value: str) -> Version:
    normalized = value.strip()
    if normalized.startswith("GLIBC_"):
        normalized = normalized.removeprefix("GLIBC_")
    parts = normalized.split(".")
    if len(parts) not in {2, 3} or not all(part.isdigit() for part in parts):
        raise ValueError(f"glibc version must look like 2.31 or GLIBC_2.31: {value}")
    major, minor = int(parts[0]), int(parts[1])
    patch = int(parts[2]) if len(parts) == 3 else 0
    return (major, minor, patch)


def format_version(version: Version) -> str:
    major, minor, patch = version
    if patch:
        return f"{major}.{minor}.{patch}"
    return f"{major}.{minor}"


def glibc_versions_from_readelf(output: str) -> set[Version]:
    versions = set()
    for match in GLIBC_VERSION_PATTERN.finditer(output):
        patch = int(match.group(3)) if match.group(3) else 0
        versions.add((int(match.group(1)), int(match.group(2)), patch))
    return versions


def readelf_glibc_versions(
    binary: Path,
    *,
    readelf: str = "readelf",
    runner: Runner = subprocess.run,
) -> set[Version]:
    try:
        result = runner(
            [readelf, "--version-info", str(binary)],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    except FileNotFoundError as error:
        raise RuntimeError("readelf is required to verify Linux glibc compatibility") from error

    if result.returncode != 0:
        details = (result.stderr or result.stdout).strip()
        raise RuntimeError(
            f"readelf failed for {binary}: {details or f'exit code {result.returncode}'}"
        )
    return glibc_versions_from_readelf(result.stdout)


def compatibility_error(versions: set[Version], maximum: Version) -> str | None:
    if not versions:
        return None
    highest = max(versions)
    if highest <= maximum:
        return None
    version_list = ", ".join(f"GLIBC_{format_version(version)}" for version in sorted(versions))
    return (
        f"highest required GLIBC_{format_version(highest)} exceeds "
        f"supported GLIBC_{format_version(maximum)}; found: {version_list}"
    )


def check_binary(binary: Path, maximum: Version) -> None:
    if not binary.is_file():
        raise RuntimeError(f"binary does not exist: {binary}")
    versions = readelf_glibc_versions(binary)
    error = compatibility_error(versions, maximum)
    if error:
        raise RuntimeError(error)
    if versions:
        highest = max(versions)
        print(
            f"glibc compatibility OK: highest required GLIBC_{format_version(highest)} "
            f"<= GLIBC_{format_version(maximum)}"
        )
    else:
        print("glibc compatibility OK: no GLIBC version requirements found")


def verify_workflow_policy(path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    required_fragments = [
        "tools/release/check_linux_glibc_compat.py",
        "ubuntu:20.04",
        "linux_glibc_max: '2.31'",
        "Verify Linux GNU glibc compatibility",
        "verify CLI skill Linux asset glibc compatibility",
    ]
    missing = [fragment for fragment in required_fragments if fragment not in text]
    if missing:
        raise RuntimeError(
            f"{path} is missing Linux glibc compatibility release policy: "
            + ", ".join(missing)
        )
    print(f"workflow policy OK: {path}")


def run_self_test() -> None:
    sample = """
      0x0010:   Name: GLIBC_2.2.5  Flags: none  Version: 5
      0x0020:   Name: GLIBC_2.31  Flags: none  Version: 4
      0x0030:   Name: GLIBC_2.17  Flags: none  Version: 3
      0x0040:   Name: GLIBC_PRIVATE  Flags: none  Version: 2
    """
    assert glibc_versions_from_readelf(sample) == {
        (2, 2, 5),
        (2, 17, 0),
        (2, 31, 0),
    }
    assert parse_version("2.31") == (2, 31, 0)
    assert parse_version("GLIBC_2.31") == (2, 31, 0)
    assert compatibility_error(set(), (2, 31, 0)) is None
    assert compatibility_error({(2, 31, 0), (2, 17, 0)}, (2, 31, 0)) is None
    assert "GLIBC_2.34" in (
        compatibility_error({(2, 31, 0), (2, 34, 0)}, (2, 31, 0)) or ""
    )

    def ok_runner(*_args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(["readelf"], 0, stdout=sample, stderr="")

    assert readelf_glibc_versions(Path("fake-binary"), runner=ok_runner) == {
        (2, 2, 5),
        (2, 17, 0),
        (2, 31, 0),
    }

    def missing_runner(*_args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
        raise FileNotFoundError("readelf")

    try:
        readelf_glibc_versions(Path("fake-binary"), runner=missing_runner)
    except RuntimeError as error:
        assert "readelf is required" in str(error)
    else:
        raise AssertionError("missing readelf should fail")

    def bad_runner(*_args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(["readelf"], 1, stdout="", stderr="not an ELF")

    try:
        readelf_glibc_versions(Path("fake-binary"), runner=bad_runner)
    except RuntimeError as error:
        assert "not an ELF" in str(error)
    else:
        raise AssertionError("bad readelf output should fail")

    print("self-test OK")


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Check Linux GNU release binary GLIBC symbol-version requirements."
    )
    parser.add_argument("binary", nargs="?", type=Path)
    parser.add_argument(
        "--max",
        default=DEFAULT_MAX_GLIBC,
        help=f"maximum supported GLIBC version, default {DEFAULT_MAX_GLIBC}",
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--verify-workflow", action="append", type=Path, default=[])
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    try:
        maximum = parse_version(args.max)
        did_work = False
        if args.self_test:
            run_self_test()
            did_work = True
        for workflow in args.verify_workflow:
            verify_workflow_policy(workflow)
            did_work = True
        if args.binary is not None:
            check_binary(args.binary, maximum)
            did_work = True
        if not did_work:
            raise RuntimeError("provide a binary, --self-test, or --verify-workflow")
    except (OSError, RuntimeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
