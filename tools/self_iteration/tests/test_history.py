from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from history import (
    append_run,
    best_accepted_run,
    ensure_history,
    export_history,
    history_paths,
    previous_scored_run,
)


class HistoryTests(unittest.TestCase):
    def test_history_exports_csv_and_svg(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            append_run(
                paths,
                {
                    "run_id": "one",
                    "timestamp": "2026-05-15T00:00:00+00:00",
                    "accepted": False,
                    "score": 0.25,
                    "accuracy": 0.5,
                    "performance": 0.25,
                    "stability": 0.0,
                    "reject_reasons": ["not enough"],
                },
            )
            append_run(
                paths,
                {
                    "run_id": "two",
                    "timestamp": "2026-05-15T00:01:00+00:00",
                    "accepted": True,
                    "score": 0.75,
                    "accuracy": 1.0,
                    "performance": 0.5,
                    "stability": 1.0,
                    "commit": "abc123",
                    "reject_reasons": [],
                },
            )

            csv_path, svg_path = export_history(paths)

            self.assertEqual(best_accepted_run(paths)["run_id"], "two")
            self.assertEqual(previous_scored_run(paths)["run_id"], "two")
            self.assertIn("run_id", csv_path.read_text(encoding="utf-8"))
            self.assertIn("<svg", svg_path.read_text(encoding="utf-8"))

    def test_previous_scored_run_uses_latest_timestamp_not_best_score(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)
            append_run(
                paths,
                {
                    "run_id": "best",
                    "timestamp": "2026-05-15T00:00:00+00:00",
                    "accepted": True,
                    "score": 0.99,
                },
            )
            append_run(
                paths,
                {
                    "run_id": "latest",
                    "timestamp": "2026-05-15T00:01:00+00:00",
                    "accepted": False,
                    "score": 0.50,
                },
            )

            self.assertEqual(previous_scored_run(paths)["run_id"], "latest")

    def test_history_initializes_progressive_memory_directories(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            (workspace / ".git").mkdir()
            paths = history_paths(workspace)

            ensure_history(paths)

            self.assertTrue(paths.memory_summaries.is_dir())
            self.assertTrue(paths.memory_details.is_dir())
            self.assertTrue(paths.memory_artifacts.is_dir())


if __name__ == "__main__":
    unittest.main()
