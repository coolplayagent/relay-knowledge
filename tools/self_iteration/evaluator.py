"""Build and repository-retrieval evaluation for self-iteration candidates."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from statistics import median
from typing import Any

from scoring import CaseObservation, EvaluationObservation, GateObservation, MetricObservation


@dataclass(frozen=True)
class CommandResult:
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


@dataclass
class EvaluationRun:
    observation: EvaluationObservation
    report: dict[str, Any]


@dataclass(frozen=True)
class EvaluatorConfig:
    workspace: Path
    state_work_dir: Path
    cases_path: Path
    profile: str = "full"
    command_timeout_seconds: int = 900
    keep_workdirs: bool = False


def evaluate_candidate(config: EvaluatorConfig, generated_diff: bool) -> EvaluationRun:
    cases_config = load_cases(config.cases_path)
    run_home = config.state_work_dir / "home"
    if run_home.exists() and not config.keep_workdirs:
        shutil.rmtree(run_home)
    run_home.mkdir(parents=True, exist_ok=True)

    commands: list[CommandResult] = []
    gates: list[GateObservation] = []
    case_observations: list[CaseObservation] = []
    metrics: list[MetricObservation] = []
    repo_reports: list[dict[str, Any]] = []

    quality_commands = quality_gate_commands(config.profile)
    for gate_name, command, timeout in quality_commands:
        result = run_command(gate_name, command, config.workspace, None, timeout)
        commands.append(result)
        gates.append(result.gate())
        metrics.append(
            MetricObservation(
                name=f"{gate_name}_ms",
                value=result.duration_ms,
                budget=quality_budget_ms(gate_name),
                key=gate_name == "cargo_build_release",
            )
        )
        if not result.passed:
            return finish_evaluation(
                generated_diff, gates, case_observations, metrics, commands, repo_reports, run_home, config
            )

    if config.profile == "smoke":
        return finish_evaluation(
            generated_diff, gates, case_observations, metrics, commands, repo_reports, run_home, config
        )

    binary = config.workspace / "target" / "release" / "relay-knowledge"
    env = dict(os.environ)
    env["RELAY_KNOWLEDGE_HOME"] = str(run_home)
    env.setdefault("RUST_BACKTRACE", "1")

    repositories = cases_config.get("repositories", {})
    for repo_name, repo_config in repositories.items():
        if repo_config.get("profile") == "exhaustive" and config.profile != "exhaustive":
            continue
        repo_report = evaluate_repository(
            binary=binary,
            workspace=config.workspace,
            env=env,
            repo_name=repo_name,
            repo_config=repo_config,
            all_cases=cases_config.get("query_cases", []),
            timeout=config.command_timeout_seconds,
        )
        repo_reports.append(repo_report)
        commands.extend(repo_report["commands"])
        gates.extend(result.gate() for result in repo_report["commands"])
        case_observations.extend(repo_report["cases"])
        metrics.extend(repo_report["metrics"])

    return finish_evaluation(
        generated_diff, gates, case_observations, metrics, commands, repo_reports, run_home, config
    )


def evaluate_repository(
    binary: Path,
    workspace: Path,
    env: dict[str, str],
    repo_name: str,
    repo_config: dict[str, Any],
    all_cases: list[dict[str, Any]],
    timeout: int,
) -> dict[str, Any]:
    path = Path(repo_config["path"])
    alias = repo_config.get("alias", repo_name)
    ref_selector = repo_config.get("ref", "HEAD")
    scope = repo_config.get("scope", "all")
    repo_cases = [case for case in all_cases if case.get("repository") == repo_name]
    commands: list[CommandResult] = []
    case_observations: list[CaseObservation] = []
    metrics: list[MetricObservation] = []

    if not path.exists():
        missing = CommandResult(
            name=f"{repo_name}_repository_exists",
            command=["test", "-d", str(path)],
            exit_code=1,
            duration_ms=0,
            stdout="",
            stderr=f"repository path is missing: {path}",
        )
        commands.append(missing)
        return serializable_repo_report(repo_name, commands, case_observations, metrics, {}, scope)
    if scope != "all":
        invalid = CommandResult(
            name=f"{repo_name}_scope_is_all",
            command=["validate", "scope", str(scope)],
            exit_code=1,
            duration_ms=0,
            stdout="",
            stderr=f"self-iteration repositories must use full scope=all, got: {scope}",
        )
        commands.append(invalid)
        return serializable_repo_report(repo_name, commands, case_observations, metrics, {}, scope)

    register = run_command(
        f"{repo_name}_register",
        register_command(binary, path, alias, repo_config),
        workspace,
        env,
        timeout,
    )
    commands.append(register)
    if not register.passed:
        return serializable_repo_report(repo_name, commands, case_observations, metrics, {}, scope)

    index = run_command(
        f"{repo_name}_index",
        [str(binary), "repo", "index", alias, "--ref", ref_selector, "--format", "json"],
        workspace,
        env,
        timeout,
    )
    commands.append(index)
    index_json = parse_json_output(index.stdout) if index.passed else {}
    metrics.append(
        MetricObservation(
            name=f"{repo_name}_index_ms",
            value=index.duration_ms,
            budget=float(repo_config.get("index_budget_ms", 0)) or None,
            key=True,
        )
    )
    if not index.passed:
        return serializable_repo_report(repo_name, commands, case_observations, metrics, index_json, scope)

    query_durations: list[int] = []
    for case in repo_cases:
        query = run_command(
            f"{repo_name}_{case['id']}",
            query_command(binary, alias, ref_selector, case),
            workspace,
            env,
            timeout,
        )
        commands.append(query)
        query_durations.append(query.duration_ms)
        case_observations.append(score_query_case(repo_name, case, query))

    if query_durations:
        metrics.append(
            MetricObservation(
                name=f"{repo_name}_query_p50_ms",
                value=float(median(query_durations)),
                budget=float(repo_config.get("query_p50_budget_ms", 0)) or None,
                key=False,
            )
        )
        metrics.append(
            MetricObservation(
                name=f"{repo_name}_query_p95_ms",
                value=float(percentile(query_durations, 95)),
                budget=float(repo_config.get("query_p95_budget_ms", 0)) or None,
                key=True,
            )
        )

    return serializable_repo_report(repo_name, commands, case_observations, metrics, index_json, scope)


def quality_gate_commands(profile: str) -> list[tuple[str, list[str], int]]:
    if profile == "smoke":
        return [("cargo_fmt_check", ["cargo", "fmt", "--all", "--", "--check"], 120)]
    return [
        ("cargo_build_release", ["cargo", "build", "--release"], 1200),
        ("cargo_fmt_check", ["cargo", "fmt", "--all", "--", "--check"], 120),
        (
            "cargo_clippy",
            ["cargo", "clippy", "--all-targets", "--all-features", "--", "-D", "warnings"],
            1200,
        ),
        ("cargo_test", ["cargo", "test", "--all-targets", "--all-features"], 1200),
    ]


def quality_budget_ms(name: str) -> float | None:
    budgets = {
        "cargo_build_release": 180_000.0,
        "cargo_fmt_check": 20_000.0,
        "cargo_clippy": 180_000.0,
        "cargo_test": 240_000.0,
    }
    return budgets.get(name)


def register_command(
    binary: Path,
    repo_path: Path,
    alias: str,
    repo_config: dict[str, Any],
) -> list[str]:
    command = [str(binary), "repo", "register", str(repo_path), "--alias", alias]
    if repo_config.get("scope", "all") != "all":
        for path_filter in repo_config.get("path_filters", []):
            command.extend(["--path", path_filter])
        for language_filter in repo_config.get("language_filters", []):
            command.extend(["--language", language_filter])
    command.extend(["--format", "json"])
    return command


def query_command(
    binary: Path,
    alias: str,
    ref_selector: str,
    case: dict[str, Any],
) -> list[str]:
    command = [
        str(binary),
        "repo",
        "query",
        alias,
        "--query",
        case["query"],
        "--kind",
        case["kind"],
        "--ref",
        case.get("ref", ref_selector),
        "--freshness",
        "wait-until-fresh",
        "--limit",
        str(case.get("limit", 10)),
    ]
    for path_filter in case.get("path_filters", []):
        command.extend(["--path", path_filter])
    for language_filter in case.get("language_filters", []):
        command.extend(["--language", language_filter])
    command.extend(["--format", "json"])
    return command


def score_query_case(repo_name: str, case: dict[str, Any], result: CommandResult) -> CaseObservation:
    if not result.passed:
        return CaseObservation(
            case_id=case["id"],
            repository=repo_name,
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
        repository=repo_name,
        passed=passed,
        rank=rank,
        max_rank=max_rank,
        false_positive_count=false_positives,
        message=f"results={len(hits)} rank={rank}",
    )


def first_expected_rank(hits: list[dict[str, Any]], expected: list[dict[str, Any]]) -> int | None:
    for index, hit in enumerate(hits, start=1):
        if hit_matches_any(hit, expected):
            return index
    return None


def hit_matches_any(hit: dict[str, Any], patterns: list[dict[str, Any]]) -> bool:
    return any(hit_matches(hit, pattern) for pattern in patterns)


def hit_matches(hit: dict[str, Any], pattern: dict[str, Any]) -> bool:
    if "path" in pattern and hit.get("path") != pattern["path"]:
        return False
    if "line_start" in pattern:
        line_range = hit.get("line_range", {})
        start = int(line_range.get("start", -1))
        end = int(line_range.get("end", -1))
        expected = int(pattern["line_start"])
        if not (start <= expected <= end or start == expected):
            return False
    if "edge_resolution_state" in pattern:
        if hit.get("edge_resolution_state") != pattern["edge_resolution_state"]:
            return False
    if "edge_target_hint" in pattern:
        target = hit.get("edge_target_hint") or ""
        if pattern["edge_target_hint"] not in target:
            return False
    if "excerpt_contains" in pattern:
        if pattern["excerpt_contains"] not in hit.get("excerpt", ""):
            return False
    return True


def finish_evaluation(
    generated_diff: bool,
    gates: list[GateObservation],
    case_observations: list[CaseObservation],
    metrics: list[MetricObservation],
    commands: list[CommandResult],
    repo_reports: list[dict[str, Any]],
    run_home: Path,
    config: EvaluatorConfig,
) -> EvaluationRun:
    if run_home.exists() and not config.keep_workdirs:
        shutil.rmtree(run_home)
    observation = EvaluationObservation(
        gates=gates,
        cases=case_observations,
        metrics=metrics,
        generated_diff=generated_diff,
    )
    report = {
        "profile": config.profile,
        "generated_diff": generated_diff,
        "gates": [gate.__dict__ for gate in gates],
        "cases": [case.__dict__ for case in case_observations],
        "metrics": [metric.__dict__ for metric in metrics],
        "commands": [serializable_command(command) for command in commands],
        "repositories": [serializable_repository_report(report) for report in repo_reports],
    }
    return EvaluationRun(observation=observation, report=report)


def run_command(
    name: str,
    command: list[str],
    cwd: Path,
    env: dict[str, str] | None,
    timeout: int,
) -> CommandResult:
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
        return CommandResult(
            name=name,
            command=command,
            exit_code=completed.returncode,
            duration_ms=int((time.monotonic() - started) * 1000),
            stdout=completed.stdout,
            stderr=completed.stderr,
        )
    except subprocess.TimeoutExpired as error:
        return CommandResult(
            name=name,
            command=command,
            exit_code=124,
            duration_ms=int((time.monotonic() - started) * 1000),
            stdout=error.stdout or "",
            stderr=(error.stderr or "") + f"\ntimeout after {timeout}s",
        )


def load_cases(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def parse_json_output(stdout: str) -> dict[str, Any]:
    for line in reversed(stdout.splitlines()):
        line = line.strip()
        if not line:
            continue
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


def serializable_command(result: CommandResult) -> dict[str, Any]:
    return {
        "name": result.name,
        "command": result.command,
        "exit_code": result.exit_code,
        "duration_ms": result.duration_ms,
        "stdout_tail": result.stdout[-4000:],
        "stderr_tail": result.stderr[-4000:],
    }


def serializable_repo_report(
    repo_name: str,
    commands: list[CommandResult],
    cases: list[CaseObservation],
    metrics: list[MetricObservation],
    index_json: dict[str, Any],
    scope: str,
) -> dict[str, Any]:
    return {
        "repository": repo_name,
        "commands": commands,
        "cases": cases,
        "metrics": metrics,
        "scope": scope,
        "index_summary": index_json.get("summary", {}),
    }


def serializable_repository_report(report: dict[str, Any]) -> dict[str, Any]:
    return {
        "repository": report["repository"],
        "scope": report.get("scope", "all"),
        "commands": [serializable_command(command) for command in report["commands"]],
        "cases": [case.__dict__ for case in report["cases"]],
        "metrics": [metric.__dict__ for metric in report["metrics"]],
        "index_summary": report.get("index_summary", {}),
    }
