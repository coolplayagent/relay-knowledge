from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from history import history_paths
from memory import load_memory_index, progressive_memory_index, write_memory_index, write_run_memory


class MemoryTests(unittest.TestCase):
    def test_write_run_memory_persists_index_summary_and_detail(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)

            items = write_run_memory(
                paths,
                {
                    "run_id": "accepted",
                    "timestamp": "2026-05-17T00:00:00+00:00",
                    "accepted": True,
                    "score": 0.95,
                    "accuracy": 1.0,
                    "performance": 0.8,
                    "stability": 1.0,
                    "patch": str(paths.patches / "accepted.patch"),
                    "report": str(paths.reports / "accepted.json"),
                    "reject_reasons": [],
                    "improvements": [
                        {
                            "kind": "metric",
                            "name": "leveldb_cpp_index_ms",
                            "previous": 8000,
                            "current": 6000,
                        }
                    ],
                    "degradations": [],
                    "optimization_plan": {
                        "changed_paths": ["src/relay_knowledge/storage/sqlite/code_query.rs"]
                    },
                    "gates": [{"name": "cargo_test", "passed": True}],
                    "cases": [],
                    "metrics": [{"name": "leveldb_cpp_index_ms", "value": 6000}],
                },
            )

            self.assertEqual(len(items), 1)
            index = load_memory_index(paths)
            self.assertEqual(index[0]["kind"], "accepted_optimization")
            summary = Path(index[0]["summary_path"]).read_text(encoding="utf-8")
            detail = Path(index[0]["detail_path"]).read_text(encoding="utf-8")
            self.assertIn("Accepted run accepted", summary)
            self.assertIn("Changed Paths", detail)
            self.assertIn("leveldb_cpp_index_ms", detail)

    def test_write_run_memory_replaces_same_id_without_dropping_unrelated_items(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            base_record = {
                "run_id": "same",
                "timestamp": "2026-05-17T00:00:00+00:00",
                "accepted": False,
                "score": 0.1,
                "reject_reasons": ["first"],
                "gates": [],
                "cases": [],
                "metrics": [],
            }
            unrelated = dict(base_record, run_id="other", reject_reasons=["other"])

            write_run_memory(paths, base_record)
            write_run_memory(paths, unrelated)
            write_run_memory(paths, dict(base_record, score=0.2, reject_reasons=["second"]))

            index = load_memory_index(paths)
            self.assertEqual(len(index), 2)
            same = [item for item in index if item["id"] == "same-rejected_attempt"][0]
            self.assertEqual(same["score_impact"]["score"], 0.2)
            self.assertTrue(any(item["id"] == "other-rejected_attempt" for item in index))

    def test_progressive_memory_index_exposes_paths_not_full_details(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            write_run_memory(
                paths,
                {
                    "run_id": "failed",
                    "timestamp": "2026-05-17T00:00:00+00:00",
                    "accepted": False,
                    "score": 0.2,
                    "reject_reasons": ["quality gates failed: cargo_test"],
                    "gates": [{"name": "cargo_test", "passed": False, "message": "failure"}],
                    "degradations": [
                        {
                            "kind": "case",
                            "case_id": "linux_definition_start_kernel",
                            "previous": {"passed": True},
                            "current": {"passed": False},
                        }
                    ],
                    "cases": [],
                    "metrics": [],
                },
            )

            rendered = progressive_memory_index(paths)

            self.assertIn("summary_path=", rendered)
            self.assertIn("detail_path=", rendered)
            self.assertIn("quality_gate_failure", rendered)
            self.assertIn("foundational_capability_regression", rendered)
            self.assertNotIn("## Score", rendered)

    def test_competitive_case_regression_gets_dedicated_memory_kind(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            write_run_memory(
                paths,
                {
                    "run_id": "competitive-regression",
                    "timestamp": "2026-05-17T00:00:00+00:00",
                    "accepted": False,
                    "score": 0.4,
                    "reject_reasons": ["protected competitive_capability objective regressed"],
                    "gates": [],
                    "degradations": [
                        {
                            "kind": "case",
                            "objective": "competitive_capability",
                            "case_id": "rt_hybrid_eval_checkpoint_store",
                            "previous": {"passed": True},
                            "current": {"passed": False},
                        }
                    ],
                    "cases": [],
                    "metrics": [],
                },
            )

            kinds = {item["kind"] for item in load_memory_index(paths)}

            self.assertIn("competitive_capability_regression", kinds)

    def test_semantic_vector_case_regression_gets_dedicated_memory_kind(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            write_run_memory(
                paths,
                {
                    "run_id": "sv-regression",
                    "timestamp": "2026-05-17T00:00:00+00:00",
                    "accepted": False,
                    "score": 0.4,
                    "reject_reasons": ["protected semantic_vector objective regressed"],
                    "gates": [],
                    "degradations": [
                        {
                            "kind": "case",
                            "objective": "semantic_vector",
                            "case_id": "sv_semantic_context_pack_source",
                            "previous": {"passed": True},
                            "current": {"passed": False},
                        }
                    ],
                    "cases": [],
                    "metrics": [],
                },
            )

            kinds = {item["kind"] for item in load_memory_index(paths)}

            self.assertIn("semantic_vector_regression", kinds)

    def test_memory_index_is_jsonl(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            write_run_memory(
                paths,
                {
                    "run_id": "jsonl",
                    "timestamp": "2026-05-17T00:00:00+00:00",
                    "accepted": False,
                    "score": 0.0,
                    "reject_reasons": ["no diff"],
                    "gates": [],
                    "cases": [],
                    "metrics": [],
                },
            )

            line = paths.memory_index.read_text(encoding="utf-8").strip()
            self.assertEqual(json.loads(line)["id"], "jsonl-rejected_attempt")

    def test_malformed_memory_index_lines_are_ignored(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            paths.memory.mkdir(parents=True)
            paths.memory_index.write_text(
                '{"id":"valid","kind":"accepted_optimization"}\n'
                '{"id":\n'
                '[]\n',
                encoding="utf-8",
            )

            index = load_memory_index(paths)
            rendered = progressive_memory_index(paths)

            self.assertEqual([item["id"] for item in index], ["valid"])
            self.assertIn("id=valid", rendered)

    def test_memory_index_writes_through_temp_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)

            write_memory_index(paths, [{"id": "one", "kind": "accepted_optimization"}])

            self.assertEqual(load_memory_index(paths)[0]["id"], "one")
            self.assertFalse(paths.memory_index.with_suffix(".jsonl.tmp").exists())


if __name__ == "__main__":
    unittest.main()
