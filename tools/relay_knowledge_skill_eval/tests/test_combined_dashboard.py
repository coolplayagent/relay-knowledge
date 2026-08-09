from __future__ import annotations

import json
from pathlib import Path

from relay_knowledge_skill_eval.combined_dashboard import (
    _completed,
    _condition_row,
    write_combined_dashboard,
)


def test_combined_dashboard_has_switchable_benchmark_tables(tmp_path: Path) -> None:
    report = {
        "metadata": {"expected_results": 2, "completed_results": 0},
        "conditions": {"baseline": {"count": 1}, "skill": {"count": 1}},
        "paired": {"count": 0, "metrics": {}},
        "results": [],
    }
    swe = tmp_path / "swe.json"
    deep = tmp_path / "deep.json"
    swe.write_text(json.dumps(report), encoding="utf-8")
    deep.write_text(json.dumps(report), encoding="utf-8")

    target = write_combined_dashboard(
        swe_report_path=swe,
        deep_swe_report_path=deep,
        output_dir=tmp_path / "site",
    )
    rendered = target.read_text(encoding="utf-8")

    assert _completed(report) == 0
    assert 'data-target="result-swe"' in rendered
    assert 'data-target="result-deep"' in rendered
    assert rendered.count("<li>") == 3
    assert "text-overflow:ellipsis" not in rendered
    assert "max-height:150px" in rendered


def test_dashboard_counts_every_accepted_repository_query_kind() -> None:
    rendered = _condition_row(
        "Skill 组",
        {
            "count": 1,
            "resolved": 1,
            "pass_rate": 1.0,
            "tools": {
                "relay_commands": {
                    "repo query": 1,
                    "repo context": 2,
                    "repo software": 3,
                    "repo feature-flags": 4,
                    "repo impact": 5,
                    "repo status": 99,
                }
            },
        },
    )

    assert '知识库查询</span><span class="metric-value">15</span>' in rendered
