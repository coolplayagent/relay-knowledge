#!/usr/bin/env python3
"""Validate the repository documentation bookshelf without third-party packages."""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
import unicodedata
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
from urllib.parse import unquote, urlsplit


FENCE_RE = re.compile(r"^ {0,3}(`{3,}|~{3,})(?![`~])(.*)$")
FENCE_CANDIDATE_RE = re.compile(r"^ {0,3}([`~]{3,})(.*)$")
HEADING_RE = re.compile(r"^ {0,3}(#{1,6})\s+(.+?)\s*$")
REFERENCE_LINK_RE = re.compile(r"^\s*\[[^\]]+\]:\s*(\S.*)$")
VOLUME_RE = re.compile(r"^\d{2}-")
CHAPTER_RE = re.compile(r"^(\d{2})-")
POSIX_FENCE_LANGUAGES = {"bash", "console", "sh", "shell", "zsh"}
WINDOWS_COMMAND_RE = re.compile(
    r"(?:(?<![A-Za-z0-9])[A-Za-z]:[\\/]|\.exe\b|assets[\\/]windows-)",
    re.IGNORECASE,
)
MAX_DOCUMENT_LINES = 1_000


@dataclass(frozen=True, order=True)
class Diagnostic:
    path: Path
    line: int
    message: str

    def render(self, root: Path) -> str:
        try:
            display_path = self.path.relative_to(root)
        except ValueError:
            display_path = self.path
        return f"{display_path}:{self.line}: {self.message}"


@dataclass(frozen=True)
class DocumentScan:
    diagnostics: tuple[Diagnostic, ...]
    local_targets: frozenset[Path]


def _link_destination(raw_target: str) -> str:
    target = raw_target.strip()
    if target.startswith("<"):
        closing = _find_unescaped(target, ">", start=1)
        return _unescape_markdown(target[1:closing] if closing >= 0 else target[1:])

    destination: list[str] = []
    index = 0
    while index < len(target):
        character = target[index]
        if character == "\\" and index + 1 < len(target):
            destination.append(target[index + 1])
            index += 2
            continue
        if character.isspace():
            break
        destination.append(character)
        index += 1
    return "".join(destination)


def _is_escaped(value: str, index: int) -> bool:
    backslashes = 0
    index -= 1
    while index >= 0 and value[index] == "\\":
        backslashes += 1
        index -= 1
    return backslashes % 2 == 1


def _find_unescaped(value: str, character: str, start: int = 0) -> int:
    index = value.find(character, start)
    while index >= 0 and _is_escaped(value, index):
        index = value.find(character, index + 1)
    return index


def _unescape_markdown(value: str) -> str:
    output: list[str] = []
    index = 0
    while index < len(value):
        if value[index] == "\\" and index + 1 < len(value):
            output.append(value[index + 1])
            index += 2
        else:
            output.append(value[index])
            index += 1
    return "".join(output)


def _mixed_fence_marker(line: str) -> str | None:
    candidate = FENCE_CANDIDATE_RE.match(line)
    if candidate is None:
        return None
    marker = candidate.group(1)
    return marker if "`" in marker and "~" in marker else None


def _mixed_marker_attempts_close(
    marker: str, fence_character: str, fence_width: int
) -> bool:
    same_character_prefix = len(marker) - len(marker.lstrip(fence_character))
    return same_character_prefix >= fence_width


def _inline_link_targets(line: str) -> list[str]:
    """Return inline-link payloads while honoring nesting and backslash escapes."""
    targets: list[str] = []
    index = 0
    while index < len(line):
        label_start = line.find("[", index)
        if label_start < 0:
            break
        if _is_escaped(line, label_start):
            index = label_start + 1
            continue

        label_depth = 1
        cursor = label_start + 1
        while cursor < len(line) and label_depth:
            if not _is_escaped(line, cursor):
                if line[cursor] == "[":
                    label_depth += 1
                elif line[cursor] == "]":
                    label_depth -= 1
            cursor += 1
        if label_depth or cursor >= len(line) or line[cursor] != "(":
            index = label_start + 1
            continue

        payload_start = cursor + 1
        parenthesis_depth = 1
        quote = ""
        cursor = payload_start
        while cursor < len(line) and parenthesis_depth:
            character = line[cursor]
            if _is_escaped(line, cursor):
                cursor += 1
                continue
            if quote:
                if character == quote:
                    quote = ""
            elif character in {'"', "'"}:
                quote = character
            elif character == "(":
                parenthesis_depth += 1
            elif character == ")":
                parenthesis_depth -= 1
            cursor += 1
        if parenthesis_depth == 0:
            targets.append(line[payload_start : cursor - 1])
            index = cursor
        else:
            index = label_start + 1
    return targets


def _without_inline_code(line: str) -> str:
    """Mask inline-code spans so source examples are not parsed as links."""
    output = list(line)
    index = 0
    while index < len(line):
        if line[index] != "`" or _is_escaped(line, index):
            index += 1
            continue
        end = index
        while end < len(line) and line[end] == "`":
            end += 1
        width = end - index
        closing = end
        while closing < len(line):
            closing = line.find("`" * width, closing)
            if closing < 0:
                break
            run_end = closing
            while run_end < len(line) and line[run_end] == "`":
                run_end += 1
            if run_end - closing == width and not _is_escaped(line, closing):
                output[index:run_end] = " " * (run_end - index)
                index = run_end
                break
            closing = run_end
        else:
            closing = -1
        if closing < 0:
            index = end
    return "".join(output)


def _resolve_local_target(source: Path, raw_target: str) -> tuple[Path | None, str]:
    destination = _link_destination(raw_target)
    parsed = urlsplit(destination)
    if parsed.scheme or parsed.netloc:
        return None, ""
    decoded_path = unquote(parsed.path)
    if not decoded_path:
        return source.resolve(), unquote(parsed.fragment)
    if decoded_path.startswith("/"):
        target = Path(decoded_path).resolve()
    else:
        target = (source.parent / decoded_path).resolve()
    return target, unquote(parsed.fragment)


def _heading_slug(value: str) -> str:
    plain = re.sub(r"<[^>]+>", "", value)
    plain = re.sub(r"!?\[([^\]]+)\]\([^)]+\)", r"\1", plain)
    plain = plain.replace("`", "").replace("*", "")
    plain = "".join(
        character
        for character in plain.casefold()
        if character.isalnum() or character in {" ", "-", "_"}
    )
    return plain.replace(" ", "-").strip("-")


@lru_cache(maxsize=None)
def _document_anchors(path: Path) -> frozenset[str] | None:
    anchors: set[str] = set()
    in_fence = False
    fence_character = ""
    fence_width = 0
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeDecodeError):
        return None
    for line in lines:
        fence_match = FENCE_RE.match(line)
        if fence_match:
            marker = fence_match.group(1)
            if not in_fence:
                in_fence = True
                fence_character = marker[0]
                fence_width = len(marker)
            elif (
                marker[0] == fence_character
                and len(marker) >= fence_width
                and not fence_match.group(2).strip()
            ):
                in_fence = False
            continue
        if in_fence:
            continue
        heading_match = HEADING_RE.match(line)
        if not heading_match:
            continue
        base = _heading_slug(heading_match.group(2).rstrip("#").rstrip())
        if not base:
            continue
        candidate = base
        suffix = 1
        while candidate in anchors:
            candidate = f"{base}-{suffix}"
            suffix += 1
        anchors.add(candidate)
    return frozenset(anchors)


def _validate_posix_fence(
    path: Path, line: int, language: str, body: list[str]
) -> list[Diagnostic]:
    if language not in POSIX_FENCE_LANGUAGES:
        return []
    diagnostics: list[Diagnostic] = []
    for offset, content in enumerate(body, start=1):
        if WINDOWS_COMMAND_RE.search(content):
            diagnostics.append(
                Diagnostic(
                    path,
                    line + offset,
                    "Windows command appears in a POSIX-labelled code fence",
                )
            )
    return diagnostics


def scan_document(path: Path, repository_root: Path) -> DocumentScan:
    diagnostics: list[Diagnostic] = []
    local_targets: set[Path] = set()
    try:
        raw = path.read_bytes()
        text = raw.decode("utf-8")
    except UnicodeDecodeError as error:
        invalid_line = raw[: error.start].count(b"\n") + 1
        return DocumentScan(
            (Diagnostic(path, invalid_line, "file is not valid UTF-8"),),
            frozenset(),
        )

    if not text.strip():
        diagnostics.append(Diagnostic(path, 1, "document is empty"))
        return DocumentScan(tuple(diagnostics), frozenset())
    if not text.endswith("\n"):
        diagnostics.append(Diagnostic(path, len(text.splitlines()), "missing final newline"))
    if "\r" in text:
        diagnostics.append(Diagnostic(path, 1, "use LF line endings"))
    line_count = len(text.splitlines())
    if line_count > MAX_DOCUMENT_LINES:
        diagnostics.append(
            Diagnostic(
                path,
                MAX_DOCUMENT_LINES + 1,
                f"document has {line_count} lines; maximum is {MAX_DOCUMENT_LINES}",
            )
        )

    fence_character: str | None = None
    fence_width = 0
    fence_line = 0
    fence_language = ""
    fence_body: list[str] = []
    heading_levels: list[tuple[int, int]] = []
    first_content_line: tuple[int, str] | None = None

    for line_number, line in enumerate(text.splitlines(), start=1):
        if line.rstrip(" \t") != line:
            diagnostics.append(Diagnostic(path, line_number, "trailing whitespace"))
        if "\t" in line:
            diagnostics.append(Diagnostic(path, line_number, "tab character is not allowed"))

        fence_match = FENCE_RE.match(line)
        mixed_fence_marker = _mixed_fence_marker(line)
        if fence_character is not None:
            if (
                fence_match
                and fence_match.group(1)[0] == fence_character
                and len(fence_match.group(1)) >= fence_width
                and not fence_match.group(2).strip()
            ):
                diagnostics.extend(
                    _validate_posix_fence(
                        path, fence_line, fence_language, fence_body
                    )
                )
                fence_character = None
                fence_body = []
            else:
                if mixed_fence_marker is not None and _mixed_marker_attempts_close(
                    mixed_fence_marker, fence_character, fence_width
                ):
                    diagnostics.append(
                        Diagnostic(
                            path,
                            line_number,
                            "code fence marker cannot mix backticks and tildes",
                        )
                    )
                fence_body.append(line)
            continue

        if mixed_fence_marker is not None:
            diagnostics.append(
                Diagnostic(
                    path,
                    line_number,
                    "code fence marker cannot mix backticks and tildes",
                )
            )
            continue

        if fence_match:
            marker, info = fence_match.groups()
            fence_character = marker[0]
            fence_width = len(marker)
            fence_line = line_number
            fence_language = info.strip().split(maxsplit=1)[0].lower() if info.strip() else ""
            if not fence_language:
                diagnostics.append(
                    Diagnostic(path, line_number, "opening code fence needs a language label")
                )
            continue

        if first_content_line is None and line.strip():
            first_content_line = (line_number, line)

        heading_match = HEADING_RE.match(line)
        if heading_match:
            level = len(heading_match.group(1))
            heading_levels.append((line_number, level))

        link_text = _without_inline_code(line)
        raw_targets = _inline_link_targets(link_text)
        reference_match = REFERENCE_LINK_RE.match(link_text)
        if reference_match:
            raw_targets.append(reference_match.group(1))
        for raw_target in raw_targets:
            try:
                target, fragment = _resolve_local_target(path, raw_target)
            except ValueError as error:
                diagnostics.append(
                    Diagnostic(path, line_number, f"invalid link destination: {error}")
                )
                continue
            if target is None:
                continue
            local_targets.add(target)
            try:
                target.relative_to(repository_root)
            except ValueError:
                diagnostics.append(
                    Diagnostic(path, line_number, "local link escapes the repository")
                )
                continue
            if not target.exists():
                diagnostics.append(
                    Diagnostic(
                        path,
                        line_number,
                        f"local link target does not exist: {_link_destination(raw_target)}",
                    )
                )
                continue
            target_anchors = (
                _document_anchors(target)
                if fragment
                and target.is_file()
                and target.suffix.lower() in {".md", ".markdown"}
                else None
            )
            if fragment and target_anchors is not None and fragment not in target_anchors:
                diagnostics.append(
                    Diagnostic(
                        path,
                        line_number,
                        f"local link anchor does not exist: #{fragment}",
                    )
                )

    if fence_character is not None:
        diagnostics.append(Diagnostic(path, fence_line, "code fence is not closed"))

    h1_lines = [line for line, level in heading_levels if level == 1]
    if len(h1_lines) != 1:
        diagnostics.append(
            Diagnostic(path, 1, f"expected exactly one H1 heading, found {len(h1_lines)}")
        )
    if first_content_line and not re.match(r"^#\s+", first_content_line[1]):
        diagnostics.append(
            Diagnostic(path, first_content_line[0], "first content line must be the H1 heading")
        )
    for (previous_line, previous), (line_number, current) in zip(
        heading_levels, heading_levels[1:]
    ):
        if current > previous + 1:
            diagnostics.append(
                Diagnostic(
                    path,
                    line_number,
                    f"heading level jumps from H{previous} on line {previous_line} to H{current}",
                )
            )

    return DocumentScan(tuple(diagnostics), frozenset(local_targets))


def _markdown_directories(docs_root: Path) -> list[Path]:
    directories: set[Path] = set()
    for document in docs_root.rglob("*.md"):
        directory = document.parent
        while directory == docs_root or docs_root in directory.parents:
            directories.add(directory)
            if directory == docs_root:
                break
            directory = directory.parent
    return sorted(directories)


def validate_navigation(
    docs_root: Path, scans: dict[Path, DocumentScan]
) -> list[Diagnostic]:
    diagnostics: list[Diagnostic] = []
    markdown_directories = set(_markdown_directories(docs_root))
    for directory in sorted(markdown_directories):
        readme = directory / "README.md"
        if not readme.exists():
            diagnostics.append(Diagnostic(directory, 1, "directory needs a README.md index"))
            continue
        linked_targets = scans[readme].local_targets
        for document in sorted(directory.glob("*.md")):
            if document == readme:
                continue
            if document.resolve() not in linked_targets:
                diagnostics.append(
                    Diagnostic(
                        readme,
                        1,
                        f"index does not link sibling document: {document.name}",
                    )
                )
        for child in sorted(
            item
            for item in directory.iterdir()
            if item.is_dir() and item in markdown_directories
        ):
            child_readme = child / "README.md"
            if not child_readme.exists():
                continue
            if child_readme.resolve() not in linked_targets:
                diagnostics.append(
                    Diagnostic(
                        readme,
                        1,
                        f"index does not link child index: {child_readme.relative_to(directory)}",
                    )
                )
    return diagnostics


def validate_book_structure(docs_root: Path) -> list[Diagnostic]:
    diagnostics: list[Diagnostic] = []
    for language in ("en", "zh"):
        edition = docs_root / language
        if not edition.is_dir():
            diagnostics.append(Diagnostic(edition, 1, "missing language edition"))
            continue
        volume_prefixes: dict[str, Path] = {}
        for volume in sorted(path for path in edition.iterdir() if path.is_dir()):
            volume_match = VOLUME_RE.match(volume.name)
            if not volume_match:
                diagnostics.append(
                    Diagnostic(volume, 1, "volume directory needs a two-digit prefix")
                )
                continue
            prefix = volume.name[:2]
            if prefix in volume_prefixes:
                diagnostics.append(
                    Diagnostic(
                        volume,
                        1,
                        f"volume prefix {prefix} duplicates {volume_prefixes[prefix].name}",
                    )
                )
            else:
                volume_prefixes[prefix] = volume
            chapters: dict[str, Path] = {}
            for document in sorted(volume.glob("*.md")):
                if document.name == "README.md":
                    continue
                match = CHAPTER_RE.match(document.name)
                if not match:
                    diagnostics.append(
                        Diagnostic(document, 1, "chapter filename needs a two-digit prefix")
                    )
                    continue
                chapter = match.group(1)
                if chapter in chapters:
                    diagnostics.append(
                        Diagnostic(
                            document,
                            1,
                            f"chapter prefix {chapter} duplicates {chapters[chapter].name}",
                        )
                    )
                else:
                    chapters[chapter] = document
    return diagnostics


def validate_language_editions(docs_root: Path) -> list[Diagnostic]:
    """Reject untranslated prose in English while allowing bilingual labels."""
    diagnostics: list[Diagnostic] = []
    english_root = docs_root / "en"
    for path in sorted(english_root.rglob("*.md")):
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except (OSError, UnicodeDecodeError):
            # scan_document owns file-read diagnostics. Avoid a second read
            # turning one malformed document into a checker crash.
            continue
        in_fence = False
        fence_character = ""
        fence_width = 0
        for line_number, line in enumerate(lines, start=1):
            fence_match = FENCE_RE.match(line)
            if fence_match:
                marker = fence_match.group(1)
                if not in_fence:
                    in_fence = True
                    fence_character = marker[0]
                    fence_width = len(marker)
                elif (
                    marker[0] == fence_character
                    and len(marker) >= fence_width
                    and not fence_match.group(2).strip()
                ):
                    in_fence = False
                continue
            if in_fence:
                continue
            prose = _without_inline_code(line).replace("中文", "")
            cjk_count = sum(
                1
                for character in prose
                if unicodedata.name(character, "").startswith("CJK UNIFIED IDEOGRAPH")
            )
            if cjk_count:
                diagnostics.append(
                    Diagnostic(
                        path,
                        line_number,
                        f"English prose contains {cjk_count} CJK characters; "
                        "translate or move it to the Chinese edition",
                    )
                )
    return diagnostics


def validate_docs(repository_root: Path, docs_root: Path) -> tuple[int, list[Diagnostic]]:
    files = sorted(docs_root.rglob("*.md"))
    scans = {path: scan_document(path, repository_root) for path in files}
    diagnostics = [item for scan in scans.values() for item in scan.diagnostics]
    diagnostics.extend(validate_navigation(docs_root, scans))
    diagnostics.extend(validate_book_structure(docs_root))
    diagnostics.extend(validate_language_editions(docs_root))
    return len(files), sorted(set(diagnostics))


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(f"documentation checker self-test failed: {message}")


def run_self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="relay-knowledge-doc-check-") as temporary:
        def write_fixture(path: Path, value: str) -> None:
            path.write_bytes(value.encode("utf-8"))

        root = Path(temporary)
        docs = root / "docs"
        docs.mkdir()
        target = docs / "target.md"
        write_fixture(
            target,
            "# Target\n\n## relay_code_query\n\n## Repeat\n\n"
            "## Repeat\n\n## Repeat-1\n",
        )
        escaped_target = docs / "target (copy).md"
        write_fixture(escaped_target, "# Escaped target\n")
        good = docs / "good.md"
        write_fixture(
            good,
            "# Good\n\n[Target](target.md), "
            "[escaped](target\\ \\(copy\\).md), and `[value](not-a-link)`.\n\n"
            "[underscore](target.md#relay_code_query) and "
            "[collision](target.md#repeat-1-1).\n\n"
            "```bash\nprintf 'ok\\n'\n```\n",
        )
        _require(not scan_document(good, root).diagnostics, "valid document rejected")

        anchored = docs / "anchored.md"
        write_fixture(
            anchored,
            "# Anchored\n\n## Pinned External Repositories\n\n"
            "[Target](target.md)\n",
        )
        anchor_link = docs / "anchor-link.md"
        write_fixture(
            anchor_link,
            "# Link\n\n[Section](anchored.md#pinned-external-repositories)\n",
        )
        _require(
            not scan_document(anchor_link, root).diagnostics,
            "valid cross-document anchor rejected",
        )

        bad = docs / "bad.md"
        write_fixture(bad, "# Bad\n\n### Jump\n\n```\nvalue\n```\n")
        messages = [item.message for item in scan_document(bad, root).diagnostics]
        _require(
            any("heading level jumps" in message for message in messages),
            "heading-level jump not detected",
        )
        _require(
            any("language label" in message for message in messages),
            "unlabelled code fence not detected",
        )

        bad_links = docs / "bad-links.md"
        write_fixture(
            bad_links,
            "# Bad links\n\n[Missing](missing.md)\n\n"
            "[Missing anchor](anchored.md#missing)\n\n"
            "[Missing local anchor](#missing)\n\n"
            "[Malformed](http://[)\n\n"
            "```bash\nC:/Relay/RELAY-KNOWLEDGE.EXE version\n"
            "assets\\windows-x86_64\\relay-knowledge.exe version\n```\n",
        )
        messages = [item.message for item in scan_document(bad_links, root).diagnostics]
        _require(
            any("target does not exist" in message for message in messages),
            "missing local target not detected",
        )
        _require(
            sum("anchor does not exist" in message for message in messages) == 2,
            "missing same-document or cross-document anchor not detected",
        )
        _require(
            any("invalid link destination" in message for message in messages),
            "malformed link destination not detected",
        )
        _require(
            sum("Windows command" in message for message in messages) == 2,
            "Windows commands in POSIX fences not fully detected",
        )

        too_long = docs / "too-long.md"
        write_fixture(too_long, "# Long\n" + "\n" * MAX_DOCUMENT_LINES)
        _require(
            any(
                "maximum is" in item.message
                for item in scan_document(too_long, root).diagnostics
            ),
            "document line limit not enforced",
        )

        invalid_utf8 = docs / "invalid-utf8.md"
        invalid_utf8.write_bytes(b"# UTF-8\nvalid\n\xff\n")
        invalid_diagnostics = scan_document(invalid_utf8, root).diagnostics
        _require(
            invalid_diagnostics and invalid_diagnostics[0].line == 3,
            "invalid UTF-8 line number is inaccurate",
        )

        invalid_anchor_target = docs / "invalid-anchor-target.md"
        invalid_anchor_target.write_bytes(b"# Invalid anchor target\n\xff\n")
        invalid_anchor_link = docs / "invalid-anchor-link.md"
        write_fixture(
            invalid_anchor_link,
            "# Invalid anchor link\n\n[Section](invalid-anchor-target.md#section)\n",
        )
        _require(
            not scan_document(invalid_anchor_link, root).diagnostics,
            "invalid UTF-8 link target crashed or produced a false anchor diagnostic",
        )
        _require(
            any(
                "not valid UTF-8" in diagnostic.message
                for diagnostic in scan_document(invalid_anchor_target, root).diagnostics
            ),
            "invalid UTF-8 anchor target lost its owning scan diagnostic",
        )

        mixed_fence = docs / "mixed-fence.md"
        write_fixture(
            mixed_fence,
            "# Mixed fence\n\n```~bash\nprintf 'not fenced\\n'\n```~\n",
        )
        mixed_messages = [
            diagnostic.message for diagnostic in scan_document(mixed_fence, root).diagnostics
        ]
        _require(
            sum("cannot mix backticks and tildes" in message for message in mixed_messages)
            == 2,
            "mixed code-fence markers not diagnosed",
        )

        english = docs / "en"
        english.mkdir()
        untranslated = english / "untranslated.md"
        write_fixture(untranslated, "# English title\n\n这是短句。\n")
        _require(
            bool(validate_language_editions(docs)),
            "short untranslated English-edition prose not detected",
        )
        invalid_english = english / "invalid-utf8.md"
        invalid_english.write_bytes(b"# English title\n\xff\n")
        language_diagnostics = validate_language_editions(docs)
        _require(
            all(diagnostic.path != invalid_english for diagnostic in language_diagnostics),
            "English-edition validation did not defer invalid UTF-8 to document scanning",
        )

        navigation = root / "navigation"
        deep = navigation / "branch" / "deep"
        deep.mkdir(parents=True)
        write_fixture(navigation / "README.md", "# Navigation\n")
        write_fixture(deep / "README.md", "# Deep\n")
        navigation_files = sorted(navigation.rglob("*.md"))
        navigation_scans = {
            path: scan_document(path, root) for path in navigation_files
        }
        _require(
            any(
                diagnostic.path == navigation / "branch"
                and "needs a README.md" in diagnostic.message
                for diagnostic in validate_navigation(navigation, navigation_scans)
            ),
            "intermediate directory without an index not detected",
        )

        book = root / "book"
        for directory in (book / "en" / "01-one", book / "en" / "01-two", book / "zh"):
            directory.mkdir(parents=True)
        _require(
            any(
                "volume prefix 01 duplicates" in diagnostic.message
                for diagnostic in validate_book_structure(book)
            ),
            "duplicate volume prefix not detected",
        )
    print("documentation checker self-test passed")


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="repository root (default: inferred from this script)",
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument(
        "--self-test-and-check",
        action="store_true",
        help="run the parser self-test, then validate the repository documentation",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv if argv is not None else sys.argv[1:])
    if args.self_test or args.self_test_and_check:
        run_self_test()
    if args.self_test and not args.self_test_and_check:
        return 0

    repository_root = args.root.resolve()
    docs_root = repository_root / "docs"
    file_count, diagnostics = validate_docs(repository_root, docs_root)
    for diagnostic in diagnostics:
        print(diagnostic.render(repository_root))
    if diagnostics:
        print(
            f"documentation check failed: {len(diagnostics)} issue(s) across "
            f"{file_count} Markdown files",
            file=sys.stderr,
        )
        return 1
    print(f"documentation check passed: {file_count} Markdown files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
