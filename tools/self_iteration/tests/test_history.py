from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from history import append_run, best_accepted_run, export_history, history_paths


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
            self.assertIn("run_id", csv_path.read_text(encoding="utf-8"))
            self.assertIn("<svg", svg_path.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
