from __future__ import annotations

import ast
import re
from pathlib import Path

RUNTIME_SOURCE = Path(__file__).parents[1] / "src" / "relay_knowledge_skill_eval"
INSTANCE_ID = re.compile(r"[a-z0-9_.-]+__[a-z0-9_.-]+-\d+", re.IGNORECASE)
IDENTITY_NAMES = {
    "instance_id",
    "problem_statement",
    "repo",
    "repository",
    "slug",
    "task_id",
    "task_name",
}


def test_runtime_source_contains_no_benchmark_instance_ids() -> None:
    matches: list[str] = []
    for path in sorted(RUNTIME_SOURCE.rglob("*.py")):
        for line_number, line in enumerate(
            path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            if INSTANCE_ID.search(line):
                matches.append(f"{path.relative_to(RUNTIME_SOURCE)}:{line_number}")

    assert matches == []


def test_runtime_has_no_literal_task_identity_comparisons() -> None:
    matches: list[str] = []
    for path in sorted(RUNTIME_SOURCE.rglob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for node in ast.walk(tree):
            if not isinstance(node, ast.Compare):
                continue
            identifiers = {
                child.id for child in ast.walk(node) if isinstance(child, ast.Name)
            }
            identifiers.update(
                child.attr
                for child in ast.walk(node)
                if isinstance(child, ast.Attribute)
            )
            string_literals = [
                child.value
                for child in ast.walk(node)
                if isinstance(child, ast.Constant) and isinstance(child.value, str)
            ]
            if identifiers & IDENTITY_NAMES and string_literals:
                matches.append(f"{path.relative_to(RUNTIME_SOURCE)}:{node.lineno}")

    assert matches == []
