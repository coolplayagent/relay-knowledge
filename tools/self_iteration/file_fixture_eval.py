"""Local file-index fixture evaluation for self-iteration candidates."""

from __future__ import annotations

import json
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from statistics import median
from typing import Any

from scoring import CaseObservation, GateObservation, MetricObservation

AUTHORIZED_FILE_FIXTURE_SCOPE = "local-files"


@dataclass(frozen=True)
class FileCommandResult:
    name: str
    command: list[str]
    exit_code: int
    duration_ms: int
    stdout: str
    stderr: str

    @property
    def passed(self) -> bool:
        return self.exit_code == 0

    def gate(self) -> GateObservation:
        return GateObservation(
            name=self.name,
            passed=self.passed,
            duration_ms=self.duration_ms,
            message=last_output_line(self.stdout, self.stderr),
        )


def evaluate_file_fixtures(
    binary: Path,
    workspace: Path,
    env: dict[str, str],
    run_home: Path,
    fixtures: dict[str, Any],
    all_cases: list[dict[str, Any]],
    timeout: int,
) -> dict[str, Any]:
    commands: list[FileCommandResult] = []
    case_observations: list[CaseObservation] = []
    metrics: list[MetricObservation] = []
    fixture_root = run_home / "file-fixtures"
    fixture_root.mkdir(parents=True, exist_ok=True)

    for fixture_name, fixture in fixtures.items():
        root = fixture_root / fixture_name
        create_file_fixture(root, fixture)
        scope = AUTHORIZED_FILE_FIXTURE_SCOPE
        fixture_env = file_fixture_runtime_env(env, root)
        index = run_command(
            f"{fixture_name}_files_index",
            [
                str(binary),
                "files",
                "index",
                "--root",
                str(root),
                "--source",
                scope,
                "--format",
                "json",
            ],
            workspace,
            fixture_env,
            timeout,
        )
        commands.append(index)
        metrics.append(
            MetricObservation(
                name=f"{fixture_name}_file_index_ms",
                value=index.duration_ms,
                budget=float(fixture.get("index_budget_ms", 0)) or None,
                key=True,
            )
        )
        if not index.passed:
            continue

        durations: list[int] = []
        for case in [case for case in all_cases if case.get("fixture") == fixture_name]:
            query = run_command(
                f"{fixture_name}_{case['id']}",
                file_query_command(binary, scope, case),
                workspace,
                fixture_env,
                min(timeout, int(case.get("timeout_seconds", 10))),
            )
            commands.append(query)
            durations.append(query.duration_ms)
            case_observations.append(score_file_case(fixture_name, case, query))

        if durations:
            metrics.append(
                MetricObservation(
                    name=f"{fixture_name}_file_query_p50_ms",
                    value=float(median(durations)),
                    budget=float(fixture.get("query_p50_budget_ms", 0)) or None,
                    key=False,
                )
            )
            metrics.append(
                MetricObservation(
                    name=f"{fixture_name}_file_query_p95_ms",
                    value=float(percentile(durations, 95)),
                    budget=float(fixture.get("query_p95_budget_ms", 0)) or None,
                    key=True,
                )
            )

    return {"commands": commands, "cases": case_observations, "metrics": metrics}


def file_fixture_runtime_env(env: dict[str, str], root: Path) -> dict[str, str]:
    fixture_env = dict(env)
    root_value = str(root)
    configured_roots = [
        value
        for value in fixture_env.get("RELAY_KNOWLEDGE_FILE_INDEX_ROOTS", "").split(";")
        if value
    ]
    if root_value not in configured_roots:
        configured_roots.append(root_value)
    fixture_env["RELAY_KNOWLEDGE_FILE_INDEX_ROOTS"] = ";".join(configured_roots)
    return fixture_env


def create_file_fixture(root: Path, fixture: dict[str, Any]) -> None:
    if root.exists():
        shutil.rmtree(root)
    root.mkdir(parents=True)
    for file_config in fixture.get("files", []):
        write_fixture_file(root / file_config["path"], file_config.get("content", "fixture"))
    for index in range(int(fixture.get("generate_noise_files", 0))):
        write_fixture_file(
            root / "noise" / f"quarterly-design-noise-{index:04}.txt",
            f"noise {index}",
        )


def write_fixture_file(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def file_query_command(binary: Path, scope: str, case: dict[str, Any]) -> list[str]:
    return [
        str(binary),
        "files",
        "query",
        case["query"],
        "--source",
        scope,
        "--limit",
        str(case.get("limit", 10)),
        "--format",
        "json",
    ]


def score_file_case(fixture_name: str, case: dict[str, Any], result: Any) -> CaseObservation:
    if not result.passed:
        return CaseObservation(
            case_id=case["id"],
            repository="local_files",
            passed=False,
            message=last_output_line(result.stdout, result.stderr),
        )
    payload = parse_json_output(result.stdout)
    hits = payload.get("results", [])
    expected = case.get("expected", [])
    forbidden = case.get("forbidden", [])
    max_rank = int(case.get("max_rank", 1))
    rank = first_expected_rank(hits, expected)
    false_positives = sum(1 for hit in hits if hit_matches_any(hit, forbidden))
    passed = (not expected or (rank is not None and rank <= max_rank)) and false_positives == 0
    if case.get("expect_empty"):
        passed = len(hits) == 0
        rank = 0 if passed else None
    return CaseObservation(
        case_id=case["id"],
        repository=fixture_name,
        passed=passed,
        rank=rank,
        max_rank=max_rank,
        false_positive_count=false_positives,
        message=f"results={len(hits)} rank={rank}",
    )


def run_command(
    name: str,
    command: list[str],
    cwd: Path,
    env: dict[str, str],
    timeout: int,
) -> FileCommandResult:
    started = time.monotonic()
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            env=env,
            text=True,
            capture_output=True,
            timeout=timeout,
            check=False,
        )
        return FileCommandResult(
            name=name,
            command=command,
            exit_code=completed.returncode,
            duration_ms=int((time.monotonic() - started) * 1000),
            stdout=completed.stdout,
            stderr=completed.stderr,
        )
    except subprocess.TimeoutExpired as error:
        return FileCommandResult(
            name=name,
            command=command,
            exit_code=124,
            duration_ms=int((time.monotonic() - started) * 1000),
            stdout=error.stdout or "",
            stderr=(error.stderr or "") + f"\ntimeout after {timeout}s",
        )


def first_expected_rank(hits: list[dict[str, Any]], expected: list[dict[str, Any]]) -> int | None:
    for index, hit in enumerate(hits, start=1):
        if hit_matches_any(hit, expected):
            return index
    return None


def hit_matches_any(hit: dict[str, Any], patterns: list[dict[str, Any]]) -> bool:
    return any(hit_matches(hit, pattern) for pattern in patterns)


def hit_matches(hit: dict[str, Any], pattern: dict[str, Any]) -> bool:
    for field in ("relative_path", "file_name", "extension", "status"):
        if field in pattern and hit.get(field) != pattern[field]:
            return False
    return True


def parse_json_output(stdout: str) -> dict[str, Any]:
    for line in reversed(stdout.splitlines()):
        line = line.strip()
        if line:
            return json.loads(line)
    return {}


def percentile(values: list[int], percentile_value: int) -> int:
    if not values:
        return 0
    ordered = sorted(values)
    index = round((len(ordered) - 1) * percentile_value / 100)
    return ordered[index]


def last_output_line(stdout: str, stderr: str) -> str:
    for output in (stderr, stdout):
        lines = [line.strip() for line in output.splitlines() if line.strip()]
        if lines:
            return lines[-1][:400]
    return ""
