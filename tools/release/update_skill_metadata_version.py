#!/usr/bin/env python3
"""Update or verify SKILL.md frontmatter metadata."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


VERSION_PATTERN = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$")
FRONTMATTER_BOUNDARY = "---"
DESCRIPTION_PREFIX = "description:"
MAX_DESCRIPTION_CHARS = 1024
METADATA_HEADER = "metadata:"
METADATA_VERSION_PREFIX = "  version:"
YAML_NULL_VALUES = {"null", "Null", "NULL", "~"}


def validate_version(version: str) -> None:
    if not VERSION_PATTERN.fullmatch(version):
        raise ValueError(f"metadata.version must be numeric semver: {version}")


def frontmatter_end_index(lines: list[str]) -> int:
    if not lines or lines[0] != FRONTMATTER_BOUNDARY:
        raise ValueError("SKILL.md must start with YAML frontmatter")
    try:
        return lines.index(FRONTMATTER_BOUNDARY, 1)
    except ValueError as error:
        raise ValueError("SKILL.md frontmatter is missing a closing boundary") from error


def metadata_header_index(lines: list[str], end_index: int) -> int:
    for index in range(1, end_index):
        if lines[index] == METADATA_HEADER:
            return index
    raise ValueError("SKILL.md frontmatter is missing metadata")


def top_level_continuation_exists(lines: list[str], index: int, end_index: int) -> bool:
    for next_line in lines[index + 1 : end_index]:
        if not next_line:
            continue
        if not next_line.startswith((" ", "\t")):
            return False
        return True
    return False


def plain_scalar_without_comment(raw_value: str) -> str:
    value = raw_value.strip()
    for index, character in enumerate(value):
        if character == "#" and (index == 0 or value[index - 1].isspace()):
            return value[:index].rstrip()
    return value


def quoted_scalar(raw_value: str, quote: str) -> str:
    escaped = False
    for index, character in enumerate(raw_value[1:], start=1):
        if quote == '"' and character == "\\" and not escaped:
            escaped = True
            continue
        if character == quote and not escaped:
            trailing = plain_scalar_without_comment(raw_value[index + 1 :])
            if trailing:
                raise ValueError("SKILL.md frontmatter description has invalid YAML text")
            return raw_value[1:index]
        escaped = False
    raise ValueError("SKILL.md frontmatter description must be a single-line value")


def single_line_yaml_description(raw_value: str, has_continuation: bool) -> str:
    value = raw_value.strip()
    if not value or value[0] == "#":
        raise ValueError("SKILL.md frontmatter description must not be empty")
    if value[0] in {"|", ">"}:
        raise ValueError("SKILL.md frontmatter description must be a single-line value")
    if has_continuation:
        raise ValueError("SKILL.md frontmatter description must be a single-line value")
    if value[0] in {"'", '"'}:
        description = quoted_scalar(value, value[0])
    else:
        description = plain_scalar_without_comment(value)
    if not description or description in YAML_NULL_VALUES:
        raise ValueError("SKILL.md frontmatter description must not be empty")
    return description


def frontmatter_description(lines: list[str], end_index: int) -> str:
    description = None
    for index in range(1, end_index):
        line = lines[index]
        if line.startswith(DESCRIPTION_PREFIX):
            if description is not None:
                raise ValueError("SKILL.md frontmatter has duplicate description fields")
            raw_value = line.split(":", 1)[1]
            description = single_line_yaml_description(
                raw_value,
                top_level_continuation_exists(lines, index, end_index),
            )
    if description is None:
        raise ValueError("SKILL.md frontmatter is missing description")
    return description


def validate_description(path: Path, description: str) -> None:
    description_chars = len(description)
    if description_chars > MAX_DESCRIPTION_CHARS:
        raise ValueError(
            f"{path} description is {description_chars} characters; "
            f"maximum is {MAX_DESCRIPTION_CHARS}"
        )


def metadata_version_index(lines: list[str], metadata_index: int, end_index: int) -> int | None:
    for index in range(metadata_index + 1, end_index):
        line = lines[index]
        if line and not line.startswith(" "):
            return None
        if line.startswith(METADATA_VERSION_PREFIX):
            return index
    return None


def read_metadata_version(path: Path) -> str | None:
    lines = path.read_text(encoding="utf-8").splitlines()
    end_index = frontmatter_end_index(lines)
    metadata_index = metadata_header_index(lines, end_index)
    version_index = metadata_version_index(lines, metadata_index, end_index)
    if version_index is None:
        return None
    return lines[version_index].split(":", 1)[1].strip()


def check_frontmatter_description(path: Path) -> None:
    lines = path.read_text(encoding="utf-8").splitlines()
    end_index = frontmatter_end_index(lines)
    validate_description(path, frontmatter_description(lines, end_index))


def write_metadata_version(path: Path, version: str) -> None:
    validate_version(version)
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    end_index = frontmatter_end_index(lines)
    metadata_index = metadata_header_index(lines, end_index)
    version_index = metadata_version_index(lines, metadata_index, end_index)

    if version_index is None:
        lines.insert(metadata_index + 1, f"  version: {version}")
    else:
        lines[version_index] = f"  version: {version}"

    trailing_newline = "\n" if text.endswith("\n") else ""
    path.write_text("\n".join(lines) + trailing_newline, encoding="utf-8")


def check_metadata_version(path: Path, expected: str) -> None:
    validate_version(expected)
    actual = read_metadata_version(path)
    if actual != expected:
        raise ValueError(f"{path} metadata.version is {actual!r}; expected {expected!r}")


def check_skill_metadata(path: Path, expected: str) -> None:
    check_metadata_version(path, expected)
    check_frontmatter_description(path)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify without rewriting")
    parser.add_argument("skill_md", type=Path)
    parser.add_argument("version")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.check:
            check_skill_metadata(args.skill_md, args.version)
        else:
            write_metadata_version(args.skill_md, args.version)
            check_skill_metadata(args.skill_md, args.version)
    except (OSError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
