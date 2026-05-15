#!/usr/bin/env python3
"""Continuous Codex-driven self-iteration entrypoint."""

from __future__ import annotations

import argparse
import hashlib
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
from scoring import EvaluationObservation, GateObservation, score_evaluation


@dataclass(frozen=True)
class PatchSnapshot:
    path: Path
    diff: str
    sha256: str
    base_ref: str

    @property
    def has_diff(self) -> bool:
        return bool(self.diff.strip())


class NonRetryableIterationError(RuntimeError):
    """Failure that cannot be fixed by starting another iteration."""


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
    previous_run = previous_scored_run(paths)
    candidate_score = score_evaluation(evaluation.observation, previous_run)
    commit = None
    if candidate_score.accepted:
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
    )
    append_run(paths, record)
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
    )
    append_run(paths, record)
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
) -> dict[str, Any]:
    return {
        "run_id": run_id,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "accepted": score["accepted"],
        "score": score["score"],
        "accuracy": score["accuracy"],
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
            f"accuracy={best.get('accuracy')} commit={best.get('commit')}"
        )
    rejected_summary = recent_rejected_summary(paths)
    return f"""You are running inside relay-knowledge self-iteration run {run_id}.

Goal:
- Improve Linux and relay-teams code repository tree parsing, code graph query accuracy, and performance.
- Focus on the highest-value small change you can safely implement in this repository.

Constraints:
- Follow AGENTS.md and repository architecture constraints.
- Keep the self-iteration framework independent; do not edit tools/self_iteration unless the framework itself is broken.
- Do not add broad rewrites, speculative APIs, dead code, or shallow wrappers.
- Preserve existing CLI/API behavior unless a test-backed correctness fix requires a compatible adjustment.
- Run relevant local checks for your change when feasible.
- Do not create commits yourself; the harness squashes accepted net changes into one commit.

Historical context:
{best_summary}

Recent rejected attempts to avoid:
{rejected_summary}

Recent worsened evaluation items:
{recent_degradation_summary(paths)}

Recent improved evaluation items to preserve:
{recent_improvement_summary(paths)}

Make one concrete candidate code change now. The self-iteration harness will build, test, score, squash-commit accepted improvements, or roll them back.
"""


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
            f"accuracy={run.get('accuracy', '')} "
            f"stability={run.get('stability', '')} "
            f"reasons={reasons} "
            f"report={run.get('report', '')}"
        )
    return "\n".join(lines)


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
        "accuracy": previous_run.get("accuracy"),
        "performance": previous_run.get("performance"),
        "stability": previous_run.get("stability"),
        "accepted": previous_run.get("accepted"),
        "commit": previous_run.get("commit"),
    }


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


def current_head(workspace: Path) -> str:
    return git(workspace, ["rev-parse", "HEAD"], check=True).stdout.strip()


def ensure_clean_worktree(workspace: Path) -> None:
    status = git(workspace, ["status", "--short"], check=True).stdout.strip()
    if status:
        raise NonRetryableIterationError(
            "working tree is not clean; commit/stash changes or pass --use-current-candidate"
        )


def git(workspace: Path, args: list[str], check: bool) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        ["git", *args],
        cwd=workspace,
        text=True,
        capture_output=True,
        check=False,
    )
    if check and completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or completed.stdout.strip())
    return completed


def git_lines(workspace: Path, args: list[str]) -> list[str]:
    output = git(workspace, args, check=True).stdout
    return [line for line in output.splitlines() if line]


def print_score(record: dict[str, Any]) -> None:
    status = "accepted" if record["accepted"] else "rejected"
    print(
        f"[self-iterate] {status} score={record['score']:.6f} "
        f"accuracy={record['accuracy']:.6f} performance={record['performance']:.6f} "
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
