from __future__ import annotations

import json
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest

from relay_knowledge_skill_eval.models import (
    Condition,
    EvalResult,
    RunOutcome,
    SweBenchDiagnostics,
    TimingMetrics,
    TokenUsage,
)
from relay_knowledge_skill_eval.reporting import (
    build_report,
    write_completed_reports,
    write_reports,
)
from relay_knowledge_skill_eval.statistics import distribution, mcnemar_exact


def make_result(
    instance_id: str,
    condition: Condition,
    *,
    resolved: bool,
    tokens: int,
    preindex: float = 0.0,
    agent: float = 1.0,
    outcome: RunOutcome = RunOutcome.COMPLETED,
) -> EvalResult:
    return EvalResult(
        instance_id=instance_id,
        condition=condition,
        outcome=outcome,
        tokens=TokenUsage(total=tokens, cost_usd=tokens / 1000),
        timings=TimingMetrics(preindex_seconds=preindex, agent_seconds=agent),
        swebench=SweBenchDiagnostics(
            resolved=resolved,
            patch_exists=True,
            patch_applied=True,
        ),
    )


def test_report_calculates_paired_quality_cost_and_time(tmp_path: Path) -> None:
    results = [
        make_result("a", Condition.BASELINE, resolved=False, tokens=10),
        make_result(
            "a", Condition.SKILL, resolved=True, tokens=20, preindex=7, agent=2
        ),
        make_result("b", Condition.BASELINE, resolved=True, tokens=30),
        make_result("b", Condition.SKILL, resolved=True, tokens=40, preindex=8),
    ]
    report = build_report(results)
    assert report["primary_time_excludes_preindex"] is True
    assert report["paired"]["skill_only_pass"] == 1
    assert report["paired"]["baseline_only_pass"] == 0
    assert report["paired"]["pass_rate_delta"] == 0.5
    assert report["paired"]["total_tokens_delta"]["mean"] == 10
    assert (
        report["paired"]["pass_rate_delta_ci_method"] == "paired-normal-approximation"
    )
    assert (
        report["conditions"]["skill"]["timings_seconds"]["preindex_excluded"]["total"]
        == 15
    )
    written = write_reports(results, tmp_path, metadata={"runner": "pi"})
    assert written["metadata"] == {"runner": "pi"}
    for name in ("report.json", "report.jsonl", "report.csv", "report.html"):
        assert (tmp_path / name).is_file()
    parsed = json.loads((tmp_path / "report.json").read_text(encoding="utf-8"))
    assert parsed["paired"]["count"] == 2
    rendered = (tmp_path / "report.html").read_text(encoding="utf-8")
    assert "<h2>总体概览</h2>" in rendered
    assert "<h2>同题耗时与资源对比</h2>" in rendered
    assert "<th>指标</th><th>普通组</th><th>Skill 组</th>" in rendered
    assert "以下均为每题平均值" in rendered
    assert "缓存写入 Token" not in rendered
    assert "Skill - 普通" not in rendered
    assert "强制 Skill 组" not in rendered

    final = write_reports(results, tmp_path, final=True)
    assert final["paired"]["pass_rate_delta_ci_method"] == "paired-bootstrap-10000"


def test_report_does_not_count_verifier_pass_after_agent_timeout() -> None:
    results = [
        make_result("case", Condition.BASELINE, resolved=True, tokens=10),
        make_result(
            "case",
            Condition.SKILL,
            resolved=True,
            tokens=20,
            outcome=RunOutcome.TIMED_OUT,
        ),
    ]

    report = build_report(results)

    assert report["conditions"]["baseline"]["resolved"] == 1
    assert report["conditions"]["skill"]["resolved"] == 0
    assert report["paired"]["baseline_only_pass"] == 1
    assert report["paired"]["skill_only_pass"] == 0
    skill = next(
        result for result in report["results"] if result["condition"] == "skill"
    )
    assert skill["resolved"] is False


def test_concurrent_report_writers_publish_complete_files(tmp_path: Path) -> None:
    results = [
        make_result("a", Condition.BASELINE, resolved=True, tokens=10),
        make_result("a", Condition.SKILL, resolved=False, tokens=20),
    ]

    with ThreadPoolExecutor(max_workers=4) as executor:
        reports = list(
            executor.map(
                lambda index: write_reports(
                    results, tmp_path, metadata={"writer": index}
                ),
                range(12),
            )
        )

    assert len(reports) == 12
    assert (
        json.loads((tmp_path / "report.json").read_text(encoding="utf-8"))["paired"][
            "count"
        ]
        == 1
    )
    assert (
        len((tmp_path / "report.jsonl").read_text(encoding="utf-8").splitlines()) == 2
    )
    assert not list(tmp_path.glob(".*.tmp"))


def test_final_report_rejects_retryable_infrastructure_results(tmp_path: Path) -> None:
    results = [
        EvalResult(
            instance_id="retry-me",
            condition=Condition.BASELINE,
            outcome=RunOutcome.INFRA_ERROR,
        )
    ]

    with pytest.raises(RuntimeError, match="resume the run before finalizing"):
        write_completed_reports(results, tmp_path)

    report = json.loads((tmp_path / "report.json").read_text(encoding="utf-8"))
    assert report["paired"]["pass_rate_delta_ci_method"] == (
        "paired-normal-approximation"
    )


def test_final_report_rejects_missing_expected_results(tmp_path: Path) -> None:
    results = [
        make_result("partial", Condition.BASELINE, resolved=True, tokens=10),
    ]

    with pytest.raises(RuntimeError, match=r"1 expected result.*still missing"):
        write_completed_reports(
            results,
            tmp_path,
            metadata={"expected_results": 2},
        )

    report = json.loads((tmp_path / "report.json").read_text(encoding="utf-8"))
    assert report["metadata"]["expected_results"] == 2
    assert report["paired"]["pass_rate_delta_ci_method"] == (
        "paired-normal-approximation"
    )


def test_final_report_rejects_unknown_expected_count(tmp_path: Path) -> None:
    results = [make_result("partial", Condition.BASELINE, resolved=True, tokens=10)]

    with pytest.raises(RuntimeError, match="expected result count is unavailable"):
        write_completed_reports(results, tmp_path, metadata={})

    report = json.loads((tmp_path / "report.json").read_text(encoding="utf-8"))
    assert report["paired"]["pass_rate_delta_ci_method"] == (
        "paired-normal-approximation"
    )


def test_live_html_uses_explicit_non_infrastructure_progress(tmp_path: Path) -> None:
    results = [
        EvalResult(
            instance_id="retry-me",
            condition=Condition.BASELINE,
            outcome=RunOutcome.INFRA_ERROR,
        )
    ]

    write_reports(
        results,
        tmp_path,
        metadata={"expected_results": 1, "completed_results": 0},
    )

    rendered = (tmp_path / "live.html").read_text(encoding="utf-8")
    assert "已完成 0/1 次执行" in rendered
    assert "已完成 1/1 次执行" not in rendered


def test_distribution_and_exact_mcnemar_boundaries() -> None:
    assert distribution([]) == {"total": 0, "mean": 0.0, "p50": 0.0, "p95": 0.0}
    assert distribution([1, 2, 3])["p50"] == 2
    assert mcnemar_exact(0, 0) == 1
    assert mcnemar_exact(4, 0) == 0.125
