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

from file_fixture_eval import evaluate_file_fixtures
from llm_judge import evaluate_research_judge_suite
from ranked_case_scoring import (
    RankedHitAssessment,
    assess_ranked_hits,
    hit_matches_any,
)
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


def evaluate_candidate(
    config: EvaluatorConfig,
    generated_diff: bool,
    candidate_diff: str = "",
) -> EvaluationRun:
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

    file_report = evaluate_file_fixtures(
        binary=binary,
        workspace=config.workspace,
        env=env,
        run_home=run_home,
        fixtures=cases_config.get("file_fixtures", {}),
        all_cases=cases_config.get("file_query_cases", []),
        timeout=config.command_timeout_seconds,
    )
    commands.extend(file_report["commands"])
    gates.extend(result.gate() for result in file_report["commands"])
    case_observations.extend(file_report["cases"])
    metrics.extend(file_report["metrics"])

    semantic_vector_config = cases_config.get("semantic_vector_suite")
    if isinstance(semantic_vector_config, dict):
        semantic_vector_report = evaluate_semantic_vector_suite(
            binary=binary,
            workspace=config.workspace,
            env=env,
            suite_config=semantic_vector_config,
            timeout=config.command_timeout_seconds,
        )
        repo_reports.append(semantic_vector_report)
        commands.extend(semantic_vector_report["commands"])
        gates.extend(result.gate() for result in semantic_vector_report["commands"])
        case_observations.extend(semantic_vector_report["cases"])
        metrics.extend(semantic_vector_report["metrics"])

    research_judge_config = cases_config.get("research_judge_suite")
    if isinstance(research_judge_config, dict):
        research_judge_report = evaluate_research_judge_suite(
            workspace=config.workspace,
            run_home=run_home,
            env=env,
            suite_config=research_judge_config,
            generated_diff=generated_diff,
            candidate_diff=candidate_diff,
            gates=gates,
            cases=case_observations,
            metrics=metrics,
            repo_reports=repo_reports,
        )
        repo_reports.append(research_judge_report)
        gates.extend(research_judge_report.get("gates", []))
        case_observations.extend(research_judge_report.get("cases", []))
        metrics.extend(research_judge_report.get("metrics", []))

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
    metrics.append(
        MetricObservation(
            name=f"{repo_name}_register_index_ms",
            value=register.duration_ms + index.duration_ms,
            budget=float(repo_config.get("register_index_budget_ms", 0)) or None,
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


def evaluate_semantic_vector_suite(
    binary: Path,
    workspace: Path,
    env: dict[str, str],
    suite_config: dict[str, Any],
    timeout: int,
) -> dict[str, Any]:
    scope = suite_config.get("source_scope", "self-iteration-semantic-vector")
    commands: list[CommandResult] = []
    case_observations: list[CaseObservation] = []
    metrics: list[MetricObservation] = []
    runtime_profile = semantic_vector_runtime_profile(env)

    if runtime_profile["external_requested"]:
        env_check = semantic_vector_env_check(runtime_profile)
        commands.append(env_check)
        if not env_check.passed:
            return serializable_repo_report(
                "semantic_vector",
                commands,
                case_observations,
                metrics,
                {"summary": runtime_profile},
                scope,
            )
        if suite_config.get("probe_provider_when_external", True):
            probe = run_provider_probe(binary, workspace, env, timeout)
            commands.append(probe)
            metrics.append(
                MetricObservation(
                    name="semantic_vector_provider_probe_ms",
                    value=probe.duration_ms,
                    budget=float(suite_config.get("provider_probe_budget_ms", 0)) or None,
                    key=True,
                )
            )

    for index, evidence in enumerate(suite_config.get("evidence", []), start=1):
        ingest = run_command(
            f"semantic_vector_ingest_{index}",
            semantic_vector_ingest_command(binary, scope, evidence),
            workspace,
            env,
            timeout,
        )
        commands.append(ingest)
        if not ingest.passed:
            return serializable_repo_report(
                "semantic_vector",
                commands,
                case_observations,
                metrics,
                {"summary": runtime_profile},
                scope,
            )

    refresh = run_command(
        "semantic_vector_index_refresh",
        [
            str(binary),
            "index",
            "refresh",
            "--kind",
            "semantic",
            "--kind",
            "vector",
            "--format",
            "json",
        ],
        workspace,
        env,
        timeout,
    )
    commands.append(refresh)
    metrics.append(
        MetricObservation(
            name="semantic_vector_refresh_ms",
            value=refresh.duration_ms,
            budget=float(suite_config.get("refresh_budget_ms", 0)) or None,
            key=True,
        )
    )
    if not refresh.passed:
        return serializable_repo_report(
            "semantic_vector",
            commands,
            case_observations,
            metrics,
            parse_json_output(refresh.stdout) if refresh.passed else {"summary": runtime_profile},
            scope,
        )

    query_durations: list[int] = []
    for case in suite_config.get("query_cases", []):
        query = run_command(
            f"semantic_vector_{case['id']}",
            semantic_vector_query_command(binary, scope, case),
            workspace,
            env,
            timeout,
        )
        commands.append(query)
        query_durations.append(query.duration_ms)
        case_observations.append(score_semantic_vector_case(case, query))

    if query_durations:
        metrics.append(
            MetricObservation(
                name="semantic_vector_query_p50_ms",
                value=float(median(query_durations)),
                budget=float(suite_config.get("query_p50_budget_ms", 0)) or None,
                key=False,
            )
        )
        metrics.append(
            MetricObservation(
                name="semantic_vector_query_p95_ms",
                value=float(percentile(query_durations, 95)),
                budget=float(suite_config.get("query_p95_budget_ms", 0)) or None,
                key=True,
            )
        )

    return serializable_repo_report(
        "semantic_vector",
        commands,
        case_observations,
        metrics,
        {"summary": runtime_profile},
        scope,
    )


def semantic_vector_runtime_profile(env: dict[str, str]) -> dict[str, Any]:
    semantic_backend = normalized_env_value(env, "RELAY_KNOWLEDGE_SEMANTIC_BACKEND", "local")
    vector_backend = normalized_env_value(env, "RELAY_KNOWLEDGE_VECTOR_BACKEND", "local")
    external_requested = "external" in {semantic_backend, vector_backend}
    required = [
        "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL",
        "RELAY_KNOWLEDGE_EMBEDDING_API_KEY",
        "RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL",
        "RELAY_KNOWLEDGE_EMBEDDING_DIMENSION",
    ]
    missing = [name for name in required if external_requested and not env.get(name, "").strip()]
    return {
        "semantic_backend": semantic_backend,
        "vector_backend": vector_backend,
        "external_requested": external_requested,
        "missing_external_env": missing,
        "llm_provider": normalized_env_value(env, "RELAY_KNOWLEDGE_LLM_PROVIDER", "openai_compatible"),
        "text_embedding_model": env.get("RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL", ""),
        "embedding_dimension": env.get("RELAY_KNOWLEDGE_EMBEDDING_DIMENSION", ""),
        "embedding_base_url_configured": bool(env.get("RELAY_KNOWLEDGE_EMBEDDING_BASE_URL", "").strip()),
        "embedding_api_key_configured": bool(env.get("RELAY_KNOWLEDGE_EMBEDDING_API_KEY", "").strip()),
    }


def normalized_env_value(env: dict[str, str], name: str, default: str) -> str:
    value = env.get(name, "").strip().lower()
    return value or default


def semantic_vector_env_check(profile: dict[str, Any]) -> CommandResult:
    missing = profile.get("missing_external_env", [])
    passed = not missing
    message = (
        "external semantic/vector environment is configured"
        if passed
        else "missing external semantic/vector env: " + ", ".join(str(name) for name in missing)
    )
    return CommandResult(
        name="semantic_vector_external_env",
        command=["validate", "semantic-vector-env"],
        exit_code=0 if passed else 1,
        duration_ms=0,
        stdout=json.dumps(profile, sort_keys=True),
        stderr="" if passed else message,
    )


def run_provider_probe(
    binary: Path,
    workspace: Path,
    env: dict[str, str],
    timeout: int,
) -> CommandResult:
    raw = run_command(
        "semantic_vector_provider_probe",
        [str(binary), "provider", "probe", "--format", "json"],
        workspace,
        env,
        timeout,
    )
    if not raw.passed:
        return raw
    payload = parse_json_output(raw.stdout)
    if payload.get("ok") is True:
        return raw
    message = provider_probe_message(payload, raw)
    return CommandResult(
        name=raw.name,
        command=raw.command,
        exit_code=1,
        duration_ms=raw.duration_ms,
        stdout=raw.stdout,
        stderr=message,
    )


def provider_probe_message(payload: dict[str, Any], raw: CommandResult) -> str:
    code = payload.get("error_code") or "provider_probe_failed"
    message = payload.get("error_message") or last_output_line(raw.stdout, raw.stderr)
    return f"{code}: {message}"


def semantic_vector_ingest_command(
    binary: Path,
    scope: str,
    evidence: dict[str, Any],
) -> list[str]:
    command = [
        str(binary),
        "ingest",
        "--source",
        scope,
        "--content",
        str(evidence["content"]),
    ]
    for entity in evidence.get("entities", []):
        command.extend(["--entity", str(entity)])
    command.extend(["--format", "json"])
    return command


def semantic_vector_query_command(
    binary: Path,
    scope: str,
    case: dict[str, Any],
) -> list[str]:
    return [
        str(binary),
        "query",
        case["query"],
        "--source",
        scope,
        "--freshness",
        "wait-until-fresh",
        "--limit",
        str(case.get("limit", 10)),
        "--format",
        "json",
    ]


def score_semantic_vector_case(case: dict[str, Any], result: CommandResult) -> CaseObservation:
    if not result.passed:
        return CaseObservation(
            case_id=case["id"],
            repository="semantic_vector",
            passed=False,
            message=last_output_line(result.stdout, result.stderr),
            objective="semantic_vector",
        )
    payload = parse_json_output(result.stdout)
    hits = payload.get("results", [])
    expected = case.get("expected", [])
    forbidden = case.get("forbidden", [])
    max_rank = int(case.get("max_rank", 1))
    rank, matched_hit = first_expected_hit(hits, expected)
    false_positives = sum(1 for hit in hits if hit_matches_any(hit, forbidden))
    missing_sources = missing_required_sources(case, matched_hit, hits)
    missing_backends = missing_required_backend_states(case, payload)
    passed = (
        (not expected or (rank is not None and rank <= max_rank))
        and false_positives == 0
        and not missing_sources
        and not missing_backends
    )
    if case.get("expect_empty"):
        passed = len(hits) == 0
        rank = 0 if passed else None
    return CaseObservation(
        case_id=case["id"],
        repository="semantic_vector",
        passed=passed,
        rank=rank,
        max_rank=max_rank,
        false_positive_count=false_positives,
        message=semantic_vector_case_message(
            hits,
            rank,
            matched_hit,
            missing_sources,
            missing_backends,
            payload,
        ),
        objective="semantic_vector",
    )


def first_expected_hit(
    hits: list[dict[str, Any]],
    expected: list[dict[str, Any]],
) -> tuple[int | None, dict[str, Any] | None]:
    for index, hit in enumerate(hits, start=1):
        if hit_matches_any(hit, expected):
            return index, hit
    return None, None


def missing_required_sources(
    case: dict[str, Any],
    matched_hit: dict[str, Any] | None,
    hits: list[dict[str, Any]],
) -> list[str]:
    required = {str(source) for source in case.get("required_sources", [])}
    if not required:
        return []
    observed = hit_sources(matched_hit) if matched_hit else all_hit_sources(hits)
    return sorted(required - observed)


def hit_sources(hit: dict[str, Any] | None) -> set[str]:
    if not isinstance(hit, dict):
        return set()
    return {str(source) for source in hit.get("retriever_sources", [])}


def all_hit_sources(hits: list[dict[str, Any]]) -> set[str]:
    sources: set[str] = set()
    for hit in hits:
        sources.update(hit_sources(hit))
    return sources


def missing_required_backend_states(
    case: dict[str, Any],
    payload: dict[str, Any],
) -> list[str]:
    required = case.get("required_backend_states", {})
    if not isinstance(required, dict):
        return []
    states = {
        str(status.get("source")): str(status.get("state"))
        for status in payload.get("backend_statuses", [])
        if isinstance(status, dict)
    }
    missing: list[str] = []
    for source, allowed in required.items():
        allowed_states = {str(state) for state in allowed}
        current = states.get(str(source))
        if current not in allowed_states:
            missing.append(f"{source}:{current or 'missing'}")
    return missing


def semantic_vector_case_message(
    hits: list[dict[str, Any]],
    rank: int | None,
    matched_hit: dict[str, Any] | None,
    missing_sources: list[str],
    missing_backends: list[str],
    payload: dict[str, Any],
) -> str:
    backend_states = {
        str(status.get("source")): str(status.get("state"))
        for status in payload.get("backend_statuses", [])
        if isinstance(status, dict)
    }
    sources = sorted(hit_sources(matched_hit) if matched_hit else all_hit_sources(hits))
    return (
        f"results={len(hits)} rank={rank} sources={sources} "
        f"backend_states={backend_states} missing_sources={missing_sources} "
        f"missing_backends={missing_backends}"
    )


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
    objective = repository_case_objective(case)
    if not result.passed:
        return CaseObservation(
            case_id=case["id"],
            repository=repo_name,
            passed=False,
            message=last_output_line(result.stdout, result.stderr),
            objective=objective,
        )
    payload = parse_json_output(result.stdout)
    hits = payload.get("results", [])
    expected = case.get("expected", [])
    forbidden = case.get("forbidden", [])
    max_rank = int(case.get("max_rank", 1))
    assessment = assess_ranked_hits(case, hits, expected, forbidden)
    rank = assessment.rank
    passed = not assessment.failures
    if case.get("expect_empty"):
        passed = len(hits) == 0
        rank = 0 if passed else None
        assessment = RankedHitAssessment(
            rank=rank,
            false_positive_count=0,
            score=1.0 if passed else 0.0,
            details="expect_empty",
            failures=[] if passed else [f"expected_empty_results={len(hits)}"],
        )
    return CaseObservation(
        case_id=case["id"],
        repository=repo_name,
        passed=passed,
        rank=rank,
        max_rank=max_rank,
        false_positive_count=assessment.false_positive_count,
        message=f"results={len(hits)} rank={rank} {assessment.details}".strip(),
        objective=objective,
        score_override=assessment.score,
    )


def repository_case_objective(case: dict[str, Any]) -> str:
    explicit = str(case.get("objective", "")).strip()
    if explicit:
        return explicit
    kind = str(case.get("kind", "")).strip().lower()
    case_id = str(case.get("id", "")).strip().lower()
    competitive_kinds = {"hybrid", "callers", "callees"}
    competitive_markers = ("hybrid", "fuzzy", "full_scope", "fanout", "callers", "callees")
    if kind in competitive_kinds or any(marker in case_id for marker in competitive_markers):
        return "competitive_capability"
    return "foundational_capability"


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
    config = json.loads(path.read_text(encoding="utf-8"))
    include_files = config.pop("include_files", [])
    for include_file in include_files:
        included = load_cases(path.parent / include_file)
        merge_case_config(config, included)
    return config


def merge_case_config(config: dict[str, Any], included: dict[str, Any]) -> None:
    for key, value in included.items():
        if isinstance(value, list):
            target = config.setdefault(key, [])
            if not isinstance(target, list):
                raise ValueError(f"cannot merge list case config into non-list key: {key}")
            target.extend(value)
        elif isinstance(value, dict):
            target = config.setdefault(key, {})
            if not isinstance(target, dict):
                raise ValueError(f"cannot merge map case config into non-map key: {key}")
            merge_nested_map(target, value)
        else:
            config[key] = value


def merge_nested_map(target: dict[str, Any], included: dict[str, Any]) -> None:
    for key, value in included.items():
        existing = target.get(key)
        if isinstance(existing, dict) and isinstance(value, dict):
            merge_nested_map(existing, value)
        else:
            target[key] = value


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
        "gates": [gate.__dict__ for gate in report.get("gates", [])],
        "cases": [case.__dict__ for case in report["cases"]],
        "metrics": [metric.__dict__ for metric in report["metrics"]],
        "index_summary": report.get("index_summary", {}),
    }
