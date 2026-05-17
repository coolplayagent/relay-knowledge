"""Progressive memory storage for self-iteration runs."""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

from history import HistoryPaths, ensure_history, load_runs


def write_run_memory(paths: HistoryPaths, record: dict[str, Any]) -> list[dict[str, Any]]:
    ensure_history(paths)
    items = [primary_run_memory(record)]
    regression = regression_memory(record)
    if regression is not None:
        items.append(regression)
    for item in items:
        write_memory_item(paths, item)
    return items


def primary_run_memory(record: dict[str, Any]) -> dict[str, Any]:
    kind = primary_memory_kind(record)
    title = primary_memory_title(kind, record)
    summary = primary_memory_summary(kind, record)
    return memory_payload(
        record=record,
        kind=kind,
        title=title,
        summary=summary,
        detail=run_memory_detail(summary, record),
        tags=run_tags(record, kind),
    )


def regression_memory(record: dict[str, Any]) -> dict[str, Any] | None:
    degradations = [item for item in record.get("degradations", []) if isinstance(item, dict)]
    if not degradations:
        return None
    first = degradations[0]
    regression_kind = "performance_regression"
    if first.get("kind") == "case":
        regression_kind = "accuracy_regression"
    name = str(first.get("name") or first.get("case_id") or "regression")
    title = f"{record.get('run_id', '')} recorded {name} regression"
    summary = (
        f"Run {record.get('run_id', '')} recorded a {regression_kind.replace('_', ' ')} "
        f"while scoring {record.get('score', '')}. Future iterations should inspect the "
        "detail before making related changes."
    )
    return memory_payload(
        record=record,
        kind=regression_kind,
        title=title,
        summary=summary,
        detail=run_memory_detail(summary, record),
        tags=run_tags(record, regression_kind) + [safe_tag(name)],
        suffix=safe_id(name),
    )


def memory_payload(
    record: dict[str, Any],
    kind: str,
    title: str,
    summary: str,
    detail: str,
    tags: list[str],
    suffix: str | None = None,
) -> dict[str, Any]:
    run_id = str(record.get("run_id", "unknown-run"))
    item_id = safe_id(f"{run_id}-{kind}" + (f"-{suffix}" if suffix else ""))
    return {
        "id": item_id,
        "run_id": run_id,
        "kind": kind,
        "title": title,
        "summary": summary,
        "detail": detail,
        "tags": sorted({tag for tag in tags if tag}),
        "paths": related_paths(record),
        "score_impact": score_impact(record),
        "created_at": str(record.get("timestamp", "")),
    }


def write_memory_item(paths: HistoryPaths, item: dict[str, Any]) -> dict[str, Any]:
    ensure_history(paths)
    item_id = str(item["id"])
    summary_path = paths.memory_summaries / f"{item_id}.md"
    detail_path = paths.memory_details / f"{item_id}.md"
    summary_path.write_text(memory_markdown(item, "summary"), encoding="utf-8")
    detail_path.write_text(memory_markdown(item, "detail"), encoding="utf-8")
    index_item = {
        "id": item_id,
        "kind": item["kind"],
        "title": item["title"],
        "tags": item["tags"],
        "paths": item["paths"],
        "score_impact": item["score_impact"],
        "created_at": item["created_at"],
        "summary_path": str(summary_path),
        "detail_path": str(detail_path),
    }
    items = [entry for entry in load_memory_index(paths) if entry.get("id") != item_id]
    items.append(index_item)
    write_memory_index(paths, items)
    return index_item


def memory_markdown(item: dict[str, Any], body_key: str) -> str:
    tags = ", ".join(item.get("tags", [])) or "none"
    related = "\n".join(f"- `{path}`" for path in item.get("paths", [])) or "- none"
    return (
        f"# {item['title']}\n\n"
        f"- id: `{item['id']}`\n"
        f"- kind: `{item['kind']}`\n"
        f"- run: `{item['run_id']}`\n"
        f"- tags: {tags}\n\n"
        f"{item[body_key]}\n\n"
        "## Related Paths\n\n"
        f"{related}\n"
    )


def load_memory_index(paths: HistoryPaths) -> list[dict[str, Any]]:
    if not paths.memory_index.exists():
        return []
    items: list[dict[str, Any]] = []
    with paths.memory_index.open("r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                item = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(item, dict):
                items.append(item)
    return items


def write_memory_index(paths: HistoryPaths, items: list[dict[str, Any]]) -> None:
    ensure_history(paths)
    temp_path = paths.memory_index.with_suffix(".jsonl.tmp")
    with temp_path.open("w", encoding="utf-8") as handle:
        for item in items:
            handle.write(json.dumps(item, sort_keys=True, ensure_ascii=False))
            handle.write("\n")
    temp_path.replace(paths.memory_index)


def progressive_memory_index(paths: HistoryPaths, limit: int = 12) -> str:
    items = sorted(load_memory_index(paths), key=lambda item: str(item.get("created_at", "")), reverse=True)
    if not items:
        return "No progressive memory entries recorded yet."
    lines = [
        "Use this as an index, not as full context. Read summary_path first, then detail_path only when relevant.",
    ]
    for item in items[:limit]:
        tags = ",".join(str(tag) for tag in item.get("tags", [])[:8])
        lines.append(
            "- "
            f"id={item.get('id', '')} "
            f"kind={item.get('kind', '')} "
            f"title={compact_prompt_text(str(item.get('title', '')), 180)} "
            f"tags={tags or 'none'} "
            f"summary_path={item.get('summary_path', '')} "
            f"detail_path={item.get('detail_path', '')}"
        )
    if len(items) > limit:
        lines.append(f"- {len(items) - limit} older memory item(s) omitted from the prompt index.")
    return "\n".join(lines)


def primary_memory_kind(record: dict[str, Any]) -> str:
    if record.get("accepted"):
        return "accepted_optimization"
    if failed_gate_names(record):
        return "quality_gate_failure"
    return "rejected_attempt"


def primary_memory_title(kind: str, record: dict[str, Any]) -> str:
    run_id = record.get("run_id", "")
    if kind == "accepted_optimization":
        return f"{run_id} accepted optimization"
    if kind == "quality_gate_failure":
        return f"{run_id} failed gates: {', '.join(failed_gate_names(record)[:3])}"
    return f"{run_id} rejected attempt"


def primary_memory_summary(kind: str, record: dict[str, Any]) -> str:
    score = record.get("score", "")
    if kind == "accepted_optimization":
        improvements = "; ".join(compact_score_changes(record.get("improvements", []), 4))
        return (
            f"Accepted run {record.get('run_id', '')} scored {score}. "
            f"Changed paths: {', '.join(changed_paths(record)[:6]) or 'none recorded'}. "
            f"Key improvements: {improvements or 'none recorded'}."
        )
    if kind == "quality_gate_failure":
        return (
            f"Rejected run {record.get('run_id', '')} failed quality gates "
            f"{', '.join(failed_gate_names(record))}. Inspect the detail before retrying "
            "related changes."
        )
    reasons = "; ".join(str(reason) for reason in record.get("reject_reasons", []))
    return f"Rejected run {record.get('run_id', '')} scored {score}. Reasons: {reasons or 'none recorded'}."


def run_memory_detail(summary: str, record: dict[str, Any]) -> str:
    sections = [
        summary,
        "## Score\n\n"
        f"- score: {record.get('score', '')}\n"
        f"- accuracy: {record.get('accuracy', '')}\n"
        f"- performance: {record.get('performance', '')}\n"
        f"- stability: {record.get('stability', '')}",
        markdown_list("Changed Paths", changed_paths(record)),
        markdown_list("Reject Reasons", [str(reason) for reason in record.get("reject_reasons", [])]),
        markdown_list("Improvements", compact_score_changes(record.get("improvements", []), 12)),
        markdown_list("Degradations", compact_score_changes(record.get("degradations", []), 12)),
        markdown_list("Failed Gates", failed_gate_names(record)),
        markdown_list("Key Metrics", key_metric_lines(record)),
        markdown_list("Case Signals", case_signal_lines(record)),
    ]
    return "\n\n".join(sections)


def markdown_list(title: str, values: list[str]) -> str:
    body = "\n".join(f"- {value}" for value in values) if values else "- none recorded"
    return f"## {title}\n\n{body}"


def related_paths(record: dict[str, Any]) -> list[str]:
    paths = []
    for key in ("patch", "report"):
        value = record.get(key)
        if value:
            paths.append(str(value))
    return paths


def score_impact(record: dict[str, Any]) -> dict[str, Any]:
    return {
        "accepted": bool(record.get("accepted")),
        "score": record.get("score"),
        "accuracy": record.get("accuracy"),
        "performance": record.get("performance"),
        "stability": record.get("stability"),
        "improvement_count": len(record.get("improvements", [])),
        "degradation_count": len(record.get("degradations", [])),
    }


def run_tags(record: dict[str, Any], kind: str) -> list[str]:
    tags = [kind, "accepted" if record.get("accepted") else "rejected"]
    tags.extend(safe_tag(path) for path in changed_paths(record)[:8])
    tags.extend(safe_tag(name) for name in failed_gate_names(record)[:4])
    return tags


def failed_gate_names(record: dict[str, Any]) -> list[str]:
    return [
        str(gate.get("name", ""))
        for gate in record.get("gates", [])
        if isinstance(gate, dict) and not gate.get("passed", False) and gate.get("name")
    ]


def changed_paths(record: dict[str, Any]) -> list[str]:
    plan = record.get("optimization_plan")
    if isinstance(plan, dict):
        paths = plan.get("changed_paths")
        if isinstance(paths, list):
            return [str(path) for path in paths]
    return []


def key_metric_lines(record: dict[str, Any], limit: int = 8) -> list[str]:
    lines: list[str] = []
    for metric in record.get("metrics", []):
        if not isinstance(metric, dict) or len(lines) >= limit:
            continue
        name = metric.get("name")
        value = metric.get("value")
        if name is not None and value is not None:
            lines.append(f"{name}={value}")
    return lines


def case_signal_lines(record: dict[str, Any], limit: int = 8) -> list[str]:
    lines: list[str] = []
    for case in record.get("cases", []):
        if not isinstance(case, dict) or len(lines) >= limit:
            continue
        if case.get("passed"):
            continue
        lines.append(f"{case.get('case_id', '')} failed: {case.get('message', '')}")
    return lines


def historical_patch_memory_index(paths: HistoryPaths, limit: int = 12) -> str:
    if not paths.patches.exists():
        return "No historical patch files recorded yet."

    run_by_patch = {
        Path(str(run.get("patch", ""))).name: run
        for run in load_runs(paths)
        if run.get("patch")
    }
    patch_files = sorted(paths.patches.glob("*.patch"), key=lambda path: path.name, reverse=True)
    if not patch_files:
        return "No historical patch files recorded yet."

    lines = [
        "Use this as an index, not as full context. Read only patches that look relevant.",
    ]
    for patch_path in patch_files[:limit]:
        run = run_by_patch.get(patch_path.name, {})
        paths_changed = historical_patch_changed_paths(patch_path, run)
        reasons = compact_prompt_text("; ".join(str(reason) for reason in run.get("reject_reasons", [])), 260)
        improvements = compact_prompt_text("; ".join(compact_score_changes(run.get("improvements", []), 3)), 320)
        status = "accepted" if run.get("accepted") else "rejected"
        if not run:
            status = "unscored"
        line = (
            "- "
            f"patch={patch_path} "
            f"size_bytes={patch_path.stat().st_size} "
            f"status={status} "
            f"score={run.get('score', '')} "
            f"changed_paths={', '.join(paths_changed[:6]) or 'unknown'}"
        )
        if reasons:
            line += f" reject_reasons={reasons}"
        if improvements:
            line += f" improvements={improvements}"
        lines.append(line)
    if len(patch_files) > limit:
        lines.append(
            f"- {len(patch_files) - limit} older patch file(s) omitted; list them with "
            f"`find {paths.patches} -maxdepth 1 -type f -name '*.patch' -printf '%f %s\\n' | sort`."
        )
    return "\n".join(lines)


def historical_patch_changed_paths(patch_path: Path, run: dict[str, Any]) -> list[str]:
    plan = run.get("optimization_plan")
    if isinstance(plan, dict):
        paths_changed = plan.get("changed_paths")
        if isinstance(paths_changed, list) and paths_changed:
            return [str(path) for path in paths_changed]
    try:
        return changed_paths_from_diff(patch_path.read_text(encoding="utf-8", errors="replace"))
    except OSError:
        return []


def compact_prompt_text(value: str, limit: int) -> str:
    compact = " ".join(line.strip() for line in value.splitlines() if line.strip())
    if len(compact) <= limit:
        return compact
    return compact[-limit:]


def compact_score_changes(changes: object, limit: int = 8) -> list[str]:
    if not isinstance(changes, list):
        return []
    compact: list[str] = []
    for change in changes[:limit]:
        if not isinstance(change, dict):
            continue
        name = change.get("name") or change.get("case_id") or change.get("kind", "")
        previous = change.get("previous", "")
        current = change.get("current", "")
        reason = change.get("reason") or change.get("message", "")
        compact.append(f"{change.get('kind', '')}:{name} {previous}->{current} {reason}".strip())
    return compact


def changed_paths_from_diff(diff: str) -> list[str]:
    paths: list[str] = []
    seen: set[str] = set()
    for line in diff.splitlines():
        if not line.startswith("diff --git "):
            continue
        parts = line.split()
        if len(parts) < 4:
            continue
        path = parts[3]
        if path.startswith("b/"):
            path = path[2:]
        if path not in seen:
            seen.add(path)
            paths.append(path)
    return paths


def safe_id(value: str) -> str:
    slug = re.sub(r"[^A-Za-z0-9_.-]+", "-", value.strip()).strip("-")
    return slug[:160] or "memory"


def safe_tag(value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9_.:/-]+", "-", value.strip()).strip("-")
    return cleaned[:80]
