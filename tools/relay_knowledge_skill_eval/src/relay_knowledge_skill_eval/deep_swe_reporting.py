from __future__ import annotations

import json
from collections.abc import Mapping
from datetime import datetime
from pathlib import Path
from typing import Any

from pier.models.trial.result import TimingInfo, TrialResult

from relay_knowledge_skill_eval.models import (
    Condition,
    EvalResult,
    RunOutcome,
    SweBenchDiagnostics,
    TimingMetrics,
    TokenUsage,
    ToolUsage,
)
from relay_knowledge_skill_eval.pi_events import recover_truncated_trace_usage
from relay_knowledge_skill_eval.reporting import _atomic_write_text, build_report
from relay_knowledge_skill_eval.security import SecretRedactor

DEEP_SWE_PIER_REVISION = "0daf53d3599e58c4506cf0bcff5e12c77dc282d2"


def load_deep_swe_results(results_root: Path) -> list[EvalResult]:
    results: list[EvalResult] = []
    for result_path in sorted(results_root.rglob("result.json")):
        try:
            trial = TrialResult.model_validate_json(
                result_path.read_text(encoding="utf-8")
            )
            results.append(_convert_trial(trial, result_path.parent))
        except (ValueError, OSError, json.JSONDecodeError):
            continue
    return results


def write_deep_swe_report(
    results_root: Path,
    *,
    skill_version: str,
    skill_sha256: str,
    expected_results: int = 226,
    output_path: Path | None = None,
) -> Path:
    results = load_deep_swe_results(results_root)
    infrastructure_failures = sum(
        result.outcome is RunOutcome.INFRA_ERROR for result in results
    )
    completed_results = len(results) - infrastructure_failures
    final = completed_results >= expected_results
    report = build_report(
        results,
        paired_bootstrap_samples=10_000 if final else 0,
    )
    report.update(
        {
            "benchmark": "DeepSWE",
            "metadata": {
                "active_suite": "deep-swe-113",
                "expected_results": expected_results,
                "completed_results": completed_results,
                "recorded_results": len(results),
                "infrastructure_failures": infrastructure_failures,
                "final": final,
                "agent_timeout_seconds": 3600,
                "condition_execution_mode": "parallel-one-per-condition",
                "persisted_output_budget_bytes_per_stream": 64 * 1024 * 1024,
                "model": "deepseek-v4-flash",
                "thinking": "high",
                "pi_version": "0.80.3",
                "pier_revision": DEEP_SWE_PIER_REVISION,
                "skill_version": skill_version,
                "skill_sha256": skill_sha256,
            },
            "results": [_public_result(result) for result in results],
        }
    )
    output_path = output_path or results_root.parent / "report.json"
    _atomic_write_text(
        output_path,
        json.dumps(report, ensure_ascii=False, indent=2),
    )
    return output_path


def _convert_trial(trial: TrialResult, trial_dir: Path) -> EvalResult:
    kwargs = trial.config.agent.kwargs
    condition = Condition(str(kwargs.get("condition", "baseline")))
    metadata = _mapping(trial.agent_result.metadata if trial.agent_result else None)
    tokens = TokenUsage.model_validate(_mapping(metadata.get("tokens")))
    tools = ToolUsage.model_validate(_mapping(metadata.get("tools")))
    outcome, error = trial_outcome(trial, trial_dir)
    if outcome is RunOutcome.TIMED_OUT and tokens.total == 0 and tools.calls == 0:
        recovered_tokens, recovered_tools = recover_truncated_trace_usage(
            sorted((trial_dir / "agent").glob("pi-trace-[0-9][0-9].jsonl.gz"))
        )
        if recovered_tokens.requests or recovered_tools.calls:
            tokens, tools = recovered_tokens, recovered_tools
    rewards = trial.verifier_result.rewards if trial.verifier_result else {}
    resolved = bool(rewards and float(rewards.get("reward", 0)) == 1.0)
    patch_path = trial_dir / "artifacts" / "model.patch"
    patch_exists = patch_path.exists() and patch_path.stat().st_size > 0
    return EvalResult(
        instance_id=trial.task_name.split("/")[-1],
        condition=condition,
        outcome=outcome,
        error=SecretRedactor().redact(error),
        prompt_path=str(trial_dir / "agent" / "prompt.txt"),
        trace_path=str(trial_dir / "agent" / "pi-trace.jsonl.gz"),
        patch_path=str(patch_path),
        index_log_path=(
            str(trial_dir / "agent" / "relay-index.jsonl")
            if condition is Condition.SKILL
            else ""
        ),
        tokens=tokens,
        tools=tools,
        timings=TimingMetrics(
            image_prepare_seconds=_duration(trial.environment_setup),
            preindex_seconds=float(metadata.get("preindex_seconds", 0) or 0),
            agent_seconds=_duration(trial.agent_execution),
            scoring_seconds=_duration(trial.verifier),
            end_to_end_seconds=_datetime_duration(trial.started_at, trial.finished_at),
        ),
        swebench=SweBenchDiagnostics(
            completed=outcome is RunOutcome.COMPLETED,
            resolved=resolved,
            resolution_status="full" if resolved else "none",
            patch_exists=patch_exists,
            patch_applied=bool(rewards and not rewards.get("apply_failed", 0)),
            report_path=str(trial_dir / "verifier" / "reward.json"),
            test_output_path=str(trial_dir / "verifier" / "test-stdout.txt"),
        ),
    )


def trial_outcome(
    trial: TrialResult, trial_dir: Path | None = None
) -> tuple[RunOutcome, str]:
    candidate_error = _verifier_candidate_error(trial_dir)
    if candidate_error:
        return RunOutcome.COMPLETED, candidate_error
    exception = trial.exception_info
    verifier_error = _verifier_infrastructure_error(trial_dir)
    if exception is not None:
        name = exception.exception_type
        message = exception.exception_message
        if name in {"AgentTimeoutError", "TimeoutError"} and trial.agent_execution:
            return RunOutcome.TIMED_OUT, message
        if name in {"DeepSweConfigurationError", "DeepSweTransportError"}:
            return RunOutcome.INFRA_ERROR, message
        if verifier_error:
            return RunOutcome.INFRA_ERROR, verifier_error
        if trial.agent_execution is not None and (
            name.startswith("Agent") or trial.verifier is None
        ):
            return RunOutcome.AGENT_ERROR, message
        if trial.verifier is not None and trial.verifier_result is None:
            return RunOutcome.INFRA_ERROR, message
        if trial.agent_execution is not None:
            return RunOutcome.AGENT_ERROR, message
        return RunOutcome.INFRA_ERROR, message
    if trial.verifier is not None and not _candidate_patch_was_collected(trial_dir):
        return RunOutcome.INFRA_ERROR, "DeepSWE candidate patch was not collected"
    if verifier_error:
        return RunOutcome.INFRA_ERROR, verifier_error
    if trial.verifier_result is not None:
        return RunOutcome.COMPLETED, ""
    return RunOutcome.INFRA_ERROR, "DeepSWE verifier produced no result"


def _candidate_patch_was_collected(trial_dir: Path | None) -> bool:
    """Require the separate verifier handoff artifact, including empty patches."""
    if trial_dir is None:
        return False
    return (trial_dir / "artifacts" / "model.patch").is_file()


def _verifier_infrastructure_error(trial_dir: Path | None) -> str:
    if trial_dir is None:
        return ""
    markers = {
        "cannot execute: required file not found": (
            "DeepSWE verifier script could not execute"
        ),
    }
    for name in ("test-stdout.txt", "test-stderr.txt"):
        path = trial_dir / "verifier" / name
        try:
            output = path.read_text(encoding="utf-8", errors="replace").lower()
        except OSError:
            continue
        for marker, error in markers.items():
            if marker in output:
                return error
    return ""


def _verifier_candidate_error(trial_dir: Path | None) -> str:
    if trial_dir is None:
        return ""
    for name in ("test-stdout.txt", "test-stderr.txt"):
        path = trial_dir / "verifier" / name
        try:
            output = path.read_text(encoding="utf-8", errors="replace").lower()
        except OSError:
            continue
        if "test.patch failed to apply" in output:
            return "DeepSWE test patch conflicted with the candidate patch"
    return ""


def _duration(value: TimingInfo | None) -> float:
    if value is None:
        return 0.0
    return _datetime_duration(value.started_at, value.finished_at)


def _datetime_duration(started: datetime | None, finished: datetime | None) -> float:
    if started is None or finished is None:
        return 0.0
    return max(0.0, (finished - started).total_seconds())


def _mapping(value: object) -> Mapping[str, Any]:
    return value if isinstance(value, Mapping) else {}


def _public_result(result: EvalResult) -> dict[str, object]:
    return {
        "instance_id": result.instance_id,
        "condition": result.condition.value,
        "outcome": result.outcome.value,
        "resolved": result.benchmark_resolved,
        "patch_exists": result.swebench.patch_exists,
        "tokens": result.tokens.model_dump(),
        "tools": result.tools.model_dump(),
        "timings": result.timings.model_dump(),
    }
