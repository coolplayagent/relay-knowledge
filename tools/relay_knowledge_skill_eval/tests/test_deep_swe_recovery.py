from __future__ import annotations

import json
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from types import SimpleNamespace
from typing import cast

import pytest
from pier.models.trial.result import TrialResult

from relay_knowledge_skill_eval.deep_swe_reporting import (
    _candidate_patch_was_collected,
    trial_outcome,
    write_deep_swe_report,
)
from relay_knowledge_skill_eval.deep_swe_runner import (
    _archive_infrastructure_failures,
    _has_infrastructure_failure,
)
from relay_knowledge_skill_eval.models import RunOutcome


def test_missing_candidate_patch_is_an_infrastructure_failure(tmp_path: Path) -> None:
    trial = cast(
        TrialResult,
        SimpleNamespace(
            exception_info=None,
            verifier=object(),
            verifier_result=SimpleNamespace(rewards={"reward": 0}),
            agent_execution=object(),
            agent_result=object(),
        ),
    )

    assert not _candidate_patch_was_collected(tmp_path)
    assert trial_outcome(trial, tmp_path) == (
        RunOutcome.INFRA_ERROR,
        "DeepSWE candidate patch was not collected",
    )
    artifacts = tmp_path / "artifacts"
    artifacts.mkdir()
    (artifacts / "model.patch").write_text("", encoding="utf-8")
    assert _candidate_patch_was_collected(tmp_path)
    assert trial_outcome(trial, tmp_path) == (RunOutcome.COMPLETED, "")


@pytest.mark.parametrize(
    ("exception_type", "expected"),
    [
        ("AgentTimeoutError", RunOutcome.TIMED_OUT),
        ("AgentExecutionError", RunOutcome.AGENT_ERROR),
        ("DeepSweConfigurationError", RunOutcome.INFRA_ERROR),
    ],
)
def test_agent_exception_precedes_missing_patch_check(
    tmp_path: Path, exception_type: str, expected: RunOutcome
) -> None:
    trial = cast(
        TrialResult,
        SimpleNamespace(
            exception_info=SimpleNamespace(
                exception_type=exception_type,
                exception_message="agent stopped",
            ),
            verifier=None,
            verifier_result=None,
            agent_execution=object(),
            agent_result=None,
        ),
    )

    assert trial_outcome(trial, tmp_path) == (expected, "agent stopped")


def test_agent_setup_exception_before_execution_is_infrastructure(
    tmp_path: Path,
) -> None:
    trial = cast(
        TrialResult,
        SimpleNamespace(
            exception_info=SimpleNamespace(
                exception_type="AgentSetupTimeoutError",
                exception_message="setup watchdog expired",
            ),
            verifier=None,
            verifier_result=None,
            agent_execution=None,
            agent_result=None,
        ),
    )

    assert trial_outcome(trial, tmp_path) == (
        RunOutcome.INFRA_ERROR,
        "setup watchdog expired",
    )


def test_deep_swe_report_writes_are_atomic_under_concurrency(tmp_path: Path) -> None:
    results_root = tmp_path / "jobs"
    results_root.mkdir()
    report_path = tmp_path / "report.json"

    with ThreadPoolExecutor(max_workers=8) as executor:
        futures = [
            executor.submit(
                write_deep_swe_report,
                results_root,
                output_path=report_path,
                skill_version="test-version",
                skill_sha256="test-sha",
            )
            for _ in range(24)
        ]
        for future in futures:
            assert future.result() == report_path

    assert '"benchmark": "DeepSWE"' in report_path.read_text(encoding="utf-8")
    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert report["metadata"]["skill_version"] == "test-version"
    assert report["metadata"]["skill_sha256"] == "test-sha"
    assert not list(tmp_path.glob(".report.json.*.tmp"))


def test_infrastructure_retry_preserves_and_resets_pier_job_state(
    tmp_path: Path, monkeypatch
) -> None:
    jobs_dir = tmp_path / "tasks"
    output_dir = tmp_path / "run"
    job_dir = jobs_dir / "task-a"
    infra_trial = job_dir / "trial-a"
    infra_trial.mkdir(parents=True)
    (infra_trial / "result.json").write_text("{}", encoding="utf-8")
    completed_trial = job_dir / "trial-b"
    completed_trial.mkdir()
    (completed_trial / "result.json").write_text("{}", encoding="utf-8")
    for name in ("config.json", "result.json", "lock.json", "job.log"):
        (job_dir / name).write_text(name, encoding="utf-8")

    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.TrialResult.model_validate_json",
        lambda _: object(),
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.JobResult.model_validate_json",
        lambda _: object(),
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.trial_outcome",
        lambda _trial, trial_dir: (
            (RunOutcome.INFRA_ERROR, "verifier failed")
            if trial_dir.name == "trial-a"
            else (RunOutcome.COMPLETED, "")
        ),
    )

    _archive_infrastructure_failures("task-a", jobs_dir, output_dir)

    assert not infra_trial.exists()
    assert completed_trial.exists()
    assert (job_dir / "result.json").exists()
    assert (job_dir / "config.json").exists()
    assert (job_dir / "lock.json").exists()
    history = output_dir / "infra-history" / "task-a"
    assert len(list(history.glob("*-trial-a/result.json"))) == 1
    assert not list(history.glob("*-job-state"))


def test_infrastructure_retry_recovers_state_left_after_archiving(
    tmp_path: Path, monkeypatch
) -> None:
    jobs_dir = tmp_path / "tasks"
    output_dir = tmp_path / "run"
    job_dir = jobs_dir / "task-a"
    job_dir.mkdir(parents=True)
    (job_dir / "result.json").write_text("stale", encoding="utf-8")
    (job_dir / "config.json").write_text("config", encoding="utf-8")
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.JobResult.model_validate_json",
        lambda _: (_ for _ in ()).throw(ValueError()),
    )

    _archive_infrastructure_failures("task-a", jobs_dir, output_dir)

    assert not (job_dir / "result.json").exists()
    archived = list((output_dir / "infra-history" / "task-a").glob("*-job-result.json"))
    assert len(archived) == 1
    assert archived[0].read_text(encoding="utf-8") == "stale"
    assert (job_dir / "config.json").exists()


def test_corrupt_job_result_is_archived_without_removing_valid_trials(
    tmp_path: Path, monkeypatch
) -> None:
    jobs_dir = tmp_path / "tasks"
    output_dir = tmp_path / "run"
    job_dir = jobs_dir / "task-a"
    for trial_name in ("trial-a", "trial-b"):
        trial_dir = job_dir / trial_name
        trial_dir.mkdir(parents=True)
        (trial_dir / "result.json").write_text("{}", encoding="utf-8")
    (job_dir / "result.json").write_text("{truncated", encoding="utf-8")
    (job_dir / "config.json").write_text("config", encoding="utf-8")
    (job_dir / "lock.json").write_text("lock", encoding="utf-8")
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.JobResult.model_validate_json",
        lambda _: (_ for _ in ()).throw(ValueError()),
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.TrialResult.model_validate_json",
        lambda _: object(),
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.trial_outcome",
        lambda *_: (RunOutcome.COMPLETED, ""),
    )

    assert _archive_infrastructure_failures("task-a", jobs_dir, output_dir) is True

    assert not (job_dir / "result.json").exists()
    assert (job_dir / "trial-a" / "result.json").exists()
    assert (job_dir / "trial-b" / "result.json").exists()
    assert (job_dir / "config.json").exists()
    assert (job_dir / "lock.json").exists()
    archived = list((output_dir / "infra-history" / "task-a").glob("*-job-result.json"))
    assert len(archived) == 1


def test_unreadable_trial_result_is_archived_for_retry(
    tmp_path: Path, monkeypatch
) -> None:
    jobs_dir = tmp_path / "tasks"
    output_dir = tmp_path / "run"
    job_dir = jobs_dir / "task-a"
    broken = job_dir / "trial-a"
    broken.mkdir(parents=True)
    (broken / "result.json").write_text("{truncated", encoding="utf-8")
    sibling = job_dir / "trial-b"
    sibling.mkdir()
    (sibling / "result.json").write_text("{}", encoding="utf-8")
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.TrialResult.model_validate_json",
        lambda value: object()
        if value == "{}"
        else (_ for _ in ()).throw(ValueError()),
    )
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_runner.trial_outcome",
        lambda *_: (RunOutcome.COMPLETED, ""),
    )

    assert _has_infrastructure_failure(job_dir) is True
    assert _archive_infrastructure_failures("task-a", jobs_dir, output_dir) is True
    assert not broken.exists()
    assert sibling.exists()
    archived = list((output_dir / "infra-history" / "task-a").glob("*-trial-a"))
    assert len(archived) == 1


def test_missing_trial_result_is_an_infrastructure_failure(tmp_path: Path) -> None:
    job_dir = tmp_path / "task-a"
    (job_dir / "trial-a").mkdir(parents=True)
    (job_dir / "trial-b").mkdir()

    assert _has_infrastructure_failure(job_dir) is True
