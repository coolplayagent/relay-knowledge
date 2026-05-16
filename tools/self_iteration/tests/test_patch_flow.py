from __future__ import annotations

import argparse
import contextlib
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import self_iterate
from codex_driver import CodexResult
from evaluator import EvaluationRun, register_command
from history import append_run, history_paths
from scoring import CaseObservation, EvaluationObservation, GateObservation, MetricObservation
from self_iterate import capture_patch, commit_candidate, reject_candidate, run_loop


class PatchFlowTests(unittest.TestCase):
    def test_prompt_includes_recent_rejected_reasons_as_negative_context(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            append_run(
                paths,
                {
                    "run_id": "accepted",
                    "timestamp": "2026-05-15T00:00:00+00:00",
                    "accepted": True,
                    "score": 0.91,
                    "accuracy": 1.0,
                    "stability": 1.0,
                    "commit": "abc123",
                    "reject_reasons": [],
                    "report": "accepted.json",
                },
            )
            append_run(
                paths,
                {
                    "run_id": "rejected_without_reason",
                    "timestamp": "2026-05-15T00:01:00+00:00",
                    "accepted": False,
                    "reject_reasons": [],
                    "report": "ignored.json",
                },
            )
            for index in range(4):
                append_run(
                    paths,
                    {
                        "run_id": f"rejected_{index}",
                        "timestamp": f"2026-05-15T00:0{index + 2}:00+00:00",
                        "accepted": False,
                        "score": index / 10,
                        "accuracy": index / 20,
                        "stability": index / 30,
                        "reject_reasons": [f"reason_{index}"],
                        "report": f"report_{index}.json",
                    },
                )

            prompt = self_iterate.build_prompt(paths, "next")

            self.assertIn("Recent rejected attempts to avoid:", prompt)
            self.assertIn("run_id=rejected_3", prompt)
            self.assertIn("run_id=rejected_2", prompt)
            self.assertIn("run_id=rejected_1", prompt)
            self.assertIn("reasons=reason_3", prompt)
            self.assertIn("report=report_3.json", prompt)
            self.assertNotIn("run_id=rejected_0", prompt)
            self.assertNotIn("rejected_without_reason", prompt)
            self.assertNotIn("run_id=accepted", prompt)

    def test_prompt_requires_algorithm_documentation_and_indexes_patch_memory(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            paths.patches.mkdir(parents=True)
            patch_path = paths.patches / "20260516T000000Z.patch"
            patch_path.write_text(
                "\n".join(
                    [
                        "diff --git a/src/relay_knowledge/storage/sqlite/code_query.rs b/src/relay_knowledge/storage/sqlite/code_query.rs",
                        "--- a/src/relay_knowledge/storage/sqlite/code_query.rs",
                        "+++ b/src/relay_knowledge/storage/sqlite/code_query.rs",
                    ]
                ),
                encoding="utf-8",
            )
            append_run(
                paths,
                {
                    "run_id": "patch-memory",
                    "timestamp": "2026-05-15T00:00:00+00:00",
                    "accepted": False,
                    "score": 0.82,
                    "reject_reasons": ["quality gates failed: linux_sample_index"],
                    "patch": str(patch_path),
                    "optimization_plan": {
                        "changed_paths": ["src/relay_knowledge/storage/sqlite/code_query.rs"]
                    },
                },
            )

            prompt = self_iterate.build_prompt(paths, "next")

            self.assertIn("must also update docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md", prompt)
            self.assertIn("algorithm, architecture, invariants", prompt)
            self.assertIn("Long-term patch memory index:", prompt)
            self.assertIn(str(patch_path), prompt)
            self.assertIn("status=rejected", prompt)
            self.assertIn("changed_paths=src/relay_knowledge/storage/sqlite/code_query.rs", prompt)

    def test_prompt_includes_failed_gate_command_diagnostics(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            paths.reports.mkdir(parents=True)
            report_path = paths.reports / "failed.json"
            report_path.write_text(
                json.dumps(
                    {
                        "evaluation": {
                            "commands": [
                                {
                                    "name": "leveldb_cpp_index",
                                    "command": [
                                        "target/release/relay-knowledge",
                                        "repo",
                                        "index",
                                        "leveldb-cpp-self-iteration",
                                        "--ref",
                                        "HEAD",
                                    ],
                                    "exit_code": 1,
                                    "duration_ms": 1697,
                                    "stdout_tail": "",
                                    "stderr_tail": (
                                        "sqlite operation failed: UNIQUE constraint failed: "
                                        "code_repository_symbols.source_scope, "
                                        "code_repository_symbols.symbol_snapshot_id\n"
                                    ),
                                }
                            ]
                        }
                    },
                    sort_keys=True,
                ),
                encoding="utf-8",
            )
            append_run(
                paths,
                {
                    "run_id": "failed-gate",
                    "timestamp": "2026-05-15T00:00:00+00:00",
                    "accepted": False,
                    "score": 0.89,
                    "accuracy": 0.82,
                    "stability": 0.97,
                    "reject_reasons": ["quality gates failed: leveldb_cpp_index"],
                    "report": str(report_path),
                    "gates": [
                        {
                            "name": "leveldb_cpp_index",
                            "passed": False,
                            "duration_ms": 1697,
                            "message": "sqlite operation failed",
                        }
                    ],
                },
            )

            prompt = self_iterate.build_prompt(paths, "next")

            self.assertIn("Recent failed quality gate diagnostics:", prompt)
            self.assertIn("run_id=failed-gate", prompt)
            self.assertIn("gate=leveldb_cpp_index", prompt)
            self.assertIn("target/release/relay-knowledge repo index", prompt)
            self.assertIn("exit_code=1", prompt)
            self.assertIn("UNIQUE constraint failed", prompt)
            self.assertIn("code_repository_symbols.symbol_snapshot_id", prompt)

    def test_prompt_prioritizes_recent_quality_gate_failures(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            append_run(
                paths,
                {
                    "run_id": "failed-large-repos",
                    "timestamp": "2026-05-15T00:00:00+00:00",
                    "accepted": False,
                    "reject_reasons": [
                        "quality gates failed: linux_sample_index, kubernetes_go_sample_index"
                    ],
                    "gates": [
                        {
                            "name": "linux_sample_index",
                            "passed": False,
                            "duration_ms": 900_000,
                            "message": "timeout after 900s",
                        },
                        {
                            "name": "kubernetes_go_sample_index",
                            "passed": False,
                            "duration_ms": 900_000,
                            "message": "timeout after 900s",
                        },
                    ],
                },
            )

            prompt = self_iterate.build_prompt(paths, "next")

            self.assertIn("Quality gate repair mode is active", prompt)
            self.assertIn("linux_sample_index", prompt)
            self.assertIn("kubernetes_go_sample_index", prompt)
            self.assertIn("Prioritize fixing these failed gates", prompt)

    def test_candidate_documentation_gate_rejects_undocumented_implementation(self) -> None:
        patch = self_iterate.PatchSnapshot(
            path=Path("candidate.patch"),
            diff=(
                "diff --git a/src/relay_knowledge/code.rs b/src/relay_knowledge/code.rs\n"
                "--- a/src/relay_knowledge/code.rs\n"
                "+++ b/src/relay_knowledge/code.rs\n"
            ),
            sha256="abc",
            base_ref="HEAD",
        )

        gate = self_iterate.candidate_documentation_gate(patch)

        self.assertEqual(gate.name, "self_iteration_algorithm_documentation")
        self.assertFalse(gate.passed)
        self.assertIn("missing candidate algorithm and architecture notes", gate.message)

    def test_candidate_documentation_gate_accepts_documented_implementation(self) -> None:
        patch = self_iterate.PatchSnapshot(
            path=Path("candidate.patch"),
            diff=(
                "diff --git a/src/relay_knowledge/code.rs b/src/relay_knowledge/code.rs\n"
                "--- a/src/relay_knowledge/code.rs\n"
                "+++ b/src/relay_knowledge/code.rs\n"
                "diff --git a/docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md b/docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md\n"
                "--- a/docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md\n"
                "+++ b/docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md\n"
            ),
            sha256="abc",
            base_ref="HEAD",
        )

        gate = self_iterate.candidate_documentation_gate(patch)

        self.assertTrue(gate.passed)

    def test_prompt_describes_missing_rejected_history(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            prompt = self_iterate.build_prompt(history_paths(workspace), "next")

            self.assertIn("No rejected historical run with reasons yet.", prompt)
            self.assertIn("No failed quality gate diagnostics recorded yet.", prompt)
            self.assertIn("No recent quality gate failures recorded", prompt)
            self.assertIn("No worsened evaluation items recorded yet.", prompt)
            self.assertIn("No improved evaluation items recorded yet.", prompt)
            self.assertIn("No historical patch files recorded yet.", prompt)

    def test_prompt_includes_recent_degradations_as_next_context(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            append_run(
                paths,
                {
                    "run_id": "worse",
                    "timestamp": "2026-05-15T00:00:00+00:00",
                    "accepted": False,
                    "reject_reasons": ["score 0.4 did not improve previous 0.5"],
                    "degradations": [
                        {
                            "kind": "metric",
                            "name": "linux_sample_index_ms",
                            "previous": 4000.0,
                            "current": 4500.0,
                            "message": "",
                        },
                        {
                            "kind": "case",
                            "case_id": "linux_definition_start_kernel",
                            "previous": {"passed": True, "rank": 1},
                            "current": {"passed": False, "rank": None},
                            "message": "results=0 rank=None",
                        },
                    ],
                },
            )

            prompt = self_iterate.build_prompt(paths, "next")

            self.assertIn("Recent worsened evaluation items:", prompt)
            self.assertIn("kind=metric name=linux_sample_index_ms", prompt)
            self.assertIn("kind=case name=linux_definition_start_kernel", prompt)

    def test_prompt_includes_recent_improvements_to_preserve(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            append_run(
                paths,
                {
                    "run_id": "better",
                    "timestamp": "2026-05-15T00:00:00+00:00",
                    "accepted": True,
                    "reject_reasons": [],
                    "improvements": [
                        {
                            "kind": "metric",
                            "name": "leveldb_cpp_index_ms",
                            "previous": 8000.0,
                            "current": 6000.0,
                            "message": "",
                        },
                        {
                            "kind": "case",
                            "case_id": "kubernetes_definition_run_kubelet",
                            "previous": {"passed": False, "rank": None},
                            "current": {"passed": True, "rank": 1},
                            "message": "results=1 rank=1",
                        },
                    ],
                },
            )

            prompt = self_iterate.build_prompt(paths, "next")

            self.assertIn("Recent improved evaluation items to preserve:", prompt)
            self.assertIn("kind=metric name=leveldb_cpp_index_ms", prompt)
            self.assertIn("kind=case name=kubernetes_definition_run_kubelet", prompt)

    def test_prompt_includes_recent_adopted_optimization_plan(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            append_run(
                paths,
                {
                    "run_id": "accepted-plan",
                    "timestamp": "2026-05-15T00:00:00+00:00",
                    "accepted": True,
                    "score": 0.93,
                    "commit": "abc123",
                    "reject_reasons": [],
                    "optimization_plan": {
                        "changed_paths": ["src/relay_knowledge/storage/sqlite/code_query.rs"],
                        "key_improvements": ["metric:linux_sample_query_p95_ms 900->600"],
                        "codex_notes": "Use SQLite preselection before in-memory rerank.",
                    },
                },
            )

            prompt = self_iterate.build_prompt(paths, "next")

            self.assertIn("Recent adopted optimization plans to build on:", prompt)
            self.assertIn("run_id=accepted-plan", prompt)
            self.assertIn("commit=abc123", prompt)
            self.assertIn("SQLite preselection", prompt)

    def test_full_scope_register_ignores_repository_filters(self) -> None:
        command = register_command(
            Path("/bin/relay-knowledge"),
            Path("/repo"),
            "repo-alias",
            {
                "scope": "all",
                "path_filters": ["src/only.rs"],
                "language_filters": ["rust"],
            },
        )

        self.assertNotIn("--path", command)
        self.assertNotIn("--language", command)
        self.assertEqual(command[-2:], ["--format", "json"])

    def test_adopted_optimization_document_is_written(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            patch = self_iterate.PatchSnapshot(
                path=workspace / ".git" / "relay-knowledge-self-iteration" / "patches" / "run.patch",
                diff="diff --git a/src/a.rs b/src/a.rs\n",
                sha256="abc",
                base_ref="HEAD",
            )
            evaluation = EvaluationRun(
                observation=EvaluationObservation(
                    gates=[GateObservation("cargo_build_release", True)],
                    cases=[CaseObservation("case", "repo", True, rank=1)],
                    metrics=[MetricObservation("repo_query_p95_ms", 42.0, budget=100.0)],
                ),
                report={},
            )

            self_iterate.write_adopted_optimization_document(
                workspace=workspace,
                run_id="run",
                patch=patch,
                score={"score": 1.0, "accuracy": 1.0, "performance": 1.0, "stability": 1.0},
                evaluation=evaluation,
                optimization_plan={
                    "changed_paths": ["src/a.rs"],
                    "key_improvements": ["case:case failed->passed"],
                    "known_degradations": [],
                    "codex_notes": "Adopt a bounded candidate index.",
                },
            )

            doc = (
                workspace
                / "docs"
                / "zh"
                / "05-benchmarks"
                / "self-iteration-accepted-optimizations.md"
            ).read_text(encoding="utf-8")
            self.assertIn("## run", doc)
            self.assertIn("bounded candidate index", doc)
            self.assertIn("`src/a.rs`", doc)

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
            notes = workspace / "docs" / "zh" / "05-benchmarks" / "self-iteration-accepted-optimizations.md"
            self.assertIn("candidate algorithm", notes.read_text(encoding="utf-8"))
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
    notes = workspace / "docs" / "zh" / "05-benchmarks" / "self-iteration-accepted-optimizations.md"
    notes.parent.mkdir(parents=True)
    notes.write_text("candidate algorithm and architecture notes\n", encoding="utf-8")
    run(workspace, ["git", "add", "generated.txt", str(notes)])
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
