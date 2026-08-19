# ruff: noqa: E501, RUF001
from __future__ import annotations

import html
import json
import time
from collections.abc import Mapping, Sequence
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from relay_knowledge_skill_eval.pi_events import REPOSITORY_QUERY_COMMANDS


def write_combined_dashboard(
    *,
    swe_report_path: Path,
    deep_swe_report_path: Path,
    output_dir: Path,
) -> Path:
    swe = _read_report(swe_report_path, "SWE-bench Verified · 前 100 题", 200)
    deep = _read_report(deep_swe_report_path, "DeepSWE · 113 题", 226)
    output_dir.mkdir(parents=True, exist_ok=True)
    target = output_dir / "live.html"
    temporary = target.with_suffix(".html.tmp")
    temporary.write_text(_render(swe, deep), encoding="utf-8")
    temporary.replace(target)
    return target


def watch_combined_dashboard(
    *,
    swe_report_path: Path,
    deep_swe_report_path: Path,
    output_dir: Path,
    interval_seconds: int = 15,
) -> None:
    observed: tuple[int, int] | None = None
    while True:
        current = (
            _mtime_ns(swe_report_path),
            _mtime_ns(deep_swe_report_path),
        )
        if current != observed:
            write_combined_dashboard(
                swe_report_path=swe_report_path,
                deep_swe_report_path=deep_swe_report_path,
                output_dir=output_dir,
            )
            observed = current
        time.sleep(interval_seconds)


def _read_report(path: Path, title: str, expected: int) -> dict[str, object]:
    if path.exists():
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
            if isinstance(payload, dict):
                payload = dict(payload)
                payload["dashboard_title"] = title
                metadata = _mapping(payload.get("metadata"))
                if "expected_results" not in metadata:
                    payload["metadata"] = {**metadata, "expected_results": expected}
                return payload
        except (OSError, json.JSONDecodeError):
            pass
    return {
        "dashboard_title": title,
        "metadata": {"expected_results": expected, "completed_results": 0},
        "conditions": {"baseline": {}, "skill": {}},
        "paired": {"count": 0, "metrics": {}},
        "results": [],
    }


def _render(swe: Mapping[str, object], deep: Mapping[str, object]) -> str:
    completed = _completed(swe) + _completed(deep)
    expected = _expected(swe) + _expected(deep)
    progress = completed / expected if expected else 0
    updated = datetime.now(UTC).astimezone().strftime("%H:%M:%S")
    cards = _benchmark_card(swe) + _benchmark_card(deep)
    averages = _average_panel(swe) + _average_panel(deep)
    recent = _recent_tabs(swe, deep)
    return f"""<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>relay-knowledge Skill A/B 测评</title>
<style>
:root{{--ink:#152033;--muted:#65738a;--line:#dfe6ef;--panel:#fff;--bg:#f3f6fa;
--blue:#2563eb;--blue2:#dbeafe;--green:#15803d;--red:#c2413a;--amber:#b45309}}
*{{box-sizing:border-box}} body{{margin:0;background:var(--bg);color:var(--ink);
font:13px/1.35 Inter,"Segoe UI","Microsoft YaHei",sans-serif}}
.shell{{max-width:1680px;margin:0 auto;padding:12px 18px 9px}}
header{{display:block;margin-bottom:8px}} h1{{font-size:22px;line-height:1.05;margin:0;
letter-spacing:-.35px;word-break:keep-all}}
.progress-head{{display:flex;gap:10px;align-items:center;
margin-bottom:9px}} .progress-track{{height:7px;background:#dce4ef;border-radius:99px;
overflow:hidden;flex:1}} .progress-fill{{height:100%;background:linear-gradient(90deg,#2563eb,#60a5fa)}}
.progress-copy{{font-variant-numeric:tabular-nums;color:#46556d;white-space:nowrap}}
.grid2{{display:grid;width:100%;grid-template-columns:minmax(0,1fr) minmax(0,1fr);gap:11px}}
.panel{{background:var(--panel);min-width:0;overflow:hidden;
border:1px solid var(--line);border-radius:12px;box-shadow:0 1px 2px #1520330a}}
.bench{{padding:11px 13px 9px}} .bench-head{{display:flex;align-items:flex-start;
justify-content:space-between;margin-bottom:7px}} h2{{font-size:15px;margin:0 0 2px}}
.small{{font-size:12px;color:var(--muted)}} .score{{font-size:20px;font-weight:700;
font-variant-numeric:tabular-nums}} .mini-track{{height:6px;background:#e7edf5;border-radius:99px;
overflow:hidden;margin-bottom:7px}} .mini-fill{{height:100%;background:var(--blue)}}
.cond-scroll{{width:100%;overflow-x:auto;overscroll-behavior-inline:contain}}
.cond-row{{display:grid;grid-template-columns:76px repeat(8,minmax(0,1fr));gap:4px;
align-items:center;padding:6px 0;border-bottom:1px solid #edf1f6}}
.cond-row:last-child{{border-bottom:0}} .metric{{min-width:0;text-align:right}}
.metric-label{{display:block;font-size:9px;color:var(--muted);white-space:nowrap}}
.metric-value{{display:block;font-size:11px;font-weight:650;white-space:normal;
overflow-wrap:anywhere}}
.group-stack{{font-size:11px}} .group-stack b{{display:block;font-size:12px;margin-bottom:1px}}
table{{width:100%;border-collapse:collapse;font-variant-numeric:tabular-nums}}
th{{font-size:11px;color:var(--muted);font-weight:600;text-align:right;padding:6px 7px;
white-space:nowrap;border-bottom:1px solid var(--line)}} td{{text-align:right;padding:7px;
white-space:nowrap;border-bottom:1px solid #edf1f6}} th:first-child,td:first-child{{text-align:left}}
tr:last-child td{{border-bottom:0}} .group{{font-weight:650}} .skill{{color:#1d4ed8}}
.pass{{color:var(--green);font-weight:650}} .fail{{color:var(--red);font-weight:650}}
.section-title{{font-size:14px;margin:9px 0 6px}} .compact{{padding:8px 12px}}
.compact h3{{font-size:13px;margin:0 0 3px}} .compact th,.compact td{{padding:3px 6px}}
.better{{color:var(--green);font-weight:650}} .worse{{color:var(--red);font-weight:650}}
.neutral{{color:var(--muted)}} .recent{{padding:6px 11px 5px}} .recent h3{{font-size:13px;
margin:0 0 3px}} .recent th,.recent td{{padding:3px 6px}}
.result-head{{display:flex;align-items:center;justify-content:space-between;margin-top:9px}}
.result-head .section-title{{margin:0}} .result-tabs{{display:flex;gap:5px}}
.tab-button{{border:1px solid var(--line);background:#fff;color:#536177;border-radius:7px;
padding:4px 9px;font:inherit;cursor:pointer}} .tab-button.active{{background:#e8f0ff;
border-color:#b8cdfa;color:#1d4ed8;font-weight:650}} .result-panel{{margin-top:6px}}
.result-panel[hidden]{{display:none}} .recent-wrap{{max-height:150px;overflow:auto}}
.recent thead{{position:sticky;top:0;background:#fff;z-index:1}}
footer{{color:var(--muted);font-size:11px;margin-top:7px;
padding:0 2px}} .notes{{margin:0;padding-left:20px;display:grid;grid-template-columns:repeat(3,1fr);
gap:12px}} .updated{{text-align:right;margin-top:4px}} code{{font:11px/1.2 ui-monospace,Consolas,monospace;
background:#f0f4f8;border-radius:4px;padding:2px 4px}}
@media(max-width:1100px){{
.shell{{padding:10px 10px 7px}} .grid2{{grid-template-columns:minmax(0,1fr);gap:8px}}
.panel{{width:100%}} header{{width:100%}} h1{{font-size:19px;line-height:1.15}}
.progress-head{{margin-bottom:7px}}
.bench{{padding:9px 10px 7px}} .cond-row{{min-width:690px}}
.cond-row .metric-value{{white-space:nowrap;overflow-wrap:normal}}
.bench-head{{display:grid;grid-template-columns:minmax(0,1fr) auto;align-items:start}}
.bench-head>div{{min-width:0}} .bench-head h2{{overflow-wrap:anywhere}}
.section-title{{word-break:keep-all;margin-top:8px}} .compact{{padding:7px 9px}}
.compact table{{table-layout:fixed}} .compact th,.compact td{{font-size:11px;padding:4px}}
.recent-wrap{{max-height:210px}} .recent{{overflow:hidden}} .recent table{{min-width:580px}}
footer .notes{{display:block;padding-left:18px}} footer .notes li{{margin-top:4px}}
}}
</style></head><body><main class="shell">
<header><h1>relay-knowledge CLI Skill A/B 测评</h1></header>
<div class="progress-head"><div class="progress-track"><div class="progress-fill" style="width:{progress:.2%}"></div></div>
<div class="progress-copy">总进度 <b>{completed}/{expected}</b> · {progress:.1%}</div></div>
<div class="grid2">{cards}</div>
<div class="section-title">同题耗时与资源对比 <span class="small">仅统计两组都已完成的题目，显示每题平均</span></div>
<div class="grid2">{averages}</div>
{recent}
<footer><ol class="notes">
<li>普通组与 Skill 组使用相同题目、运行环境、Pi 0.80.3 和 DeepSeek V4 Flash。</li>
<li>DeepSWE 的普通组和 Skill 组各保持单并发并同时运行；SWE-bench Verified 延续每次两题并行、每题两组并行。</li>
<li>每个 Agent 最长运行 3600 秒；Agent 执行时间不包含 Skill 组的预索引时间，预索引耗时会单独记录。</li>
</ol><div class="updated">页面仅在结果变化时下载更新 · {updated} 生成</div></footer>
</main><script>
let sig='';async function poll(){{if(document.hidden)return;try{{const u='live.html?t='+Date.now();
const r=await fetch(u,{{method:'HEAD',cache:'no-store'}});const n=(r.headers.get('last-modified')||'')+'|'+
(r.headers.get('content-length')||'');if(sig&&n&&n!==sig)location.reload();if(n)sig=n;}}catch{{}}}}
setInterval(poll,60000);document.addEventListener('visibilitychange',()=>{{if(!document.hidden)poll()}});poll();
for(const button of document.querySelectorAll('.tab-button')){{button.addEventListener('click',()=>{{
 const target=button.dataset.target;for(const item of document.querySelectorAll('.result-panel'))
 item.hidden=item.id!==target;for(const item of document.querySelectorAll('.tab-button'))
 item.classList.toggle('active',item===button);}})}}
</script></body></html>"""


def _benchmark_card(report: Mapping[str, object]) -> str:
    title = html.escape(str(report.get("dashboard_title", "测评")))
    completed = _completed(report)
    expected = _expected(report)
    pairs = int(_number(_mapping(report.get("paired")).get("count")))
    target_pairs = expected // 2
    progress = completed / expected if expected else 0
    rows = "".join(
        _condition_row(label, _mapping(_mapping(report.get("conditions")).get(key)))
        for key, label in (("baseline", "普通组"), ("skill", "Skill 组"))
    )
    return f"""<section class="panel bench"><div class="bench-head"><div><h2>{title}</h2>
<div class="small">已完成 {pairs}/{target_pairs} 道两组结果</div></div>
<div class="score">{progress:.1%}</div></div><div class="mini-track"><div class="mini-fill" style="width:{progress:.2%}"></div></div>
<div class="cond-scroll">{rows}</div></section>"""


def _condition_row(label: str, condition: Mapping[str, object]) -> str:
    tokens = _mapping(condition.get("tokens"))
    timings = _mapping(condition.get("timings_seconds"))
    tools = _mapping(condition.get("tools"))
    relay = _mapping(tools.get("relay_commands"))
    query_count = sum(
        _number(value)
        for key, value in relay.items()
        if key in REPOSITORY_QUERY_COMMANDS
    )
    skill_class = " skill" if "Skill" in label else ""
    count = _int(condition.get("count"))
    resolved = _int(condition.get("resolved"))
    rate = _number(condition.get("pass_rate"))
    values = (
        ("通过", f"{resolved}/{count} · {rate:.1%}"),
        ("输入", _total(tokens, "input")),
        ("输出", _total(tokens, "output")),
        ("缓存命中", _total(tokens, "cache_read")),
        ("总 Token", _total(tokens, "total")),
        ("费用", f"${_total_number(tokens, 'cost_usd'):,.3f}"),
        ("Agent 时间", _duration(_total_number(timings, "agent_primary"))),
        ("知识库查询", f"{int(query_count):,}"),
    )
    metrics = "".join(
        f'<span class="metric"><span class="metric-label">{name}</span>'
        f'<span class="metric-value">{value}</span></span>'
        for name, value in values
    )
    return (
        f'<div class="cond-row"><span class="group-stack{skill_class}">'
        f"<b>{html.escape(label)}</b>{count} 条结果</span>{metrics}</div>"
    )


def _average_panel(report: Mapping[str, object]) -> str:
    title = html.escape(str(report.get("dashboard_title", "测评")))
    metrics = _mapping(_mapping(report.get("paired")).get("metrics"))
    specifications = (
        ("Agent 时间", "agent_seconds", "duration", False),
        ("输入 Token", "input_tokens", "integer", False),
        ("输出 Token", "output_tokens", "integer", False),
        ("缓存命中 Token", "cache_read_tokens", "integer", False),
        ("总 Token", "total_tokens", "integer", False),
        ("费用", "cost_usd", "currency", False),
        ("工具调用", "tool_calls", "decimal", False),
    )
    rows = "".join(
        _metric_row(label, _mapping(metrics.get(key)), style, higher_better)
        for label, key, style, higher_better in specifications
    )
    count = _int(_mapping(report.get("paired")).get("count"))
    if count == 0:
        return (
            f'<section class="panel compact"><h3>{title}</h3>'
            '<div class="small">等待两组首道题完成后显示平均数据</div></section>'
        )
    return f"""<section class="panel compact"><h3>{title}</h3><div class="small">{count} 道题可对比</div>
<table><thead><tr><th>指标</th><th>普通组平均</th><th>Skill 组平均</th><th>变化</th></tr></thead>
<tbody>{rows}</tbody></table></section>"""


def _metric_row(
    label: str,
    metric: Mapping[str, object],
    style: str,
    higher_better: bool,
) -> str:
    baseline = _number(_mapping(metric.get("baseline")).get("mean"))
    skill = _number(_mapping(metric.get("skill")).get("mean"))
    ratio = (skill - baseline) / baseline if baseline else 0.0
    improvement = ratio > 0 if higher_better else ratio < 0
    change_class = "neutral" if ratio == 0 else ("better" if improvement else "worse")
    return f"""<tr><td>{html.escape(label)}</td><td>{_format_value(baseline, style)}</td>
<td>{_format_value(skill, style)}</td><td class="{change_class}">{ratio:+.1%}</td></tr>"""


def _recent_tabs(swe: Mapping[str, object], deep: Mapping[str, object]) -> str:
    return f"""<div class="result-head"><div class="section-title">逐题结果</div>
<div class="result-tabs"><button class="tab-button active" data-target="result-swe">SWE-bench Verified</button>
<button class="tab-button" data-target="result-deep">DeepSWE</button></div></div>
{_recent_panel(swe, panel_id="result-swe", active=True)}
{_recent_panel(deep, panel_id="result-deep", active=False)}"""


def _recent_panel(report: Mapping[str, object], *, panel_id: str, active: bool) -> str:
    title = html.escape(str(report.get("dashboard_title", "测评")))
    rows = _paired_recent(_sequence(report.get("results")))
    rendered = "".join(
        f"<tr><td><code>{html.escape(task)}</code></td><td>{_status(base)}</td>"
        f"<td>{_status(skill)}</td><td>{_short_resources(base)} → {_short_resources(skill)}</td></tr>"
        for task, base, skill in rows
    )
    if not rendered:
        rendered = '<tr><td colspan="4" class="small">等待首道题完成</td></tr>'
    hidden = "" if active else " hidden"
    return f"""<section id="{panel_id}" class="panel recent result-panel"{hidden}><h3>{title}</h3>
<div class="recent-wrap"><table><thead><tr><th>题目</th><th>普通组</th><th>Skill 组</th>
<th>总 Token（普通 → Skill）</th></tr></thead><tbody>{rendered}</tbody></table></div></section>"""


def _paired_recent(
    results: Sequence[object],
) -> list[tuple[str, Mapping[str, object], Mapping[str, object]]]:
    grouped: dict[str, dict[str, Mapping[str, object]]] = {}
    order: list[str] = []
    for item in results:
        result = _mapping(item)
        task = str(result.get("instance_id", ""))
        condition = str(result.get("condition", ""))
        if not task or condition not in {"baseline", "skill"}:
            continue
        if task not in grouped:
            order.append(task)
        grouped.setdefault(task, {})[condition] = result
    return [
        (task, grouped[task]["baseline"], grouped[task]["skill"])
        for task in order
        if "baseline" in grouped[task] and "skill" in grouped[task]
    ]


def _status(result: Mapping[str, object]) -> str:
    passed = (
        bool(result.get("resolved"))
        if "resolved" in result
        else bool(_mapping(result.get("swebench")).get("resolved"))
    )
    return (
        '<span class="pass">✓ 通过</span>'
        if passed
        else '<span class="fail">× 未通过</span>'
    )


def _short_resources(result: Mapping[str, object]) -> str:
    tokens = _mapping(result.get("tokens"))
    return _compact_number(_number(tokens.get("total")))


def _completed(report: Mapping[str, object]) -> int:
    metadata = _mapping(report.get("metadata"))
    if "completed_results" in metadata:
        return _int(metadata.get("completed_results"))
    conditions = _mapping(report.get("conditions"))
    return sum(
        _int(_mapping(conditions.get(key)).get("count"))
        for key in ("baseline", "skill")
    )


def _expected(report: Mapping[str, object]) -> int:
    return _int(_mapping(report.get("metadata")).get("expected_results"))


def _total(values: Mapping[str, object], key: str) -> str:
    return _compact_number(_total_number(values, key))


def _total_number(values: Mapping[str, object], key: str) -> float:
    return _number(_mapping(values.get(key)).get("total"))


def _compact_number(value: float) -> str:
    if abs(value) >= 1_000_000_000:
        return f"{value / 1_000_000_000:.2f}B"
    if abs(value) >= 1_000_000:
        return f"{value / 1_000_000:.2f}M"
    if abs(value) >= 1_000:
        return f"{value / 1_000:.1f}K"
    return f"{value:,.0f}"


def _duration(seconds: float) -> str:
    hours, remainder = divmod(int(seconds), 3600)
    minutes, secs = divmod(remainder, 60)
    return f"{hours}h {minutes:02d}m" if hours else f"{minutes}m {secs:02d}s"


def _format_value(value: float, style: str) -> str:
    if style == "duration":
        return f"{value:,.1f}s"
    if style == "currency":
        return f"${value:,.4f}"
    if style == "integer":
        return _compact_number(value)
    return f"{value:,.1f}"


def _mapping(value: object) -> Mapping[str, Any]:
    return value if isinstance(value, Mapping) else {}


def _sequence(value: object) -> Sequence[object]:
    return value if isinstance(value, Sequence) and not isinstance(value, str) else []


def _number(value: object) -> float:
    return float(value) if isinstance(value, int | float) else 0.0


def _int(value: object) -> int:
    return int(_number(value))


def _mtime_ns(path: Path) -> int:
    try:
        return path.stat().st_mtime_ns
    except OSError:
        return 0
