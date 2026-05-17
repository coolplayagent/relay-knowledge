#!/usr/bin/env python3
"""Continuous Codex-driven self-iteration entrypoint."""

from __future__ import annotations

import argparse
import hashlib
import json
import shlex
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from codex_driver import CodexConfig, CodexResult, run_codex
from evaluator import EvaluatorConfig, evaluate_candidate
from history import (
    append_run,
    best_accepted_run,
    ensure_history,
    export_history,
    history_paths,
    load_runs,
    previous_scored_run,
    write_report,
)
from memory import (
    changed_paths_from_diff,
    compact_prompt_text,
    compact_score_changes,
    historical_patch_memory_index,
    progressive_memory_index,
    write_run_memory,
)
from scoring import EvaluationObservation, GateObservation, score_evaluation
from workspace_git import (
    NonRetryableIterationError,
    current_head,
    ensure_clean_worktree,
    git,
    git_lines,
)

ACCEPTED_OPTIMIZATION_DOC = "docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md"


@dataclass(frozen=True)
class PatchSnapshot:
    path: Path
    diff: str
    sha256: str
    base_ref: str

    @property
    def has_diff(self) -> bool:
        return bool(self.diff.strip())

def main() -> int:
    args = parse_args()
    workspace = args.workspace.resolve()
    paths = history_paths(workspace)
    ensure_history(paths)

    if args.mode == "chart":
        csv_path, svg_path = export_history(paths)
        print(f"score csv: {csv_path}")
        print(f"score svg: {svg_path}")
        return 0
    if args.mode == "evaluate":
        return run_evaluate(args, workspace, paths)
    if args.mode == "once":
        args.max_iterations = 1
    return run_loop(args, workspace, paths)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Codex YOLO self-iteration for relay-knowledge code retrieval."
    )
    parser.add_argument(
        "mode",
        choices=["loop", "once", "evaluate", "chart"],
        nargs="?",
        default="loop",
    )
    parser.add_argument("--workspace", type=Path, default=default_workspace())
    parser.add_argument("--yolo", action="store_true", help="Use non-interactive high-permission Codex mode.")
    parser.add_argument("--model")
    parser.add_argument("--codex-profile")
    parser.add_argument("--codex-path")
    parser.add_argument("--codex-timeout-seconds", type=int, default=3600)
    parser.add_argument("--command-timeout-seconds", type=int, default=900)
    parser.add_argument("--profile", choices=["full", "smoke", "exhaustive"], default="full")
    parser.add_argument("--max-iterations", type=int)
    parser.add_argument("--stop-after-accepted", type=int)
    parser.add_argument("--sleep-seconds", type=int, default=5)
    parser.add_argument("--commit-message")
    parser.add_argument("--dry-run-codex", action="store_true")
    parser.add_argument("--keep-workdirs", action="store_true")
    parser.add_argument("--use-current-candidate", action="store_true")
    parser.add_argument("--fail-fast", action="store_true")
    return parser.parse_args()


def run_loop(args: argparse.Namespace, workspace: Path, paths: Any) -> int:
    if args.max_iterations is not None and args.max_iterations <= 0:
        return 0
    if args.stop_after_accepted is not None and args.stop_after_accepted <= 0:
        return 0
    if not args.use_current_candidate:
        try:
            ensure_clean_worktree(workspace)
        except NonRetryableIterationError as error:
            print(f"[self-iterate] cannot start: {error}", file=sys.stderr)
            return 1

    iteration = 0
    accepted_count = 0
    while True:
        if args.max_iterations is not None and iteration >= args.max_iterations:
            return 0
        if args.stop_after_accepted is not None and accepted_count >= args.stop_after_accepted:
            return 0
        iteration += 1
        print(f"[self-iterate] iteration {iteration} starting")
        try:
            accepted = run_generation_iteration(args, workspace, paths)
        except KeyboardInterrupt:
            print("[self-iterate] interrupted")
            return 130
        except NonRetryableIterationError as error:
            print(f"[self-iterate] iteration failed: {error}", file=sys.stderr)
            return 1
        except Exception as error:
            print(f"[self-iterate] iteration failed: {error}", file=sys.stderr)
            if args.fail_fast:
                return 1
            if args.max_iterations is not None and iteration >= args.max_iterations:
                return 1
            time.sleep(args.sleep_seconds)
            continue
        if accepted:
            accepted_count += 1
        time.sleep(args.sleep_seconds)


def run_evaluate(args: argparse.Namespace, workspace: Path, paths: Any) -> int:
    patch = capture_patch(workspace, paths, "manual-evaluate")
    evaluation = evaluate_candidate(
        evaluator_config(args, workspace, paths, "manual-evaluate"),
        generated_diff=patch.has_diff,
    )
    run_record = persist_scored_run(
        workspace=workspace,
        paths=paths,
        run_id="manual-evaluate",
        patch=patch,
        codex=None,
        evaluation=evaluation,
        commit=None,
        previous_run=previous_scored_run(paths),
    )
    print_score(run_record)
    return 0 if run_record["score"] > 0 else 1


def run_generation_iteration(args: argparse.Namespace, workspace: Path, paths: Any) -> bool:
    run_id = new_run_id()
    if not args.use_current_candidate:
        ensure_clean_worktree(workspace)
    base_ref = current_head(workspace)
    codex_result: CodexResult | None = None
    if args.use_current_candidate:
        print("[self-iterate] using current working tree as candidate")
    else:
        prompt = build_prompt(paths, run_id)
        codex_config = CodexConfig(
            workspace=workspace,
            codex_path=args.codex_path,
            yolo=args.yolo,
            model=args.model,
            profile=args.codex_profile,
            timeout_seconds=args.codex_timeout_seconds,
            dry_run=args.dry_run_codex,
        )
        codex_result = run_codex(codex_config, prompt)
        print(
            f"[self-iterate] codex exit={codex_result.exit_code} "
            f"duration_ms={codex_result.duration_ms}"
        )

    patch = capture_patch(workspace, paths, run_id, base_ref)
    if codex_result and not codex_result.succeeded:
        observation = EvaluationObservation(
            gates=[
                GateObservation(
                    name="codex_generation",
                    passed=False,
                    duration_ms=codex_result.duration_ms,
                    message=last_line(codex_result.stderr, codex_result.stdout),
                )
            ],
            generated_diff=patch.has_diff,
        )
        run_record = persist_failure_run(
            workspace, paths, run_id, patch, codex_result, observation
        )
        reject_candidate(workspace, patch, base_ref, hard_reset=not args.use_current_candidate)
        print_score(run_record)
        return False

    if not patch.has_diff:
        observation = EvaluationObservation(generated_diff=False)
        run_record = persist_failure_run(workspace, paths, run_id, patch, codex_result, observation)
        print_score(run_record)
        return False

    print(f"[self-iterate] candidate patch: {patch.path}")
    evaluation = evaluate_candidate(
        evaluator_config(args, workspace, paths, run_id),
        generated_diff=patch.has_diff,
    )
    apply_candidate_documentation_gate(evaluation, patch)
    previous_run = previous_scored_run(paths)
    candidate_score = score_evaluation(evaluation.observation, previous_run)
    optimization_plan = summarize_optimization_plan(codex_result, patch, candidate_score.to_dict())
    commit = None
    if candidate_score.accepted:
        write_adopted_optimization_document(
            workspace=workspace,
            run_id=run_id,
            patch=patch,
            score=candidate_score.to_dict(),
            evaluation=evaluation,
            optimization_plan=optimization_plan,
        )
        commit = commit_candidate(workspace, args.commit_message, candidate_score.score, base_ref)
    run_record = persist_scored_run(
        workspace=workspace,
        paths=paths,
        run_id=run_id,
        patch=patch,
        codex=codex_result,
        evaluation=evaluation,
        commit=commit,
        previous_run=previous_run,
        precomputed_score=candidate_score,
        optimization_plan=optimization_plan,
    )

    if run_record["accepted"]:
        print(f"[self-iterate] accepted commit={commit}")
        print_score(run_record)
        return True

    reject_candidate(workspace, patch, base_ref, hard_reset=not args.use_current_candidate)
    print("[self-iterate] rejected candidate and restored working tree")
    print_score(run_record)
    return False

def evaluator_config(
    args: argparse.Namespace,
    workspace: Path,
    paths: Any,
    run_id: str,
) -> EvaluatorConfig:
    return EvaluatorConfig(
        workspace=workspace,
        state_work_dir=paths.work / run_id,
        cases_path=workspace / "tools" / "self_iteration" / "cases.json",
        profile=args.profile,
        command_timeout_seconds=args.command_timeout_seconds,
        keep_workdirs=args.keep_workdirs,
    )


def apply_candidate_documentation_gate(evaluation: Any, patch: PatchSnapshot) -> None:
    gate = candidate_documentation_gate(patch)
    evaluation.observation.gates.append(gate)
    report = evaluation.report
    if isinstance(report, dict):
        gates = report.setdefault("gates", [])
        if isinstance(gates, list):
            gates.append(gate.__dict__)


def candidate_documentation_gate(patch: PatchSnapshot) -> GateObservation:
    changed_paths = changed_paths_from_diff(patch.diff)
    if not implementation_paths_require_documentation(changed_paths):
        return GateObservation(
            name="self_iteration_algorithm_documentation",
            passed=True,
            message="documentation not required for documentation-only candidate",
        )
    documented = ACCEPTED_OPTIMIZATION_DOC in changed_paths
    message = (
        f"{ACCEPTED_OPTIMIZATION_DOC} updated with candidate algorithm and architecture notes"
        if documented
        else f"missing candidate algorithm and architecture notes in {ACCEPTED_OPTIMIZATION_DOC}"
    )
    return GateObservation(
        name="self_iteration_algorithm_documentation",
        passed=documented,
        message=message,
    )


def implementation_paths_require_documentation(changed_paths: list[str]) -> bool:
    for path in changed_paths:
        if path.startswith("docs/") or path.endswith(".md"):
            continue
        return True
    return False


def persist_scored_run(
    workspace: Path,
    paths: Any,
    run_id: str,
    patch: PatchSnapshot,
    codex: CodexResult | None,
    evaluation: Any,
    commit: str | None,
    previous_run: dict[str, Any] | None,
    precomputed_score: Any | None = None,
    optimization_plan: dict[str, Any] | None = None,
) -> dict[str, Any]:
    score = precomputed_score or score_evaluation(evaluation.observation, previous_run)
    report = {
        "run_id": run_id,
        "workspace": str(workspace),
        "patch": patch_metadata(patch),
        "codex": codex_metadata(codex),
        "evaluation": evaluation.report,
        "score": score.to_dict(),
        "comparison": comparison_metadata(previous_run),
        "degradations": score.degradations,
        "improvements": score.improvements,
        "optimization_plan": optimization_plan or {},
    }
    report_path = write_report(paths, run_id, report)
    record = run_record(
        run_id=run_id,
        patch=patch,
        score=score.to_dict(),
        evaluation=evaluation.observation,
        report_path=report_path,
        commit=commit,
        codex=codex,
        optimization_plan=optimization_plan,
    )
    append_run(paths, record)
    write_run_memory(paths, record)
    export_history(paths)
    return record


def persist_failure_run(
    workspace: Path,
    paths: Any,
    run_id: str,
    patch: PatchSnapshot,
    codex: CodexResult | None,
    observation: EvaluationObservation,
) -> dict[str, Any]:
    previous_run = previous_scored_run(paths)
    score = score_evaluation(observation, previous_run)
    report = {
        "run_id": run_id,
        "workspace": str(workspace),
        "patch": patch_metadata(patch),
        "codex": codex_metadata(codex),
        "evaluation": {
            "generated_diff": observation.generated_diff,
            "gates": [gate.__dict__ for gate in observation.gates],
            "cases": [],
            "metrics": [],
        },
        "score": score.to_dict(),
        "comparison": comparison_metadata(previous_run),
        "degradations": score.degradations,
        "improvements": score.improvements,
        "optimization_plan": {},
    }
    report_path = write_report(paths, run_id, report)
    record = run_record(
        run_id=run_id,
        patch=patch,
        score=score.to_dict(),
        evaluation=observation,
        report_path=report_path,
        commit=None,
        codex=codex,
        optimization_plan=None,
    )
    append_run(paths, record)
    write_run_memory(paths, record)
    export_history(paths)
    return record


def run_record(
    run_id: str,
    patch: PatchSnapshot,
    score: dict[str, Any],
    evaluation: EvaluationObservation,
    report_path: Path,
    commit: str | None,
    codex: CodexResult | None,
    optimization_plan: dict[str, Any] | None,
) -> dict[str, Any]:
    return {
        "run_id": run_id,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "accepted": score["accepted"],
        "score": score["score"],
        "foundational_capability": score.get("foundational_capability", score.get("accuracy", 0.0)),
        "competitive_capability": score.get("competitive_capability", score.get("accuracy", 0.0)),
        "accuracy": score["accuracy"],
        "semantic_vector": score.get("semantic_vector", 1.0),
        "performance": score["performance"],
        "stability": score["stability"],
        "reject_reasons": score["reject_reasons"],
        "degradations": score.get("degradations", []),
        "improvements": score.get("improvements", []),
        "patch": str(patch.path),
        "patch_sha256": patch.sha256,
        "report": str(report_path),
        "commit": commit,
        "codex_exit_code": codex.exit_code if codex else None,
        "optimization_plan": optimization_plan or {},
        "gates": [gate.__dict__ for gate in evaluation.gates],
        "cases": [case.__dict__ for case in evaluation.cases],
        "metrics": [metric.__dict__ for metric in evaluation.metrics],
    }


def build_prompt(paths: Any, run_id: str) -> str:
    best = best_accepted_run(paths)
    best_summary = "No accepted historical run yet."
    if best:
        best_summary = (
            f"Best accepted score={best.get('score')} "
            f"foundational={best.get('foundational_capability', 'n/a')} "
            f"competitive={best.get('competitive_capability', 'n/a')} "
            f"accuracy={best.get('accuracy')} "
            f"semantic_vector={best.get('semantic_vector', 'n/a')} "
            f"commit={best.get('commit')}"
        )
    rejected_summary = recent_rejected_summary(paths)
    gate_priority = quality_gate_repair_priority(paths)
    return f"""You are running inside relay-knowledge self-iteration run {run_id}.

Goal:
- Prioritize foundational capability, competitive capability, semantic/vector retrieval, and stability before performance; only optimize speed after preserving or improving those protected objectives.
- Improve code repository tree parsing, code graph query accuracy, semantic/vector retrieval quality, stability, and performance across relay-teams, Linux, LevelDB, Kubernetes, Spring Framework, and the graph retrieval fixture.
- Treat foundational repo retrieval, competitive repo retrieval, and semantic/vector retrieval as protected self-iteration objectives. Runtime semantic/vector and embedding settings are read from the process environment by the relay-knowledge binary; do not hard-code provider URLs, API keys, model names, or dimensions in candidate changes.
- Focus on multi-repository, large-repository full-scope indexing and retrieval.
- Prefer algorithmic or architectural improvements over local special-casing: candidate pruning before scoring, SQLite/FTS-backed lookups, symbol identity normalization, import/call edge quality, bounded batch/finalize design, cache-aware query plans, and ranking fusion are all valid directions when test-backed.

Constraints:
- Follow AGENTS.md and repository architecture constraints.
- Keep the self-iteration framework independent; edit tools/self_iteration only when improving the harness, cases, prompt feedback, or evaluation policy itself.
- Do not add broad rewrites, speculative APIs, dead code, or shallow wrappers.
- Preserve existing CLI/API behavior unless a test-backed correctness fix requires a compatible adjustment.
- Run relevant local checks for your change when feasible.
- If recent quality gate diagnostics are present, treat them as the primary objective: reproduce or inspect the failed gate first, make the candidate directly address those failures, and only pursue ordinary score/ranking improvements after the failing gates have a concrete fix.
- Treat recent foundational, competitive, semantic/vector, case, and stability degradations as protected-objective regressions. Fix or explain them before pursuing pure latency/indexing gains, and do not trade away passing cases, backend availability, retriever source coverage, or quality gates for better timing.
- Any candidate that changes code, tests, benchmark cases, or self-iteration policy must also update {ACCEPTED_OPTIMIZATION_DOC} before evaluation. Write the optimization's algorithm, architecture, invariants, expected metric/case impact, and known risks in that document; the harness rejects undocumented implementation candidates before acceptance.
- The self-iteration memory store is progressive context. Start with the bounded memory index below, read only relevant summary_path files, and open detail_path or patch files only when the summary proves relevant to the current objective.
- The self-iteration patch directory is long-term memory. Use the patch memory index below to choose relevant historical patches, then read only the specific patch files you need in small ranges with commands like `sed -n '1,220p' .git/relay-knowledge-self-iteration/patches/<run>.patch`.
- Do not bulk-read all reports, all patch files, or all memory detail files. Load memory gradually by run, gate, path, metric, or case relevance.
- Do not create commits yourself; the harness squashes accepted net changes into one commit.

Current priority:
{gate_priority}

Historical context:
{best_summary}

Recent rejected attempts to avoid:
{rejected_summary}

Recent failed quality gate diagnostics:
{recent_failed_gate_diagnostics(paths)}

Recent worsened evaluation items:
{recent_degradation_summary(paths)}

Recent improved evaluation items to preserve:
{recent_improvement_summary(paths)}

Recent adopted optimization plans to build on:
{recent_adopted_optimization_summary(paths)}

Progressive memory index:
{progressive_memory_index(paths)}

Long-term patch memory index:
{historical_patch_memory_index(paths)}

Make one concrete candidate code change now. The self-iteration harness will build, test, score, squash-commit accepted improvements, or roll them back.
"""


def quality_gate_repair_priority(paths: Any) -> str:
    failed = recent_failed_gate_names(paths)
    if not failed:
        return "- No recent quality gate failures recorded; optimize the highest-value retrieval or indexing objective."

    return (
        "- Quality gate repair mode is active. Prioritize fixing these failed gates before "
        f"any other optimization: {', '.join(failed)}."
    )

def recent_failed_gate_names(paths: Any, limit: int = 8) -> list[str]:
    names: list[str] = []
    seen: set[str] = set()
    for run in reversed(load_runs(paths)):
        if run.get("accepted"):
            continue
        for gate in run.get("gates", []):
            if not isinstance(gate, dict) or gate.get("passed", False):
                continue
            name = str(gate.get("name", ""))
            if not name or name in seen:
                continue
            seen.add(name)
            names.append(name)
            if len(names) >= limit:
                return names
    return names


def recent_rejected_summary(paths: Any, limit: int = 3) -> str:
    rejected = [
        run
        for run in reversed(load_runs(paths))
        if not run.get("accepted") and run.get("reject_reasons")
    ][:limit]
    if not rejected:
        return "No rejected historical run with reasons yet."

    lines: list[str] = []
    for run in rejected:
        reasons = "; ".join(str(reason) for reason in run.get("reject_reasons", []))
        lines.append(
            "- "
            f"run_id={run.get('run_id', '')} "
            f"score={run.get('score', '')} "
            f"foundational={run.get('foundational_capability', '')} "
            f"competitive={run.get('competitive_capability', '')} "
            f"accuracy={run.get('accuracy', '')} "
            f"semantic_vector={run.get('semantic_vector', '')} "
            f"stability={run.get('stability', '')} "
            f"reasons={reasons} "
            f"report={run.get('report', '')}"
        )
    return "\n".join(lines)


def recent_failed_gate_diagnostics(paths: Any, limit: int = 3) -> str:
    diagnostics: list[str] = []
    for run in reversed(load_runs(paths)):
        if run.get("accepted"):
            continue
        failed_gate_names = [
            str(gate.get("name", ""))
            for gate in run.get("gates", [])
            if isinstance(gate, dict) and not gate.get("passed", False)
        ]
        if not failed_gate_names:
            continue
        report = load_run_report(run.get("report"))
        for gate_name in failed_gate_names:
            command = failed_command(report, gate_name)
            diagnostics.append(format_failed_gate_diagnostic(run, gate_name, command))
            if len(diagnostics) >= limit:
                return "\n".join(diagnostics)
    if not diagnostics:
        return "No failed quality gate diagnostics recorded yet."
    return "\n".join(diagnostics)


def load_run_report(report_path: object) -> dict[str, Any]:
    if not report_path:
        return {}
    try:
        path = Path(str(report_path))
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}


def failed_command(report: dict[str, Any], gate_name: str) -> dict[str, Any]:
    evaluation = report.get("evaluation", {})
    if not isinstance(evaluation, dict):
        return {}
    commands = evaluation.get("commands", [])
    if not isinstance(commands, list):
        return {}
    for command in commands:
        if isinstance(command, dict) and command.get("name") == gate_name:
            return command
    return {}


def format_failed_gate_diagnostic(
    run: dict[str, Any],
    gate_name: str,
    command: dict[str, Any],
) -> str:
    command_text = shell_command(command.get("command", []))
    stderr_tail = compact_prompt_text(str(command.get("stderr_tail", "")), 900)
    stdout_tail = compact_prompt_text(str(command.get("stdout_tail", "")), 900)
    fields = [
        f"- run_id={run.get('run_id', '')}",
        f"gate={gate_name}",
        f"report={run.get('report', '')}",
    ]
    if command:
        fields.extend(
            [
                f"exit_code={command.get('exit_code', '')}",
                f"duration_ms={command.get('duration_ms', '')}",
                f"command={command_text}",
            ]
        )
    if stderr_tail:
        fields.append(f"stderr_tail={stderr_tail}")
    if stdout_tail:
        fields.append(f"stdout_tail={stdout_tail}")
    return " ".join(fields)


def shell_command(command: object) -> str:
    if not isinstance(command, list):
        return ""
    return " ".join(shlex.quote(str(part)) for part in command)


def recent_degradation_summary(paths: Any, limit: int = 8) -> str:
    return recent_change_summary(
        paths=paths,
        field="degradations",
        empty_message="No worsened evaluation items recorded yet.",
        limit=limit,
    )


def recent_improvement_summary(paths: Any, limit: int = 8) -> str:
    return recent_change_summary(
        paths=paths,
        field="improvements",
        empty_message="No improved evaluation items recorded yet.",
        limit=limit,
    )


def recent_adopted_optimization_summary(paths: Any, limit: int = 3) -> str:
    plans: list[dict[str, Any]] = []
    for run in reversed(load_runs(paths)):
        if not run.get("accepted"):
            continue
        plan = run.get("optimization_plan")
        if isinstance(plan, dict) and plan:
            item = dict(plan)
            item["run_id"] = run.get("run_id", "")
            item["commit"] = run.get("commit", "")
            plans.append(item)
            if len(plans) >= limit:
                break
    if not plans:
        return "No adopted optimization plans recorded yet."

    lines: list[str] = []
    for plan in plans:
        changed_paths = ", ".join(str(path) for path in plan.get("changed_paths", [])[:6])
        improvements = "; ".join(str(item) for item in plan.get("key_improvements", [])[:4])
        codex_notes = compact_prompt_text(str(plan.get("codex_notes", "")), 700)
        lines.append(
            "- "
            f"run_id={plan.get('run_id', '')} "
            f"commit={plan.get('commit', '')} "
            f"changed_paths={changed_paths} "
            f"improvements={improvements} "
            f"notes={codex_notes}"
        )
    return "\n".join(lines)

def recent_change_summary(
    paths: Any,
    field: str,
    empty_message: str,
    limit: int,
) -> str:
    changes: list[dict[str, Any]] = []
    for run in reversed(load_runs(paths)):
        for change in run.get(field, []):
            if isinstance(change, dict):
                item = dict(change)
                item["run_id"] = run.get("run_id", "")
                changes.append(item)
                if len(changes) >= limit:
                    break
        if len(changes) >= limit:
            break
    if not changes:
        return empty_message

    lines: list[str] = []
    for item in changes:
        kind = item.get("kind", "")
        name = item.get("name") or item.get("case_id") or ""
        previous = item.get("previous", "")
        current = item.get("current", "")
        message = item.get("message", "")
        lines.append(
            "- "
            f"run_id={item.get('run_id', '')} "
            f"kind={kind} name={name} "
            f"previous={previous} current={current} "
            f"message={message}"
        )
    return "\n".join(lines)


def comparison_metadata(previous_run: dict[str, Any] | None) -> dict[str, Any] | None:
    if previous_run is None:
        return None
    return {
        "run_id": previous_run.get("run_id"),
        "score": previous_run.get("score"),
        "foundational_capability": previous_run.get("foundational_capability"),
        "competitive_capability": previous_run.get("competitive_capability"),
        "accuracy": previous_run.get("accuracy"),
        "semantic_vector": previous_run.get("semantic_vector"),
        "performance": previous_run.get("performance"),
        "stability": previous_run.get("stability"),
        "accepted": previous_run.get("accepted"),
        "commit": previous_run.get("commit"),
    }


def summarize_optimization_plan(
    codex: CodexResult | None,
    patch: PatchSnapshot,
    score: dict[str, Any],
) -> dict[str, Any]:
    output = ""
    if codex:
        output = "\n".join(part for part in [codex.stdout, codex.stderr] if part.strip())
    return {
        "changed_paths": changed_paths_from_diff(patch.diff),
        "key_improvements": compact_score_changes(score.get("improvements", [])),
        "known_degradations": compact_score_changes(score.get("degradations", [])),
        "codex_notes": compact_prompt_text(output, 1800),
    }

def write_adopted_optimization_document(
    workspace: Path,
    run_id: str,
    patch: PatchSnapshot,
    score: dict[str, Any],
    evaluation: Any,
    optimization_plan: dict[str, Any],
) -> None:
    path = workspace / ACCEPTED_OPTIMIZATION_DOC
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        path.write_text(accepted_optimization_doc_header(), encoding="utf-8")
    with path.open("a", encoding="utf-8") as handle:
        handle.write(accepted_optimization_entry(run_id, patch, score, evaluation, optimization_plan))


def accepted_optimization_doc_header() -> str:
    return (
        "# 自迭代采纳优化记录\n\n"
        "本文档由自迭代 harness 在候选通过质量门禁并被采纳时追加，"
        "用于把本轮采用的优化思路传递给后续 Codex 迭代。人工维护的总结可以继续补充在对应条目下。\n\n"
    )


def accepted_optimization_entry(
    run_id: str,
    patch: PatchSnapshot,
    score: dict[str, Any],
    evaluation: Any,
    optimization_plan: dict[str, Any],
) -> str:
    changed_paths = optimization_plan.get("changed_paths", [])
    improvements = optimization_plan.get("key_improvements", [])
    degradations = optimization_plan.get("known_degradations", [])
    case_count = len(evaluation.observation.cases)
    passed_cases = sum(1 for case in evaluation.observation.cases if case.passed)
    metrics = [
        f"{metric.name}={metric.value:.0f}ms"
        for metric in evaluation.observation.metrics
        if metric.name.endswith("_ms")
    ][:8]
    return (
        f"## {run_id}\n\n"
        f"- patch: `{patch.path}`\n"
        f"- score: {score.get('score')} "
        f"(foundational={score.get('foundational_capability', 'n/a')}, "
        f"competitive={score.get('competitive_capability', 'n/a')}, "
        f"accuracy={score.get('accuracy')}, "
        f"semantic_vector={score.get('semantic_vector', 'n/a')}, "
        f"performance={score.get('performance')}, "
        f"stability={score.get('stability')})\n"
        f"- cases: {passed_cases}/{case_count} passed\n"
        f"- changed paths: {', '.join(f'`{path}`' for path in changed_paths) or 'none recorded'}\n"
        f"- key improvements: {'; '.join(improvements) or 'none recorded'}\n"
        f"- known degradations: {'; '.join(degradations) or 'none recorded'}\n"
        f"- latency metrics: {'; '.join(metrics) or 'none recorded'}\n\n"
        "Adopted optimization notes:\n\n"
        f"{compact_prompt_text(str(optimization_plan.get('codex_notes', '')), 1200) or 'No Codex notes captured.'}\n\n"
    )


def capture_patch(
    workspace: Path,
    paths: Any,
    run_id: str,
    base_ref: str = "HEAD",
) -> PatchSnapshot:
    ensure_history(paths)
    untracked = git_lines(workspace, ["ls-files", "--others", "--exclude-standard"])
    if untracked:
        git(workspace, ["add", "-N", "--", *untracked], check=False)
    diff = git(workspace, ["diff", "--binary", base_ref], check=True).stdout
    git(workspace, ["reset", "--mixed", "HEAD"], check=True)
    patch_path = paths.patches / f"{run_id}.patch"
    patch_path.write_text(diff, encoding="utf-8")
    return PatchSnapshot(
        path=patch_path,
        diff=diff,
        sha256=hashlib.sha256(diff.encode("utf-8")).hexdigest(),
        base_ref=base_ref,
    )


def reject_candidate(
    workspace: Path,
    patch: PatchSnapshot,
    base_ref: str | None = None,
    hard_reset: bool = False,
) -> None:
    if hard_reset:
        git(workspace, ["reset", "--hard", base_ref or patch.base_ref], check=True)
        git(workspace, ["clean", "-fd"], check=True)
        return
    if patch.has_diff:
        subprocess.run(
            ["git", "apply", "-R", str(patch.path)],
            cwd=workspace,
            text=True,
            capture_output=True,
            check=True,
        )
    git(workspace, ["reset", "--mixed", "HEAD"], check=True)


def commit_candidate(
    workspace: Path,
    commit_message: str | None,
    score: float,
    base_ref: str,
) -> str:
    message = commit_message or f"Self-iterate code retrieval score {score:.6f}"
    git(workspace, ["reset", "--mixed", base_ref], check=True)
    git(workspace, ["add", "-A"], check=True)
    diff_status = git(workspace, ["diff", "--cached", "--quiet"], check=False)
    if diff_status.returncode == 0:
        raise RuntimeError("accepted candidate has no net diff to commit")
    if diff_status.returncode != 1:
        raise RuntimeError(diff_status.stderr.strip() or diff_status.stdout.strip())
    git(workspace, ["commit", "-m", message], check=True)
    return git(workspace, ["rev-parse", "--short", "HEAD"], check=True).stdout.strip()


def print_score(record: dict[str, Any]) -> None:
    status = "accepted" if record["accepted"] else "rejected"
    print(
        f"[self-iterate] {status} score={record['score']:.6f} "
        f"foundational={record.get('foundational_capability', record['accuracy']):.6f} "
        f"competitive={record.get('competitive_capability', record['accuracy']):.6f} "
        f"accuracy={record['accuracy']:.6f} "
        f"semantic_vector={record.get('semantic_vector', 1.0):.6f} "
        f"performance={record['performance']:.6f} "
        f"stability={record['stability']:.6f}"
    )
    if record["reject_reasons"]:
        print("[self-iterate] reasons: " + "; ".join(record["reject_reasons"]))
    print(f"[self-iterate] report: {record['report']}")


def patch_metadata(patch: PatchSnapshot) -> dict[str, Any]:
    return {
        "path": str(patch.path),
        "sha256": patch.sha256,
        "bytes": len(patch.diff.encode("utf-8")),
        "has_diff": patch.has_diff,
        "base_ref": patch.base_ref,
    }


def codex_metadata(codex: CodexResult | None) -> dict[str, Any] | None:
    if codex is None:
        return None
    return {
        "command": codex.command,
        "exit_code": codex.exit_code,
        "duration_ms": codex.duration_ms,
        "stdout_tail": codex.stdout[-4000:],
        "stderr_tail": codex.stderr[-4000:],
    }


def new_run_id() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def last_line(*outputs: str) -> str:
    for output in outputs:
        lines = [line.strip() for line in output.splitlines() if line.strip()]
        if lines:
            return lines[-1]
    return ""


def default_workspace() -> Path:
    return Path(__file__).resolve().parents[2]

if __name__ == "__main__":
    raise SystemExit(main())
