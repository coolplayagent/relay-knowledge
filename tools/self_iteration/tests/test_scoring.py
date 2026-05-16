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
            previous_run=None,
        )

        self.assertTrue(score.accepted)
        self.assertGreater(score.score, 0.9)

    def test_policy_rejects_previous_score_regression(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[CaseObservation("case", "repo", True, rank=1)],
                metrics=[MetricObservation("index_ms", 100.0, budget=200.0)],
            ),
            previous_run={
                "score": 1.0,
                "accuracy": 1.0,
                "metrics": [{"name": "index_ms", "value": 100.0}],
            },
        )

        self.assertFalse(score.accepted)
        self.assertTrue(any("epsilon-pareto" in reason for reason in score.reject_reasons))

    def test_any_score_improvement_over_previous_is_accepted(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[CaseObservation("case", "repo", True, rank=1)],
                metrics=[MetricObservation("index_ms", 90.0, budget=200.0)],
            ),
            previous_run={
                "score": 0.75,
                "accuracy": 1.0,
                "metrics": [{"name": "index_ms", "value": 100.0}],
            },
        )

        self.assertTrue(score.accepted)
        self.assertGreater(score.score, 0.75)

    def test_key_metric_regression_is_reported_without_standalone_rejection(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[CaseObservation("case", "repo", True, rank=1)],
                metrics=[MetricObservation("index_ms", 140.0, budget=200.0)],
            ),
            previous_run={
                "score": 0.5,
                "accuracy": 1.0,
                "metrics": [{"name": "index_ms", "value": 100.0}],
            },
        )

        self.assertTrue(score.accepted)
        self.assertEqual(score.reject_reasons, [])
        self.assertTrue(
            any(
                degradation["kind"] == "metric" and degradation["name"] == "index_ms"
                for degradation in score.degradations
            )
        )

    def test_quality_gate_failure_remains_hard_rejection(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", False)],
                cases=[CaseObservation("case", "repo", True, rank=1)],
                metrics=[MetricObservation("index_ms", 90.0, budget=200.0)],
            ),
            previous_run={
                "score": 0.5,
                "accuracy": 1.0,
                "metrics": [{"name": "index_ms", "value": 100.0}],
            },
        )

        self.assertFalse(score.accepted)
        self.assertTrue(any("quality gates failed" in reason for reason in score.reject_reasons))

    def test_accuracy_regression_is_reported_and_rejected(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[CaseObservation("case", "repo", False, rank=None, message="missing")],
                metrics=[MetricObservation("index_ms", 1.0, budget=200.0)],
            ),
            previous_run={
                "score": 0.1,
                "accuracy": 1.0,
                "cases": [
                    {
                        "case_id": "case",
                        "repository": "repo",
                        "passed": True,
                        "rank": 1,
                        "max_rank": 1,
                        "false_positive_count": 0,
                    }
                ],
                "metrics": [{"name": "index_ms", "value": 100.0}],
            },
        )

        self.assertFalse(score.accepted)
        self.assertTrue(
            any("protected accuracy objective regressed" in reason for reason in score.reject_reasons)
        )
        self.assertTrue(
            any(
                degradation["kind"] == "case"
                and degradation["case_id"] == "case"
                and degradation["reason"] == "passed_to_failed"
                for degradation in score.degradations
            )
        )

    def test_stability_regression_is_rejected_even_when_latency_improves(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[
                    GateObservation("build", True),
                    GateObservation("clippy", False),
                ],
                cases=[CaseObservation("case", "repo", True, rank=1)],
                metrics=[MetricObservation("index_ms", 1.0, budget=200.0)],
            ),
            previous_run={
                "score": 0.1,
                "accuracy": 1.0,
                "performance": 0.5,
                "stability": 1.0,
                "metrics": [{"name": "index_ms", "value": 100.0}],
                "gates": [
                    {"name": "build", "passed": True},
                    {"name": "clippy", "passed": True},
                ],
            },
        )

        self.assertFalse(score.accepted)
        self.assertTrue(any("quality gates failed" in reason for reason in score.reject_reasons))
        self.assertTrue(
            any("protected stability objective regressed" in reason for reason in score.reject_reasons)
        )

    def test_metric_and_case_improvements_are_reported(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[CaseObservation("case", "repo", True, rank=1)],
                metrics=[MetricObservation("index_ms", 60.0, budget=200.0)],
            ),
            previous_run={
                "score": 0.1,
                "accuracy": 0.0,
                "cases": [
                    {
                        "case_id": "case",
                        "repository": "repo",
                        "passed": False,
                        "rank": None,
                        "max_rank": 1,
                        "false_positive_count": 0,
                    }
                ],
                "metrics": [{"name": "index_ms", "value": 100.0}],
            },
        )

        self.assertTrue(score.accepted)
        self.assertTrue(
            any(
                improvement["kind"] == "metric" and improvement["name"] == "index_ms"
                for improvement in score.improvements
            )
        )
        self.assertTrue(
            any(
                improvement["kind"] == "case"
                and improvement["case_id"] == "case"
                and improvement["reason"] == "failed_to_passed"
                for improvement in score.improvements
            )
        )

    def test_small_metric_noise_does_not_count_as_regression(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[CaseObservation("case", "repo", True, rank=1)],
                metrics=[MetricObservation("query_p95_ms", 1010.0, budget=2000.0)],
            ),
            previous_run={
                "score": 0.9999,
                "accuracy": 1.0,
                "performance": 1.0,
                "stability": 1.0,
                "metrics": [{"name": "query_p95_ms", "value": 1000.0}],
            },
        )

        self.assertFalse(
            any(
                degradation["kind"] == "metric" and degradation["name"] == "query_p95_ms"
                for degradation in score.degradations
            )
        )

    def test_epsilon_pareto_case_improvement_can_accept_flat_score(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[
                    CaseObservation("fixed", "repo", True, rank=1),
                    CaseObservation("steady", "repo", True, rank=1),
                ],
                metrics=[MetricObservation("query_p95_ms", 1000.0, budget=2000.0)],
            ),
            previous_run={
                "score": 0.9999,
                "accuracy": 0.5,
                "performance": 1.0,
                "stability": 1.0,
                "cases": [
                    {
                        "case_id": "fixed",
                        "repository": "repo",
                        "passed": False,
                        "rank": None,
                        "max_rank": 1,
                        "false_positive_count": 0,
                    },
                    {
                        "case_id": "steady",
                        "repository": "repo",
                        "passed": True,
                        "rank": 1,
                        "max_rank": 1,
                        "false_positive_count": 0,
                    },
                ],
                "metrics": [{"name": "query_p95_ms", "value": 1000.0}],
            },
        )

        self.assertTrue(score.accepted)
        self.assertTrue(
            any(
                improvement["kind"] == "case"
                and improvement["case_id"] == "fixed"
                and improvement["reason"] == "failed_to_passed"
                for improvement in score.improvements
            )
        )

    def test_epsilon_pareto_rejects_case_improvement_with_significant_regression(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[
                    CaseObservation("fixed", "repo", True, rank=1),
                    CaseObservation("steady", "repo", True, rank=1),
                ],
                metrics=[MetricObservation("query_p95_ms", 1400.0, budget=2000.0)],
            ),
            previous_run={
                "score": 0.9999,
                "accuracy": 0.5,
                "performance": 1.0,
                "stability": 1.0,
                "cases": [
                    {
                        "case_id": "fixed",
                        "repository": "repo",
                        "passed": False,
                        "rank": None,
                        "max_rank": 1,
                        "false_positive_count": 0,
                    },
                    {
                        "case_id": "steady",
                        "repository": "repo",
                        "passed": True,
                        "rank": 1,
                        "max_rank": 1,
                        "false_positive_count": 0,
                    },
                ],
                "metrics": [{"name": "query_p95_ms", "value": 1000.0}],
            },
        )

        self.assertFalse(score.accepted)
        self.assertTrue(
            any(
                degradation["kind"] == "metric" and degradation["name"] == "query_p95_ms"
                for degradation in score.degradations
            )
        )


if __name__ == "__main__":
    unittest.main()
