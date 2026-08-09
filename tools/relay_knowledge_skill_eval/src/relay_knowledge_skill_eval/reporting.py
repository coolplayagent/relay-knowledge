from __future__ import annotations

import csv
import html
import io
import json
import os
import tempfile
import time
from collections.abc import Callable, Sequence
from datetime import UTC, datetime
from pathlib import Path

from relay_knowledge_skill_eval.models import Condition, EvalResult, RunOutcome
from relay_knowledge_skill_eval.statistics import (
    distribution,
    mcnemar_exact,
    paired_pass_delta_ci,
    paired_pass_delta_normal_ci,
    paired_results,
)


def _condition_summary(results: Sequence[EvalResult]) -> dict[str, object]:
    count = len(results)
    resolved = sum(result.benchmark_resolved for result in results)
    return {
        "count": count,
        "resolved": resolved,
        "pass_rate": resolved / count if count else 0.0,
        "outcomes": {
            outcome: sum(result.outcome.value == outcome for result in results)
            for outcome in sorted({result.outcome.value for result in results})
        },
        "tokens": {
            "input": distribution([float(result.tokens.input) for result in results]),
            "output": distribution([float(result.tokens.output) for result in results]),
            "reasoning": distribution(
                [float(result.tokens.reasoning) for result in results]
            ),
            "cache_read": distribution(
                [float(result.tokens.cache_read) for result in results]
            ),
            "cache_write": distribution(
                [float(result.tokens.cache_write) for result in results]
            ),
            "total": distribution([float(result.tokens.total) for result in results]),
            "requests": distribution(
                [float(result.tokens.requests) for result in results]
            ),
            "cost_usd": distribution([result.tokens.cost_usd for result in results]),
        },
        "timings_seconds": {
            "image_prepare": distribution(
                [result.timings.image_prepare_seconds for result in results]
            ),
            "container_start": distribution(
                [result.timings.container_start_seconds for result in results]
            ),
            "preindex_excluded": distribution(
                [result.timings.preindex_seconds for result in results]
            ),
            "agent_primary": distribution(
                [result.timings.agent_seconds for result in results]
            ),
            "scoring": distribution(
                [result.timings.scoring_seconds for result in results]
            ),
            "end_to_end": distribution(
                [result.timings.end_to_end_seconds for result in results]
            ),
        },
        "tools": {
            "calls": distribution([float(result.tools.calls) for result in results]),
            "cumulative_seconds": distribution(
                [result.tools.cumulative_seconds for result in results]
            ),
            "errors": sum(result.tools.errors for result in results),
            "auto_retries": sum(result.tools.auto_retries for result in results),
            "harness_continuations": sum(
                result.tools.harness_continuations for result in results
            ),
            "by_name": _sum_maps([result.tools.by_name for result in results]),
            "relay_commands": _sum_maps(
                [result.tools.relay_commands for result in results]
            ),
        },
        "patches": {
            "present": sum(result.swebench.patch_exists for result in results),
            "empty": sum(not result.swebench.patch_exists for result in results),
            "applied": sum(result.swebench.patch_applied for result in results),
            "application_failures": sum(
                result.swebench.patch_exists and not result.swebench.patch_applied
                for result in results
            ),
        },
        "tests": {
            "fail_to_pass": {
                "success": sum(
                    len(result.swebench.fail_to_pass.success) for result in results
                ),
                "failure": sum(
                    len(result.swebench.fail_to_pass.failure) for result in results
                ),
            },
            "pass_to_pass": {
                "success": sum(
                    len(result.swebench.pass_to_pass.success) for result in results
                ),
                "failure": sum(
                    len(result.swebench.pass_to_pass.failure) for result in results
                ),
            },
        },
        "infrastructure": {
            "failures": sum(
                result.outcome.value == "infra_error" for result in results
            ),
            "retries": sum(result.infrastructure_retries for result in results),
        },
    }


def _sum_maps(values: Sequence[dict[str, int]]) -> dict[str, int]:
    total: dict[str, int] = {}
    for value in values:
        for key, count in value.items():
            total[key] = total.get(key, 0) + count
    return dict(sorted(total.items()))


def _paired_metric(
    pairs: Sequence[tuple[EvalResult, EvalResult]],
    accessor: Callable[[EvalResult], float],
) -> dict[str, float]:
    return distribution(
        [accessor(skill) - accessor(baseline) for baseline, skill in pairs]
    )


def _paired_comparison(
    pairs: Sequence[tuple[EvalResult, EvalResult]],
    accessor: Callable[[EvalResult], float],
) -> dict[str, dict[str, float]]:
    return {
        "baseline": distribution([accessor(baseline) for baseline, _ in pairs]),
        "skill": distribution([accessor(skill) for _, skill in pairs]),
        "delta": _paired_metric(pairs, accessor),
    }


def build_report(
    results: Sequence[EvalResult], *, paired_bootstrap_samples: int = 0
) -> dict[str, object]:
    ordered = sorted(results, key=lambda result: (result.instance_id, result.condition))
    pairs = paired_results(ordered)
    baseline_only = sum(
        baseline.benchmark_resolved and not skill.benchmark_resolved
        for baseline, skill in pairs
    )
    skill_only = sum(
        skill.benchmark_resolved and not baseline.benchmark_resolved
        for baseline, skill in pairs
    )
    both_pass = sum(
        baseline.benchmark_resolved and skill.benchmark_resolved
        for baseline, skill in pairs
    )
    both_fail = len(pairs) - baseline_only - skill_only - both_pass
    baseline_rate = (
        sum(baseline.benchmark_resolved for baseline, _ in pairs) / len(pairs)
        if pairs
        else 0.0
    )
    skill_rate = (
        sum(skill.benchmark_resolved for _, skill in pairs) / len(pairs)
        if pairs
        else 0.0
    )
    if paired_bootstrap_samples > 0:
        low, high = paired_pass_delta_ci(pairs, samples=paired_bootstrap_samples)
        interval_method = f"paired-bootstrap-{paired_bootstrap_samples}"
    else:
        low, high = paired_pass_delta_normal_ci(pairs)
        interval_method = "paired-normal-approximation"
    return {
        "schema_version": 1,
        "primary_time_excludes_preindex": True,
        "conditions": {
            condition.value: _condition_summary(
                [result for result in ordered if result.condition == condition]
            )
            for condition in Condition
        },
        "paired": {
            "count": len(pairs),
            "both_pass": both_pass,
            "both_fail": both_fail,
            "skill_only_pass": skill_only,
            "baseline_only_pass": baseline_only,
            "pass_rate_delta": skill_rate - baseline_rate,
            "pass_rate_delta_95ci": [low, high],
            "pass_rate_delta_ci_method": interval_method,
            "mcnemar_exact_p": mcnemar_exact(skill_only, baseline_only),
            "agent_seconds_delta": _paired_metric(
                pairs, lambda result: result.timings.agent_seconds
            ),
            "total_tokens_delta": _paired_metric(
                pairs, lambda result: float(result.tokens.total)
            ),
            "cost_usd_delta": _paired_metric(
                pairs, lambda result: result.tokens.cost_usd
            ),
            "tool_calls_delta": _paired_metric(
                pairs, lambda result: float(result.tools.calls)
            ),
            "metrics": {
                "agent_seconds": _paired_comparison(
                    pairs, lambda result: result.timings.agent_seconds
                ),
                "preindex_seconds": _paired_comparison(
                    pairs, lambda result: result.timings.preindex_seconds
                ),
                "total_tokens": _paired_comparison(
                    pairs, lambda result: float(result.tokens.total)
                ),
                "input_tokens": _paired_comparison(
                    pairs, lambda result: float(result.tokens.input)
                ),
                "output_tokens": _paired_comparison(
                    pairs, lambda result: float(result.tokens.output)
                ),
                "cache_read_tokens": _paired_comparison(
                    pairs, lambda result: float(result.tokens.cache_read)
                ),
                "cache_write_tokens": _paired_comparison(
                    pairs, lambda result: float(result.tokens.cache_write)
                ),
                "cost_usd": _paired_comparison(
                    pairs, lambda result: result.tokens.cost_usd
                ),
                "tool_calls": _paired_comparison(
                    pairs, lambda result: float(result.tools.calls)
                ),
                "requests": _paired_comparison(
                    pairs, lambda result: float(result.tokens.requests)
                ),
            },
        },
        "results": [
            result.model_dump(mode="json") | {"resolved": result.benchmark_resolved}
            for result in ordered
        ],
    }


def write_reports(
    results: Sequence[EvalResult],
    output_dir: Path,
    *,
    metadata: dict[str, object] | None = None,
    final: bool = False,
) -> dict[str, object]:
    output_dir.mkdir(parents=True, exist_ok=True)
    report = build_report(
        results,
        paired_bootstrap_samples=10_000 if final else 0,
    )
    report["metadata"] = metadata or {}
    report["generated_at"] = datetime.now(UTC).isoformat()
    _atomic_write_text(
        output_dir / "report.json",
        json.dumps(report, indent=2, ensure_ascii=False),
    )
    _write_csv(results, output_dir / "report.csv")
    jsonl = "".join(
        result.model_dump_json() + "\n"
        for result in sorted(
            results, key=lambda item: (item.instance_id, item.condition)
        )
    )
    _atomic_write_text(output_dir / "report.jsonl", jsonl)
    rendered = _render_html(report)
    _atomic_write_text(output_dir / "report.html", rendered)
    _atomic_write_text(output_dir / "live.html", rendered)
    return report


def write_completed_reports(
    results: Sequence[EvalResult],
    output_dir: Path,
    *,
    metadata: dict[str, object] | None = None,
) -> dict[str, object]:
    """Write a final report only when every expected result is non-retryable."""
    infrastructure_failures = [
        result for result in results if result.outcome is RunOutcome.INFRA_ERROR
    ]
    expected_results = None if metadata is None else metadata.get("expected_results")
    expected_count_available = (
        isinstance(expected_results, int)
        and not isinstance(expected_results, bool)
        and expected_results >= 0
    )
    missing_results = (
        max(expected_results - len(results), 0) if expected_count_available else 0
    )
    report = write_reports(
        results,
        output_dir,
        metadata=metadata,
        final=(
            expected_count_available
            and not infrastructure_failures
            and missing_results == 0
        ),
    )
    if not expected_count_available or infrastructure_failures or missing_results:
        details = []
        if not expected_count_available:
            details.append("the expected result count is unavailable")
        if infrastructure_failures:
            details.append(
                f"{len(infrastructure_failures)} retryable infrastructure result(s)"
            )
        if missing_results:
            details.append(f"{missing_results} expected result(s) still missing")
        raise RuntimeError(
            f"Evaluation still has {' and '.join(details)}; "
            "resume the run before finalizing"
        )
    return report


def _atomic_write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        dir=path.parent,
        prefix=f".{path.name}.",
        suffix=".tmp",
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        _replace_report_file(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def _replace_report_file(temporary: Path, target: Path) -> None:
    for attempt in range(20):
        try:
            temporary.replace(target)
            return
        except PermissionError:
            if attempt == 19:
                raise
            # Windows can briefly deny two simultaneous replacements of the
            # same target even though both source files are already closed.
            time.sleep(0.005 * (attempt + 1))


def _write_csv(results: Sequence[EvalResult], path: Path) -> None:
    fieldnames = [
        "instance_id",
        "condition",
        "attempt",
        "infrastructure_retries",
        "outcome",
        "resolved",
        "input_tokens",
        "output_tokens",
        "reasoning_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "total_tokens",
        "cost_usd",
        "requests",
        "tool_calls",
        "tool_errors",
        "harness_continuations",
        "preindex_seconds",
        "agent_seconds",
        "scoring_seconds",
        "end_to_end_seconds",
        "error",
    ]
    handle = io.StringIO(newline="")
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for result in sorted(results, key=lambda item: (item.instance_id, item.condition)):
        writer.writerow(
            {
                "instance_id": result.instance_id,
                "condition": result.condition.value,
                "attempt": result.attempt,
                "infrastructure_retries": result.infrastructure_retries,
                "outcome": result.outcome.value,
                "resolved": result.benchmark_resolved,
                "input_tokens": result.tokens.input,
                "output_tokens": result.tokens.output,
                "reasoning_tokens": result.tokens.reasoning,
                "cache_read_tokens": result.tokens.cache_read,
                "cache_write_tokens": result.tokens.cache_write,
                "total_tokens": result.tokens.total,
                "cost_usd": result.tokens.cost_usd,
                "requests": result.tokens.requests,
                "tool_calls": result.tools.calls,
                "tool_errors": result.tools.errors,
                "harness_continuations": result.tools.harness_continuations,
                "preindex_seconds": result.timings.preindex_seconds,
                "agent_seconds": result.timings.agent_seconds,
                "scoring_seconds": result.timings.scoring_seconds,
                "end_to_end_seconds": result.timings.end_to_end_seconds,
                "error": result.error,
            }
        )
    _atomic_write_text(path, handle.getvalue())


def _render_html(report: dict[str, object]) -> str:
    conditions = report["conditions"]
    paired = report["paired"]
    if not isinstance(conditions, dict) or not isinstance(paired, dict):
        raise ValueError("report structure is invalid")
    overview_values: dict[str, dict[str, str]] = {}
    for name in (Condition.BASELINE.value, Condition.SKILL.value):
        summary = conditions[name]
        if not isinstance(summary, dict):
            continue
        timings = summary["timings_seconds"]
        tokens = summary["tokens"]
        if not isinstance(timings, dict) or not isinstance(tokens, dict):
            continue
        agent_time = timings["agent_primary"]
        preindex_time = timings["preindex_excluded"]
        output_tokens = tokens["output"]
        input_tokens = tokens["input"]
        cache_tokens = tokens["cache_read"]
        total_tokens = tokens["total"]
        cost = tokens["cost_usd"]
        if not all(
            isinstance(value, dict)
            for value in (
                agent_time,
                preindex_time,
                input_tokens,
                output_tokens,
                cache_tokens,
                total_tokens,
                cost,
            )
        ):
            continue
        tools = summary.get("tools", {})
        tool_calls = tools.get("calls", {}) if isinstance(tools, dict) else {}
        relay_commands = (
            tools.get("relay_commands", {}) if isinstance(tools, dict) else {}
        )
        relay_count = (
            sum(relay_commands.values()) if isinstance(relay_commands, dict) else 0
        )
        requests = tokens.get("requests", {})
        requests = requests if isinstance(requests, dict) else {}
        outcomes = summary.get("outcomes", {})
        outcomes = outcomes if isinstance(outcomes, dict) else {}
        overview_values[name] = {
            "通过": f"{summary['resolved']}/{summary['count']}",
            "通过率": f"{float(summary['pass_rate']):.1%}",
            "执行时间": f"{float(agent_time['total']):,.1f}s",
            "预索引时间": f"{float(preindex_time['total']):,.1f}s",
            "输入 Token": f"{float(input_tokens['total']):,.0f}",
            "输出 Token": f"{float(output_tokens['total']):,.0f}",
            "缓存命中 Token": f"{float(cache_tokens['total']):,.0f}",
            "总 Token": f"{float(total_tokens['total']):,.0f}",
            "费用": f"${float(cost['total']):,.4f}",
            "API 请求": f"{float(requests.get('total', 0.0)):,.0f}",
            "工具调用": f"{float(tool_calls.get('total', 0.0)):,.0f}",
            "工具错误": str(
                int(tools.get("errors", 0)) if isinstance(tools, dict) else 0
            ),
            "知识库查询": str(relay_count),
            "Harness 续跑": (
                str(tools.get("harness_continuations", 0))
                if isinstance(tools, dict)
                else "0"
            ),
            "超时": str(outcomes.get("timed_out", 0)),
        }
    result_rows: list[str] = []
    results = report["results"]
    grouped_results: dict[str, dict[str, dict[str, object]]] = {}
    if isinstance(results, list):
        for value in results:
            if not isinstance(value, dict):
                continue
            instance_id = str(value.get("instance_id", ""))
            condition = str(value.get("condition", ""))
            grouped_results.setdefault(instance_id, {})[condition] = value
    for instance_id, condition_results in sorted(grouped_results.items()):
        baseline = condition_results.get(Condition.BASELINE.value)
        skill = condition_results.get(Condition.SKILL.value)
        baseline_score = baseline.get("swebench", {}) if baseline else {}
        skill_score = skill.get("swebench", {}) if skill else {}
        baseline_pass = bool(
            baseline.get(
                "resolved",
                baseline_score.get("resolved", False)
                if isinstance(baseline_score, dict)
                else False,
            )
            if baseline
            else False
        )
        skill_pass = bool(
            skill.get(
                "resolved",
                skill_score.get("resolved", False)
                if isinstance(skill_score, dict)
                else False,
            )
            if skill
            else False
        )
        baseline_status = (
            '<span class="good">✓ 通过</span>'
            if baseline_pass
            else '<span class="bad">✕ 未通过</span>'
            if baseline
            else '<span class="neutral">… 等待</span>'
        )
        skill_status = (
            '<span class="good">✓ 通过</span>'
            if skill_pass
            else '<span class="bad">✕ 未通过</span>'
            if skill
            else '<span class="neutral">… 等待</span>'
        )
        if not baseline or not skill:
            outcome = '<span class="neutral">等待另一组完成</span>'
        elif skill_pass and not baseline_pass:
            outcome = '<span class="good">仅 Skill 组通过</span>'
        elif baseline_pass and not skill_pass:
            outcome = '<span class="bad">仅普通组通过</span>'
        elif skill_pass:
            outcome = "两组都通过"
        else:
            outcome = "两组都未通过"

        def comparison_cell(
            section: str,
            key: str,
            number_format: str,
            unit: str = "",
            baseline_result: dict[str, object] | None = baseline,
            skill_result: dict[str, object] | None = skill,
        ) -> str:
            if not baseline_result or not skill_result:
                return '<span class="neutral">等待另一组完成</span>'
            baseline_section = baseline_result.get(section, {})
            skill_section = skill_result.get(section, {})
            if not isinstance(baseline_section, dict) or not isinstance(
                skill_section, dict
            ):
                return "—"
            baseline_value = float(baseline_section.get(key, 0.0))
            skill_value = float(skill_section.get(key, 0.0))
            relative = (
                (skill_value - baseline_value) / baseline_value
                if baseline_value
                else 0.0
            )
            delta_class = (
                "good" if relative < 0 else "bad" if relative > 0 else "neutral"
            )
            return (
                f"{format(baseline_value, number_format)} → "
                f"{format(skill_value, number_format)}{unit} "
                f'<span class="{delta_class}">({relative:+.1%})</span>'
            )

        relay_count = 0
        if skill:
            tools = skill.get("tools", {})
            relay = tools.get("relay_commands", {}) if isinstance(tools, dict) else {}
            if isinstance(relay, dict):
                relay_count = sum(
                    int(value) for value in relay.values() if isinstance(value, int)
                )
        result_rows.append(
            "<tr>"
            f"<td><code>{html.escape(instance_id)}</code></td>"
            f"<td>{baseline_status}</td><td>{skill_status}</td><td>{outcome}</td>"
            f"<td>{comparison_cell('tokens', 'total', ',.0f')}</td>"
            f"<td>{comparison_cell('timings', 'agent_seconds', '.1f', 's')}</td>"
            f"<td>{comparison_cell('tokens', 'cost_usd', '.5f', ' USD')}</td>"
            f"<td>{relay_count if skill else '—'}</td>"
            "</tr>"
        )
    overview_columns = (
        "通过",
        "通过率",
        "输入 Token",
        "输出 Token",
        "缓存命中 Token",
        "总 Token",
        "费用",
        "执行时间",
        "预索引时间",
        "工具调用",
        "知识库查询",
    )
    overview_rows = [
        "<tr>"
        f"<td>{group_label}</td>"
        + "".join(
            f"<td>{overview_values.get(condition, {}).get(label, '—')}</td>"
            for label in overview_columns
        )
        + "</tr>"
        for condition, group_label in (
            (Condition.BASELINE.value, "普通组"),
            (Condition.SKILL.value, "Skill 组"),
        )
    ]
    overview_head = (
        "<tr><th>测试组</th>"
        + "".join(f"<th>{html.escape(label)}</th>" for label in overview_columns)
        + "</tr>"
    )
    comparison_head = (
        "<tr><th>指标</th><th>普通组</th><th>Skill 组</th><th>变化</th></tr>"
    )
    paired_metrics = paired.get("metrics", {})
    paired_metrics = paired_metrics if isinstance(paired_metrics, dict) else {}

    def paired_average(metric: str, condition: str, number_format: str) -> str:
        metric_values = paired_metrics.get(metric, {})
        if not isinstance(metric_values, dict):
            return "—"
        condition_values = metric_values.get(condition, {})
        if not isinstance(condition_values, dict):
            return "—"
        return format(float(condition_values.get("mean", 0.0)), number_format)

    def paired_change(metric: str) -> str:
        metric_values = paired_metrics.get(metric, {})
        if not isinstance(metric_values, dict):
            return "—"
        baseline_values = metric_values.get("baseline", {})
        skill_values = metric_values.get("skill", {})
        if not isinstance(baseline_values, dict) or not isinstance(skill_values, dict):
            return "—"
        baseline_mean = float(baseline_values.get("mean", 0.0))
        skill_mean = float(skill_values.get("mean", 0.0))
        if baseline_mean == 0:
            return "—"
        relative = (skill_mean - baseline_mean) / baseline_mean
        delta_class = "good" if relative < 0 else "bad" if relative > 0 else "neutral"
        return f'<span class="{delta_class}">{relative:+.1%}</span>'

    paired_overview_rows = [
        "<tr>"
        f"<td>{html.escape(label)}</td>"
        f"<td>{paired_average(metric, 'baseline', number_format)}{unit}</td>"
        f"<td>{paired_average(metric, 'skill', number_format)}{unit}</td>"
        f"<td>{paired_change(metric)}</td>"
        "</tr>"
        for label, metric, number_format, unit in (
            ("执行时间", "agent_seconds", ",.1f", "s"),
            ("预索引时间", "preindex_seconds", ",.1f", "s"),
            ("输入 Token", "input_tokens", ",.0f", ""),
            ("输出 Token", "output_tokens", ",.0f", ""),
            ("缓存命中 Token", "cache_read_tokens", ",.0f", ""),
            ("总 Token", "total_tokens", ",.0f", ""),
            ("费用", "cost_usd", ",.5f", " USD"),
            ("API 请求", "requests", ",.1f", " 次"),
            ("工具调用", "tool_calls", ",.1f", " 次"),
        )
    ]
    item_head = (
        "<tr><th>题目</th><th>普通组</th><th>Skill 组</th><th>结果</th>"
        "<th>总 Token: 普通 → Skill</th><th>执行时间: 普通 → Skill</th>"
        "<th>费用: 普通 → Skill</th><th>Skill 组知识库查询</th></tr>"
    )
    style = (
        ":root{color-scheme:light dark}*{box-sizing:border-box}"
        "body{font:14px/1.5 system-ui;margin:0;color:light-dark(#18212f,#e2e8f0);"
        "background:light-dark(#f5f7fb,#0b1220)}"
        ".page{max-width:1480px;margin:0 auto;padding:32px 28px 48px}"
        "h1{font-size:26px;letter-spacing:-.02em;margin:0 0 8px}"
        "h2{font-size:18px;margin:0 0 14px}"
        ".eyebrow{color:light-dark(#2563eb,#7db4ff);font-weight:650;"
        "margin:0 0 6px}"
        ".status{margin:0 0 10px}"
        "section{background:light-dark(#fff,#111b2e);border:1px solid "
        "light-dark(#e2e8f0,#26344d);border-radius:14px;padding:20px 22px;"
        "margin-top:18px;box-shadow:0 1px 2px #0000000a}"
        "table{border-collapse:collapse;width:100%}"
        "th,td{border-bottom:1px solid light-dark(#e8edf4,#293750);"
        "padding:11px 9px;text-align:left;white-space:nowrap}"
        "tbody tr:last-child td{border-bottom:0}"
        "tbody tr:hover{background:light-dark(#f8fafc,#162238)}"
        "th{color:light-dark(#5d6b7e,#a9b7ca);font-size:12px;font-weight:650;"
        "letter-spacing:.02em}"
        "code{background:light-dark(#eef2f7,#1e293b);padding:3px 6px;"
        "border-radius:5px}"
        ".progress{height:9px;background:light-dark(#e1e7ef,#25334a);"
        "border-radius:99px;overflow:hidden;max-width:760px}"
        ".bar{height:100%;background:linear-gradient(90deg,#2563eb,#4f8df7)}"
        ".muted,.detail{color:light-dark(#64748b,#94a3b8)}"
        ".muted{font-size:12px}"
        ".good{color:light-dark(#087443,#4ade80)}"
        ".bad{color:light-dark(#b42318,#fb7185)}"
        ".neutral{color:light-dark(#64748b,#94a3b8)}"
        ".table-wrap{overflow-x:auto}"
        "details{margin-top:20px;color:light-dark(#64748b,#94a3b8)}"
        "summary{cursor:pointer;font-weight:600}"
        "@media(max-width:700px){.page{padding:22px 14px}section{padding:16px}}"
    )
    metadata = report.get("metadata", {})
    metadata = metadata if isinstance(metadata, dict) else {}
    recorded_results = len(results) if isinstance(results, list) else 0
    completed_metadata = metadata.get("completed_results")
    results_completed = (
        max(completed_metadata, 0)
        if isinstance(completed_metadata, int)
        and not isinstance(completed_metadata, bool)
        else recorded_results
    )
    expected_results = int(metadata.get("expected_results", results_completed) or 0)
    expected_questions = expected_results // 2
    percentage = results_completed / expected_results if expected_results else 0.0
    generated_value = str(report.get("generated_at", ""))
    try:
        generated_at = (
            datetime.fromisoformat(generated_value).astimezone().strftime("%H:%M:%S")
        )
    except ValueError:
        generated_at = generated_value
    generated_at = html.escape(generated_at)
    pair_count = int(paired.get("count", 0))
    return (
        '<!doctype html><html lang="zh"><head><meta charset="utf-8">'
        "<title>relay-knowledge Skill 效果对比</title>"
        f'<style>{style}</style></head><body data-generated="{generated_at}">'
        '<main class="page">'
        '<p class="eyebrow">SWE-bench Verified · 实时测评</p>'
        "<h1>relay-knowledge Skill 效果对比</h1>"
        f'<p class="status"><strong>已完成 {results_completed}/{expected_results} '
        f"次执行</strong> ({percentage:.0%}) · {pair_count}/{expected_questions} "
        "道题已有两组结果</p>"
        f'<div class="progress"><div class="bar" style="width:{percentage:.1%}">'
        "</div></div>"
        '<p class="muted">前台每 60 秒检查新数据 · 最近检查 '
        '<span id="last-check">--:--:--</span> · 数据更新 '
        f"{generated_at}</p>"
        '<section><h2>总体概览</h2><div class="table-wrap"><table><thead>'
        f"{overview_head}</thead>"
        f"<tbody>{''.join(overview_rows)}</tbody></table></div></section>"
        "<section><h2>同题耗时与资源对比</h2>"
        f'<p class="detail">仅统计两组都已完成的 {pair_count} 道题&#65292;'
        "以下均为每题平均值&#12290;</p>"
        '<div class="table-wrap"><table><thead>'
        f"{comparison_head}</thead><tbody>{''.join(paired_overview_rows)}"
        "</tbody></table></div></section>"
        '<section><h2>逐题对比</h2><div class="table-wrap"><table><thead>'
        f"{item_head}</thead>"
        f"<tbody>{''.join(result_rows)}</tbody></table></div></section>"
        "<details><summary>测评说明</summary>"
        "<p>使用 Pi 调用 DeepSeek。Skill 组会被明确要求使用 "
        "relay-knowledge CLI; 两组执行和官方评分均并行。</p>"
        "<p>执行时间不含预索引; 预索引耗时单独统计。</p>"
        "</details></main>"
        "<script>let known=null;const checked=()=>{const e=document.getElementById("
        "'last-check');if(e)e.textContent=new Date().toLocaleTimeString('zh-CN',"
        "{hour12:false})};const refresh=async()=>{if(document.hidden)return;try{"
        "const u='live.html?t='+Date.now();const h=await fetch(u,{method:'HEAD',"
        "cache:'no-store'});const fingerprint=(h.headers.get('last-modified')||'')+"
        "'|'+(h.headers.get('content-length')||'');if(known===null){known=fingerprint}"
        "else if(fingerprint!==known){const r=await fetch(u,{cache:'no-store'});"
        "const d=new DOMParser().parseFromString(await r.text(),'text/html');"
        "known=fingerprint;if(d.body.dataset.generated!==document.body.dataset."
        "generated){document.body.innerHTML=d.body.innerHTML;document.body.dataset."
        "generated=d.body.dataset.generated}}}catch{}finally{checked()}};checked();"
        "refresh();setInterval(refresh,60000);document.addEventListener("
        "'visibilitychange',()=>{if(!document.hidden)refresh()})</script></body></html>"
    )
