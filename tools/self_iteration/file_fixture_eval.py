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
        for case in [
            case
            for case in all_cases
            if case.get("fixture") == fixture_name and case.get("mode") != "background_auto_index"
        ]:
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

    background_report = evaluate_background_file_index_cases(
        binary=binary,
        workspace=workspace,
        env=env,
        fixture_root=fixture_root,
        fixtures=fixtures,
        all_cases=all_cases,
        timeout=timeout,
    )
    commands.extend(background_report["commands"])
    case_observations.extend(background_report["cases"])
    metrics.extend(background_report["metrics"])

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


def background_file_fixture_runtime_env(
    env: dict[str, str],
    root: Path,
    scan_interval_ms: int,
) -> dict[str, str]:
    fixture_env = file_fixture_runtime_env(env, root)
    fixture_env["RELAY_KNOWLEDGE_FILE_INDEX_ENABLED"] = "true"
    fixture_env["RELAY_KNOWLEDGE_FILE_INDEX_SCAN_INTERVAL_MS"] = str(scan_interval_ms)
    fixture_env.setdefault("RELAY_KNOWLEDGE_FILE_INDEX_SCAN_TIMEOUT_MS", "5000")
    fixture_env.setdefault("RELAY_KNOWLEDGE_FILE_INDEX_QUERY_TIMEOUT_MS", "750")
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


def evaluate_background_file_index_cases(
    binary: Path,
    workspace: Path,
    env: dict[str, str],
    fixture_root: Path,
    fixtures: dict[str, Any],
    all_cases: list[dict[str, Any]],
    timeout: int,
) -> dict[str, Any]:
    commands: list[FileCommandResult] = []
    case_observations: list[CaseObservation] = []
    metrics: list[MetricObservation] = []
    cases = [case for case in all_cases if case.get("mode") == "background_auto_index"]
    for case in cases:
        fixture_name = str(case["fixture"])
        fixture = fixtures.get(fixture_name)
        if not isinstance(fixture, dict):
            case_observations.append(
                CaseObservation(
                    case_id=case["id"],
                    repository=fixture_name,
                    passed=False,
                    message=f"missing fixture {fixture_name}",
                    objective=str(case.get("objective", "competitive_capability")),
                )
            )
            continue
        root = fixture_root / f"{fixture_name}-{case['id']}"
        create_file_fixture(root, fixture)
        scan_interval_ms = int(case.get("scan_interval_ms", 250))
        fixture_env = background_file_fixture_runtime_env(env, root, scan_interval_ms)
        service = subprocess.Popen(
            [str(binary), "service", "run"],
            cwd=workspace,
            env=fixture_env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        started = time.monotonic()
        final_query: FileCommandResult | None = None
        try:
            for action in case.get("actions_after_start", []):
                apply_fixture_action(root, action)
            deadline = started + min(timeout, int(case.get("timeout_seconds", 8)))
            while time.monotonic() < deadline:
                if service.poll() is not None:
                    break
                query = run_command(
                    f"{fixture_name}_{case['id']}_query",
                    file_query_command(binary, AUTHORIZED_FILE_FIXTURE_SCOPE, case),
                    workspace,
                    fixture_env,
                    min(5, max(1, int(deadline - time.monotonic()) + 1)),
                )
                final_query = query
                if score_file_case(fixture_name, case, query).passed:
                    break
                time.sleep(float(case.get("poll_interval_ms", 200)) / 1000.0)
        finally:
            stop_background_service(service)
        duration_ms = int((time.monotonic() - started) * 1000)
        if final_query is None:
            final_query = FileCommandResult(
                name=f"{fixture_name}_{case['id']}_query",
                command=file_query_command(binary, AUTHORIZED_FILE_FIXTURE_SCOPE, case),
                exit_code=1,
                duration_ms=duration_ms,
                stdout="",
                stderr="background file index service exited before query",
            )
        commands.append(final_query)
        observation = score_file_case(fixture_name, case, final_query)
        case_observations.append(observation)
        metrics.append(
            MetricObservation(
                name=f"{fixture_name}_{case['id']}_file_auto_index_first_seen_ms",
                value=float(duration_ms),
                budget=float(case.get("auto_index_budget_ms", 0)) or None,
                key=True,
            )
        )

    return {"commands": commands, "cases": case_observations, "metrics": metrics}


def apply_fixture_action(root: Path, action: dict[str, Any]) -> None:
    action_type = action.get("type")
    if action_type == "write":
        write_fixture_file(root / action["path"], action.get("content", "fixture"))
    elif action_type == "delete":
        path = root / action["path"]
        if path.exists():
            path.unlink()
    elif action_type == "rename":
        source = root / action["from"]
        target = root / action["to"]
        target.parent.mkdir(parents=True, exist_ok=True)
        source.rename(target)
    else:
        raise ValueError(f"unsupported file fixture action: {action_type}")


def stop_background_service(service: subprocess.Popen[str]) -> None:
    if service.poll() is None:
        service.terminate()
        try:
            service.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            service.kill()
            service.communicate(timeout=5)


def score_file_case(fixture_name: str, case: dict[str, Any], result: Any) -> CaseObservation:
    if not result.passed:
        return CaseObservation(
            case_id=case["id"],
            repository="local_files",
            passed=False,
            message=last_output_line(result.stdout, result.stderr),
            objective=str(case.get("objective", "foundational_capability")),
        )
    payload = parse_json_output(result.stdout)
    hits = payload.get("results", [])
    expected = case.get("expected", [])
    forbidden = case.get("forbidden", [])
    max_rank = int(case.get("max_rank", 1))
    rank = first_expected_rank(hits, expected)
    false_positives = sum(1 for hit in hits if hit_matches_any(hit, forbidden))
    failures = []
    if expected and (rank is None or rank > max_rank):
        failures.append(f"rank={rank} max_rank={max_rank}")
    if false_positives:
        failures.append(f"false_positives={false_positives}")
    if "max_results" in case and len(hits) > int(case["max_results"]):
        failures.append(f"results={len(hits)} max_results={case['max_results']}")
    if "truncated" in case and bool(payload.get("truncated")) != bool(case["truncated"]):
        failures.append(f"truncated={payload.get('truncated')}")
    if "degraded_reason" in case and payload.get("degraded_reason") != case["degraded_reason"]:
        failures.append(f"degraded_reason={payload.get('degraded_reason')}")
    if "degraded_reason_contains" in case:
        degraded_reason = str(payload.get("degraded_reason") or "")
        if str(case["degraded_reason_contains"]) not in degraded_reason:
            failures.append(f"degraded_reason={degraded_reason}")
    passed = not failures
    if case.get("expect_empty"):
        passed = len(hits) == 0
        rank = 0 if passed else None
        if not passed:
            failures.append(f"expected_empty_results={len(hits)}")
    return CaseObservation(
        case_id=case["id"],
        repository=fixture_name,
        passed=passed,
        rank=rank,
        max_rank=max_rank,
        false_positive_count=false_positives,
        message=f"results={len(hits)} rank={rank}" + (
            f" failures={'; '.join(failures)}" if failures else ""
        ),
        objective=str(case.get("objective", "foundational_capability")),
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
    for field in ("scope_id", "root_id", "relative_path", "file_name", "extension", "status"):
        if field in pattern and hit.get(field) != pattern[field]:
            return False
    if "parent_dir" in pattern and hit.get("parent_dir") != pattern["parent_dir"]:
        return False
    for field, contains_field in (
        ("path", "path_contains"),
        ("relative_path", "relative_path_contains"),
        ("file_name", "file_name_contains"),
        ("parent_dir", "parent_dir_contains"),
    ):
        if contains_field in pattern and str(pattern[contains_field]) not in str(hit.get(field, "")):
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
