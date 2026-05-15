"""Run history and score chart persistence."""

from __future__ import annotations

import csv
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass(frozen=True)
class HistoryPaths:
    root: Path
    reports: Path
    patches: Path
    work: Path
    runs_jsonl: Path
    score_csv: Path
    score_svg: Path


def history_paths(workspace: Path) -> HistoryPaths:
    root = workspace / ".git" / "relay-knowledge-self-iteration"
    return HistoryPaths(
        root=root,
        reports=root / "reports",
        patches=root / "patches",
        work=root / "work",
        runs_jsonl=root / "runs.jsonl",
        score_csv=root / "score.csv",
        score_svg=root / "score.svg",
    )


def ensure_history(paths: HistoryPaths) -> None:
    paths.reports.mkdir(parents=True, exist_ok=True)
    paths.patches.mkdir(parents=True, exist_ok=True)
    paths.work.mkdir(parents=True, exist_ok=True)


def append_run(paths: HistoryPaths, record: dict[str, Any]) -> None:
    ensure_history(paths)
    with paths.runs_jsonl.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(record, sort_keys=True, ensure_ascii=False))
        handle.write("\n")


def load_runs(paths: HistoryPaths) -> list[dict[str, Any]]:
    if not paths.runs_jsonl.exists():
        return []
    runs: list[dict[str, Any]] = []
    with paths.runs_jsonl.open("r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            runs.append(json.loads(line))
    return runs


def best_accepted_run(paths: HistoryPaths) -> dict[str, Any] | None:
    accepted = [run for run in load_runs(paths) if run.get("accepted")]
    if not accepted:
        return None
    return max(
        accepted,
        key=lambda run: (
            float(run.get("score", 0.0)),
            bool(run.get("commit")),
            str(run.get("timestamp", "")),
        ),
    )


def write_report(paths: HistoryPaths, run_id: str, report: dict[str, Any]) -> Path:
    ensure_history(paths)
    report_path = paths.reports / f"{run_id}.json"
    report_path.write_text(
        json.dumps(report, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    return report_path


def export_history(paths: HistoryPaths) -> tuple[Path, Path]:
    runs = load_runs(paths)
    ensure_history(paths)
    write_csv(paths.score_csv, runs)
    write_svg(paths.score_svg, runs)
    return paths.score_csv, paths.score_svg


def write_csv(path: Path, runs: list[dict[str, Any]]) -> None:
    fields = [
        "run_id",
        "timestamp",
        "accepted",
        "score",
        "accuracy",
        "performance",
        "stability",
        "commit",
        "reject_reasons",
    ]
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for run in runs:
            writer.writerow(
                {
                    "run_id": run.get("run_id", ""),
                    "timestamp": run.get("timestamp", ""),
                    "accepted": run.get("accepted", False),
                    "score": run.get("score", 0.0),
                    "accuracy": run.get("accuracy", 0.0),
                    "performance": run.get("performance", 0.0),
                    "stability": run.get("stability", 0.0),
                    "commit": run.get("commit", ""),
                    "reject_reasons": "; ".join(run.get("reject_reasons", [])),
                }
            )


def write_svg(path: Path, runs: list[dict[str, Any]]) -> None:
    width = 760
    height = 280
    pad = 36
    accepted = [run for run in runs if run.get("score") is not None]
    if not accepted:
        svg = empty_svg(width, height, "No self-iteration scores yet")
        path.write_text(svg, encoding="utf-8")
        return

    scores = [float(run.get("score", 0.0)) for run in accepted]
    xs = scaled_positions(len(scores), pad, width - pad)
    ys = scale_values(scores, pad, height - pad)
    points = " ".join(f"{x:.1f},{y:.1f}" for x, y in zip(xs, ys))
    circles = "\n".join(
        f'<circle cx="{x:.1f}" cy="{y:.1f}" r="4" fill="{point_color(run)}" />'
        for run, x, y in zip(accepted, xs, ys)
    )
    labels = axis_labels(scores, pad, height - pad)
    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
  <rect width="100%" height="100%" fill="#ffffff"/>
  <text x="{pad}" y="24" font-family="monospace" font-size="16" fill="#111827">relay-knowledge self-iteration score</text>
  <line x1="{pad}" y1="{height - pad}" x2="{width - pad}" y2="{height - pad}" stroke="#d1d5db"/>
  <line x1="{pad}" y1="{pad}" x2="{pad}" y2="{height - pad}" stroke="#d1d5db"/>
  {labels}
  <polyline points="{points}" fill="none" stroke="#2563eb" stroke-width="2"/>
  {circles}
</svg>
"""
    path.write_text(svg, encoding="utf-8")


def empty_svg(width: int, height: int, message: str) -> str:
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
  <rect width="100%" height="100%" fill="#ffffff"/>
  <text x="24" y="42" font-family="monospace" font-size="16" fill="#111827">{message}</text>
</svg>
"""


def scaled_positions(count: int, start: int, end: int) -> list[float]:
    if count == 1:
        return [(start + end) / 2]
    span = end - start
    return [start + (span * index / (count - 1)) for index in range(count)]


def scale_values(values: list[float], top: int, bottom: int) -> list[float]:
    minimum = min(values)
    maximum = max(values)
    if maximum == minimum:
        return [(top + bottom) / 2 for _ in values]
    return [bottom - ((value - minimum) / (maximum - minimum) * (bottom - top)) for value in values]


def axis_labels(values: list[float], top: int, bottom: int) -> str:
    minimum = min(values)
    maximum = max(values)
    return (
        f'<text x="4" y="{top + 4}" font-family="monospace" font-size="11" fill="#6b7280">{maximum:.3f}</text>\n'
        f'  <text x="4" y="{bottom + 4}" font-family="monospace" font-size="11" fill="#6b7280">{minimum:.3f}</text>'
    )


def point_color(run: dict[str, Any]) -> str:
    return "#16a34a" if run.get("accepted") else "#dc2626"
