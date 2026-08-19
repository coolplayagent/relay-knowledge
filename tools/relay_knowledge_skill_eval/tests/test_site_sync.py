from __future__ import annotations

import json

from relay_knowledge_skill_eval.live_dashboard import (
    _completed_result_count,
    _load_active_suite,
    _results_are_final,
    _upload_site_snapshot,
)
from relay_knowledge_skill_eval.models import Condition, EvalResult, RunOutcome
from relay_knowledge_skill_eval.site_sync import sanitize_report


def test_site_snapshot_excludes_local_artifacts_and_prompts() -> None:
    report = {
        "schema_version": 1,
        "generated_at": "2026-08-07T00:00:00Z",
        "metadata": {
            "active_suite": "smoke-10",
            "expected_results": 20,
            "repository_commit": "private-local-detail",
        },
        "conditions": {"baseline": {"count": 1}},
        "paired": {"count": 0},
        "results": [
            {
                "instance_id": "astropy__astropy-1",
                "condition": "baseline",
                "outcome": "completed",
                "tokens": {"total": 10, "local_path": "D:/secret/token"},
                "tools": {"calls": 2, "local_path": "D:/secret/tool"},
                "timings": {
                    "agent_seconds": 1.0,
                    "local_path": "D:/secret/timing",
                },
                "swebench": {
                    "resolved": True,
                    "report_path": "D:/secret/scorer-report.json",
                    "test_output_path": "D:/secret/test-output.log",
                },
                "prompt_path": "D:/secret/prompt.txt",
                "trace_path": "D:/secret/trace.jsonl.gz",
                "patch_path": "D:/secret/generated.patch",
                "error": "contains private detail",
            }
        ],
    }

    snapshot = sanitize_report(report)
    encoded = json.dumps(snapshot)

    assert snapshot["metadata"] == {
        "active_suite": "smoke-10",
        "expected_results": 20,
    }
    assert "D:/secret" not in encoded
    assert "prompt_path" not in encoded
    assert "trace_path" not in encoded
    assert "patch_path" not in encoded
    assert "private-local-detail" not in encoded


def test_failed_site_snapshot_remains_retryable(monkeypatch) -> None:
    attempts = 0

    def flaky_upload(*_args: object) -> None:
        nonlocal attempts
        attempts += 1
        if attempts == 1:
            raise OSError("temporary network failure")

    monkeypatch.setenv("EVAL_SITE_INGEST_TOKEN", "test-token")
    monkeypatch.setattr(
        "relay_knowledge_skill_eval.live_dashboard.upload_report", flaky_upload
    )

    assert not _upload_site_snapshot({}, "https://example.test")
    assert _upload_site_snapshot({}, "https://example.test")
    assert attempts == 2


def test_live_dashboard_preserves_existing_suite(tmp_path) -> None:
    (tmp_path / "report.json").write_text(
        json.dumps({"metadata": {"active_suite": "verified-first-100"}}),
        encoding="utf-8",
    )

    assert _load_active_suite(tmp_path) == "verified-first-100"


def test_live_dashboard_waits_for_retryable_infrastructure_replacement() -> None:
    results = [
        EvalResult(
            instance_id="case",
            condition=Condition.BASELINE,
            outcome=RunOutcome.INFRA_ERROR,
        )
    ]

    assert _completed_result_count(results) == 0
    assert not _results_are_final(results, 1)
    results[0] = results[0].model_copy(update={"outcome": RunOutcome.COMPLETED})
    assert _completed_result_count(results) == 1
    assert _results_are_final(results, 1)
