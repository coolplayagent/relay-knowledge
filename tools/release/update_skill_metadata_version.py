#!/usr/bin/env python3
"""Update or verify SKILL.md frontmatter metadata."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
from typing import Callable


VERSION_PATTERN = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$")
FRONTMATTER_BOUNDARY = "---"
DESCRIPTION_PREFIX = "description:"
MAX_DESCRIPTION_CHARS = 1024
METADATA_HEADER = "metadata:"
METADATA_VERSION_PREFIX = "  version:"
YAML_NULL_VALUES = {"null", "Null", "NULL", "~"}
DOUBLE_QUOTED_ESCAPES = {
    "0": "\0",
    "a": "\x07",
    "b": "\b",
    "t": "\t",
    "n": "\n",
    "v": "\x0b",
    "f": "\f",
    "r": "\r",
    "e": "\x1b",
    '"': '"',
    "/": "/",
    "\\": "\\",
    "N": "\x85",
    "_": "\xa0",
    "L": "\u2028",
    "P": "\u2029",
}
DOUBLE_QUOTED_HEX_ESCAPE_WIDTHS = {"x": 2, "u": 4, "U": 8}


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


def reject_invalid_plain_scalar(value: str) -> None:
    for index, character in enumerate(value):
        if character == ":" and (
            index + 1 == len(value) or value[index + 1].isspace()
        ):
            raise ValueError(
                "SKILL.md frontmatter description has invalid YAML text; "
                "quote values that contain ': '"
            )


def plain_scalar_without_comment(raw_value: str) -> str:
    value = raw_value.strip()
    for index, character in enumerate(value):
        if character == "#" and (index == 0 or value[index - 1].isspace()):
            value = value[:index].rstrip()
            break
    reject_invalid_plain_scalar(value)
    return value


def verify_quoted_scalar_trailing(raw_value: str, index: int) -> None:
    trailing = plain_scalar_without_comment(raw_value[index + 1 :])
    if trailing:
        raise ValueError("SKILL.md frontmatter description has invalid YAML text")


def single_quoted_scalar(raw_value: str) -> str:
    parsed = []
    index = 1
    while index < len(raw_value):
        character = raw_value[index]
        if character == "'":
            if index + 1 < len(raw_value) and raw_value[index + 1] == "'":
                parsed.append("'")
                index += 2
                continue
            verify_quoted_scalar_trailing(raw_value, index)
            return "".join(parsed)
        parsed.append(character)
        index += 1
    raise ValueError("SKILL.md frontmatter description must be a single-line value")


def double_quoted_escape(raw_value: str, index: int) -> tuple[str, int]:
    if index + 1 >= len(raw_value):
        raise ValueError("SKILL.md frontmatter description has invalid YAML text")
    escape = raw_value[index + 1]
    if escape in DOUBLE_QUOTED_ESCAPES:
        return DOUBLE_QUOTED_ESCAPES[escape], index + 2
    if escape in DOUBLE_QUOTED_HEX_ESCAPE_WIDTHS:
        width = DOUBLE_QUOTED_HEX_ESCAPE_WIDTHS[escape]
        start = index + 2
        end = start + width
        codepoint = raw_value[start:end]
        if len(codepoint) != width or not all(
            character in "0123456789abcdefABCDEF" for character in codepoint
        ):
            raise ValueError("SKILL.md frontmatter description has invalid YAML text")
        try:
            return chr(int(codepoint, 16)), end
        except ValueError as error:
            raise ValueError(
                "SKILL.md frontmatter description has invalid YAML text"
            ) from error
    raise ValueError("SKILL.md frontmatter description has invalid YAML text")


def double_quoted_scalar(raw_value: str) -> str:
    parsed = []
    index = 1
    while index < len(raw_value):
        character = raw_value[index]
        if character == '"':
            verify_quoted_scalar_trailing(raw_value, index)
            return "".join(parsed)
        if character == "\\":
            decoded, index = double_quoted_escape(raw_value, index)
            parsed.append(decoded)
            continue
        parsed.append(character)
        index += 1
    raise ValueError("SKILL.md frontmatter description must be a single-line value")


def quoted_scalar(raw_value: str) -> str:
    if raw_value[0] == "'":
        return single_quoted_scalar(raw_value)
    return double_quoted_scalar(raw_value)


def single_line_yaml_description(raw_value: str, has_continuation: bool) -> str:
    value = raw_value.strip()
    if not value or value[0] == "#":
        raise ValueError("SKILL.md frontmatter description must not be empty")
    if value[0] in {"|", ">"}:
        raise ValueError("SKILL.md frontmatter description must be a single-line value")
    if has_continuation:
        raise ValueError("SKILL.md frontmatter description must be a single-line value")
    if value[0] in {"'", '"'}:
        description = quoted_scalar(value)
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


def expect_value_error(action: Callable[[], object], expected: str) -> None:
    try:
        action()
    except ValueError as error:
        if expected not in str(error):
            raise AssertionError(f"expected {expected!r} in {error!s}") from error
    else:
        raise AssertionError("expected ValueError")


def run_self_test() -> None:
    assert (
        single_line_yaml_description('"repository knowledge graphs: hybrid"', False)
        == "repository knowledge graphs: hybrid"
    )
    assert (
        single_line_yaml_description("https://example.test/path # registry", False)
        == "https://example.test/path"
    )
    assert single_line_yaml_description("repo query # comment", False) == "repo query"

    expect_value_error(
        lambda: single_line_yaml_description("repository knowledge graphs: hybrid", False),
        "invalid YAML text",
    )
    expect_value_error(
        lambda: single_line_yaml_description("repository knowledge graphs:", False),
        "invalid YAML text",
    )
    expect_value_error(
        lambda: single_line_yaml_description("repository knowledge graphs: # comment", False),
        "invalid YAML text",
    )

    lines = [
        "---",
        "name: relay-knowledge-cli",
        'description: "repository knowledge graphs: hybrid"',
        "metadata:",
        "  version: 1.1.0",
        "---",
    ]
    assert frontmatter_description(lines, 5) == "repository knowledge graphs: hybrid"

    print("self-test OK")


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify without rewriting")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("skill_md", nargs="?", type=Path)
    parser.add_argument("version", nargs="?")
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    try:
        did_work = False
        if args.self_test:
            run_self_test()
            did_work = True
        if args.skill_md is not None or args.version is not None:
            if args.skill_md is None or args.version is None:
                raise ValueError("provide both SKILL.md path and expected version")
            if args.check:
                check_skill_metadata(args.skill_md, args.version)
            else:
                write_metadata_version(args.skill_md, args.version)
                check_skill_metadata(args.skill_md, args.version)
            did_work = True
        if not did_work:
            raise ValueError("provide SKILL.md/version, --self-test, or both")
    except (OSError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
