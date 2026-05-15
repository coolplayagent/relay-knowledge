from __future__ import annotations

import argparse
import contextlib
import io
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import self_iterate
from codex_driver import CodexResult
from evaluator import EvaluationRun
from history import history_paths
from scoring import CaseObservation, EvaluationObservation, GateObservation, MetricObservation
from self_iterate import capture_patch, commit_candidate, reject_candidate, run_loop


class PatchFlowTests(unittest.TestCase):
    def test_rejected_patch_restores_tracked_and_untracked_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            run(workspace, ["git", "init"])
            run(workspace, ["git", "config", "user.email", "relay@example.invalid"])
            run(workspace, ["git", "config", "user.name", "Relay Test"])
            (workspace / "tracked.txt").write_text("base\n", encoding="utf-8")
            run(workspace, ["git", "add", "tracked.txt"])
            run(workspace, ["git", "commit", "-m", "base"])

            (workspace / "tracked.txt").write_text("changed\n", encoding="utf-8")
            (workspace / "new.txt").write_text("new\n", encoding="utf-8")
            patch = capture_patch(workspace, history_paths(workspace), "run")

            self.assertTrue(patch.has_diff)
            reject_candidate(workspace, patch)

            self.assertEqual((workspace / "tracked.txt").read_text(encoding="utf-8"), "base\n")
            self.assertFalse((workspace / "new.txt").exists())
            status = run(workspace, ["git", "status", "--short"]).stdout.strip()
            self.assertEqual(status, "")

    def test_rejected_patch_restores_candidate_commits_to_base(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            init_repo(workspace)
            (workspace / "tracked.txt").write_text("base\n", encoding="utf-8")
            run(workspace, ["git", "add", "tracked.txt"])
            run(workspace, ["git", "commit", "-m", "base"])
            base_ref = run(workspace, ["git", "rev-parse", "HEAD"]).stdout.strip()

            (workspace / "tracked.txt").write_text("candidate\n", encoding="utf-8")
            (workspace / "new.txt").write_text("new\n", encoding="utf-8")
            run(workspace, ["git", "add", "-A"])
            run(workspace, ["git", "commit", "-m", "candidate commit"])
            (workspace / "later.txt").write_text("later\n", encoding="utf-8")
            patch = capture_patch(workspace, history_paths(workspace), "run", base_ref)

            self.assertTrue(patch.has_diff)
            reject_candidate(workspace, patch, base_ref, hard_reset=True)

            self.assertEqual(run(workspace, ["git", "rev-parse", "HEAD"]).stdout.strip(), base_ref)
            self.assertEqual((workspace / "tracked.txt").read_text(encoding="utf-8"), "base\n")
            self.assertFalse((workspace / "new.txt").exists())
            self.assertFalse((workspace / "later.txt").exists())
            status = run(workspace, ["git", "status", "--short"]).stdout.strip()
            self.assertEqual(status, "")

    def test_accepted_candidate_commits_net_change_once_from_base(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            init_repo(workspace)
            (workspace / "tracked.txt").write_text("base\n", encoding="utf-8")
            run(workspace, ["git", "add", "tracked.txt"])
            run(workspace, ["git", "commit", "-m", "base"])
            base_ref = run(workspace, ["git", "rev-parse", "HEAD"]).stdout.strip()

            (workspace / "tracked.txt").write_text("candidate\n", encoding="utf-8")
            run(workspace, ["git", "add", "tracked.txt"])
            run(workspace, ["git", "commit", "-m", "candidate one"])
            (workspace / "new.txt").write_text("new\n", encoding="utf-8")
            run(workspace, ["git", "add", "new.txt"])
            run(workspace, ["git", "commit", "-m", "candidate two"])
            (workspace / "later.txt").write_text("later\n", encoding="utf-8")

            commit = commit_candidate(workspace, None, 0.75, base_ref)

            self.assertEqual(run(workspace, ["git", "rev-parse", "HEAD^"]).stdout.strip(), base_ref)
            self.assertEqual(run(workspace, ["git", "rev-list", "--count", f"{base_ref}..HEAD"]).stdout.strip(), "1")
            self.assertEqual(run(workspace, ["git", "rev-parse", "--short", "HEAD"]).stdout.strip(), commit)
            self.assertEqual((workspace / "tracked.txt").read_text(encoding="utf-8"), "candidate\n")
            self.assertEqual((workspace / "new.txt").read_text(encoding="utf-8"), "new\n")
            self.assertEqual((workspace / "later.txt").read_text(encoding="utf-8"), "later\n")
            status = run(workspace, ["git", "status", "--short"]).stdout.strip()
            self.assertEqual(status, "")

    def test_generation_iteration_accepts_improved_candidate_as_one_commit(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            init_repo(workspace)
            (workspace / "tracked.txt").write_text("base\n", encoding="utf-8")
            run(workspace, ["git", "add", "tracked.txt"])
            run(workspace, ["git", "commit", "-m", "base"])
            base_ref = run(workspace, ["git", "rev-parse", "HEAD"]).stdout.strip()

            args = argparse.Namespace(
                codex_path=None,
                codex_profile=None,
                codex_timeout_seconds=30,
                command_timeout_seconds=30,
                commit_message=None,
                dry_run_codex=False,
                keep_workdirs=False,
                model=None,
                profile="smoke",
                use_current_candidate=False,
                yolo=False,
            )
            evaluation = EvaluationRun(
                observation=EvaluationObservation(
                    gates=[GateObservation("build", True)],
                    cases=[CaseObservation("case", "repo", True, rank=1)],
                    metrics=[MetricObservation("index_ms", 90.0, budget=200.0)],
                ),
                report={"simulated": True},
            )

            def successful_evaluation(_config: object, generated_diff: bool) -> EvaluationRun:
                self.assertTrue(generated_diff)
                return evaluation

            original_run_codex = self_iterate.run_codex
            original_evaluate_candidate = self_iterate.evaluate_candidate
            try:
                self_iterate.run_codex = fake_committing_codex
                self_iterate.evaluate_candidate = successful_evaluation
                with contextlib.redirect_stdout(io.StringIO()):
                    accepted = self_iterate.run_generation_iteration(
                        args,
                        workspace,
                        history_paths(workspace),
                    )
            finally:
                self_iterate.run_codex = original_run_codex
                self_iterate.evaluate_candidate = original_evaluate_candidate

            self.assertTrue(accepted)
            self.assertEqual(run(workspace, ["git", "rev-parse", "HEAD^"]).stdout.strip(), base_ref)
            self.assertEqual(run(workspace, ["git", "rev-list", "--count", f"{base_ref}..HEAD"]).stdout.strip(), "1")
            self.assertEqual((workspace / "tracked.txt").read_text(encoding="utf-8"), "candidate\n")
            self.assertEqual((workspace / "generated.txt").read_text(encoding="utf-8"), "generated\n")
            status = run(workspace, ["git", "status", "--short"]).stdout.strip()
            self.assertEqual(status, "")

    def test_loop_exits_once_when_worktree_starts_dirty(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            init_repo(workspace)
            (workspace / "tracked.txt").write_text("base\n", encoding="utf-8")
            run(workspace, ["git", "add", "tracked.txt"])
            run(workspace, ["git", "commit", "-m", "base"])
            (workspace / "tracked.txt").write_text("dirty\n", encoding="utf-8")

            args = argparse.Namespace(
                fail_fast=False,
                max_iterations=3,
                sleep_seconds=0,
                stop_after_accepted=None,
                use_current_candidate=False,
            )
            stdout = io.StringIO()
            stderr = io.StringIO()

            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                exit_code = run_loop(args, workspace, history_paths(workspace))

            self.assertEqual(exit_code, 1)
            self.assertIn("[self-iterate] cannot start:", stderr.getvalue())
            self.assertNotIn("[self-iterate] iteration 1 starting", stdout.getvalue())


def init_repo(workspace: Path) -> None:
    run(workspace, ["git", "init"])
    run(workspace, ["git", "config", "user.email", "relay@example.invalid"])
    run(workspace, ["git", "config", "user.name", "Relay Test"])


def fake_committing_codex(config: object, _prompt: str) -> CodexResult:
    workspace = config.workspace
    (workspace / "tracked.txt").write_text("candidate\n", encoding="utf-8")
    run(workspace, ["git", "add", "tracked.txt"])
    run(workspace, ["git", "commit", "-m", "candidate one"])
    (workspace / "generated.txt").write_text("generated\n", encoding="utf-8")
    run(workspace, ["git", "add", "generated.txt"])
    run(workspace, ["git", "commit", "-m", "candidate two"])
    return CodexResult(
        command=["codex"],
        exit_code=0,
        duration_ms=1,
        stdout="",
        stderr="",
    )


def run(workspace: Path, command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=workspace,
        text=True,
        capture_output=True,
        check=True,
    )


if __name__ == "__main__":
    unittest.main()
