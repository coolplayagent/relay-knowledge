from __future__ import annotations

import json
from pathlib import Path
from types import SimpleNamespace
from typing import cast

import pytest
from pier.models.trial.result import TrialResult

from relay_knowledge_skill_eval.deep_swe_reporting import (
    _verifier_candidate_error,
    _verifier_infrastructure_error,
    trial_outcome,
    write_deep_swe_report,
)
from relay_knowledge_skill_eval.models import Condition, EvalResult, RunOutcome


def test_test_patch_conflict_is_a_final_candidate_failure(tmp_path: Path) -> None:
    verifier = tmp_path / "verifier"
    verifier.mkdir()
    (verifier / "test-stdout.txt").write_text(
        "[verifier] ERROR: test.patch failed to apply\n",
        encoding="utf-8",
    )

    trial = cast(
        TrialResult,
        SimpleNamespace(
            exception_info=SimpleNamespace(
                exception_type="VerifierError",
                exception_message="test patch failed",
            ),
            verifier=object(),
            verifier_result=None,
            agent_execution=object(),
            agent_result=object(),
        ),
    )

    assert _verifier_infrastructure_error(tmp_path) == ""
    assert _verifier_candidate_error(tmp_path) == (
        "DeepSWE test patch conflicted with the candidate patch"
    )
    assert trial_outcome(trial, tmp_path)[0] is RunOutcome.COMPLETED


def test_unexecutable_verifier_remains_an_infrastructure_failure(
    tmp_path: Path,
) -> None:
    verifier = tmp_path / "verifier"
    verifier.mkdir()
    (verifier / "test-stderr.txt").write_text(
        "bash: ./run-tests.sh: cannot execute: required file not found\n",
        encoding="utf-8",
    )

    assert _verifier_candidate_error(tmp_path) == ""
    assert _verifier_infrastructure_error(tmp_path) == (
        "DeepSWE verifier script could not execute"
    )


def test_deep_swe_report_with_infrastructure_result_remains_nonfinal(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    results = [
        EvalResult(
            instance_id="task-a",
            condition=Condition.BASELINE,
            outcome=RunOutcome.COMPLETED,
        ),
        EvalResult(
            instance_id="task-a",
            condition=Condition.SKILL,
            outcome=RunOutcome.INFRA_ERROR,
        ),
    ]
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.deep_swe_reporting.load_deep_swe_results",
        lambda _: results,
    )

    report_path = write_deep_swe_report(
        tmp_path / "tasks",
        skill_version="test-version",
        skill_sha256="test-sha",
        expected_results=2,
        output_path=tmp_path / "report.json",
    )

    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert report["metadata"]["recorded_results"] == 2
    assert report["metadata"]["completed_results"] == 1
    assert report["metadata"]["infrastructure_failures"] == 1
    assert report["metadata"]["final"] is False
    assert report["paired"]["pass_rate_delta_ci_method"] == (
        "paired-normal-approximation"
    )
