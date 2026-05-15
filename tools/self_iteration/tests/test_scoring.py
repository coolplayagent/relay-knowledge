from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scoring import (
    CaseObservation,
    EvaluationObservation,
    GateObservation,
    MetricObservation,
    score_evaluation,
)


class ScoringTests(unittest.TestCase):
    def test_first_successful_candidate_is_accepted(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[CaseObservation("case", "repo", True, rank=1)],
                metrics=[MetricObservation("index_ms", 100.0, budget=200.0)],
            ),
            best_previous=None,
        )

        self.assertTrue(score.accepted)
        self.assertGreater(score.score, 0.9)

    def test_strict_policy_rejects_score_regression(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[CaseObservation("case", "repo", True, rank=1)],
                metrics=[MetricObservation("index_ms", 100.0, budget=200.0)],
            ),
            best_previous={
                "score": 1.0,
                "accuracy": 1.0,
                "metrics": [{"name": "index_ms", "value": 100.0}],
            },
        )

        self.assertFalse(score.accepted)
        self.assertTrue(any("did not strictly improve" in reason for reason in score.reject_reasons))

    def test_score_improvement_is_accepted(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[CaseObservation("case", "repo", True, rank=1)],
                metrics=[MetricObservation("index_ms", 90.0, budget=200.0)],
            ),
            best_previous={
                "score": 0.75,
                "accuracy": 1.0,
                "metrics": [{"name": "index_ms", "value": 100.0}],
            },
        )

        self.assertTrue(score.accepted)
        self.assertGreater(score.score, 0.75)

    def test_key_metric_regression_is_rejected(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[CaseObservation("case", "repo", True, rank=1)],
                metrics=[MetricObservation("index_ms", 110.0, budget=200.0)],
            ),
            best_previous={
                "score": 0.5,
                "accuracy": 1.0,
                "metrics": [{"name": "index_ms", "value": 100.0}],
            },
        )

        self.assertFalse(score.accepted)
        self.assertTrue(any("index_ms regressed" in reason for reason in score.reject_reasons))


if __name__ == "__main__":
    unittest.main()
