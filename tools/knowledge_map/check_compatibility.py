#!/usr/bin/env python3
"""Validate the repository Knowledge Map with source and stable CLIs."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

MINIMUM_V2_STABLE_READER = (1, 1, 14)


def command_json(binary: Path, *arguments: str) -> tuple[int, dict[str, object]]:
    completed = subprocess.run(
        [str(binary), *arguments, "--format", "json"],
        check=False,
        capture_output=True,
        text=True,
    )
    output = completed.stdout.strip() or completed.stderr.strip()
    try:
        payload = json.loads(output)
    except json.JSONDecodeError as error:
        raise RuntimeError(
            f"{binary} returned non-JSON output for {' '.join(arguments)}: {output}"
        ) from error
    return completed.returncode, payload


def binary_version(binary: Path) -> tuple[int, int, int]:
    status, payload = command_json(binary, "version")
    if status != 0:
        raise RuntimeError(f"failed to read version from {binary}: {payload}")
    value = str(payload.get("version", ""))
    core = value.split("-", 1)[0].split("+", 1)[0]
    parts = core.split(".")
    if len(parts) != 3 or not all(part.isdigit() for part in parts):
        raise RuntimeError(f"{binary} returned invalid semantic version {value!r}")
    return tuple(int(part) for part in parts)  # type: ignore[return-value]


def validation(binary: Path) -> tuple[bool, dict[str, object]]:
    status, payload = command_json(binary, "map", "validate")
    return status == 0 and payload.get("valid") is True, payload


def stable_reader_status(
    current_version: tuple[int, int, int],
    stable_version: tuple[int, int, int],
    stable_valid: bool,
) -> str:
    if stable_valid:
        return "compatible"
    if stable_version >= MINIMUM_V2_STABLE_READER:
        return "incompatible"
    if current_version <= stable_version:
        return "incompatible_same_version"
    return "staged_pending_reader_release"


def report(
    current: Path, stable: Path, output: Path
) -> tuple[dict[str, object], bool]:
    current_version = binary_version(current)
    stable_version = binary_version(stable)
    current_valid, current_payload = validation(current)
    stable_valid, stable_payload = validation(stable)

    stable_status = stable_reader_status(current_version, stable_version, stable_valid)

    payload: dict[str, object] = {
        "schema_version": 1,
        "knowledge_map_path": ".knowledge/knowledge-map.yaml",
        "required_reader_schema_version": 2,
        "minimum_v2_stable_reader": ".".join(
            str(part) for part in MINIMUM_V2_STABLE_READER
        ),
        "current": {
            "binary": str(current),
            "version": ".".join(str(part) for part in current_version),
            "valid": current_valid,
            "diagnostics": current_payload,
        },
        "stable": {
            "binary": str(stable),
            "version": ".".join(str(part) for part in stable_version),
            "valid": stable_valid,
            "status": stable_status,
            "diagnostics": stable_payload,
        },
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return payload, current_valid and stable_status in {
        "compatible",
        "staged_pending_reader_release",
    }


def self_test() -> None:
    assert stable_reader_status((1, 1, 14), (1, 1, 13), False) == (
        "staged_pending_reader_release"
    )
    assert stable_reader_status((1, 1, 13), (1, 1, 13), False) == (
        "incompatible_same_version"
    )
    assert stable_reader_status((1, 1, 14), (1, 1, 14), False) == "incompatible"
    assert stable_reader_status((1, 1, 14), (1, 1, 13), True) == "compatible"
    print("knowledge-map compatibility checker self-test passed")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--current", type=Path)
    parser.add_argument("--stable", type=Path)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("target/knowledge-map/compatibility.json"),
    )
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return 0
    if args.current is None or args.stable is None:
        parser.error("--current and --stable are required unless --self-test is used")

    payload, compatible = report(args.current.resolve(), args.stable.resolve(), args.output)
    print(json.dumps(payload, separators=(",", ":")))
    return 0 if compatible else 1


if __name__ == "__main__":
    sys.exit(main())
