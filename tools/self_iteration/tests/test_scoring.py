from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scoring import (
    CaseObservation,
    DEFAULT_WEIGHTS,
    EvaluationObservation,
    GateObservation,
    JUDGE_WEIGHTS,
    MetricObservation,
    score_evaluation,
)


def passing_case(
    case_id: str,
    objective: str = "foundational_capability",
    repository: str = "repo",
    rank: int = 1,
    max_rank: int = 1,
) -> CaseObservation:
    return CaseObservation(
        case_id,
        repository,
        True,
        rank=rank,
        max_rank=max_rank,
        objective=objective,
    )


def full_objective_cases() -> list[CaseObservation]:
    return [
        passing_case("foundation"),
        passing_case("competitive", objective="competitive_capability"),
        passing_case("semantic", objective="semantic_vector", repository="semantic_vector"),
    ]


def research_judge_case(score: float = 0.9) -> CaseObservation:
    return CaseObservation(
        "research_judge",
        "research_judge",
        True,
        rank=1,
        objective="research_judge",
        score_override=score,
    )


class ScoringTests(unittest.TestCase):
    def test_weight_policy_emphasizes_research_and_performance(self) -> None:
        self.assertAlmostEqual(sum(DEFAULT_WEIGHTS.values()), 1.0)
        self.assertEqual(DEFAULT_WEIGHTS["performance"], 0.18)
        self.assertAlmostEqual(sum(JUDGE_WEIGHTS.values()), 1.0)
        self.assertEqual(JUDGE_WEIGHTS["research_judge"], 0.22)
        self.assertEqual(JUDGE_WEIGHTS["performance"], 0.15)

        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=full_objective_cases() + [research_judge_case(0.5)],
                metrics=[MetricObservation("query_p95_ms", 200.0, budget=100.0)],
            ),
            previous_run=None,
        )

        expected = (
            JUDGE_WEIGHTS["foundational_capability"]
            + JUDGE_WEIGHTS["competitive_capability"]
            + JUDGE_WEIGHTS["semantic_vector"]
            + 0.5 * JUDGE_WEIGHTS["research_judge"]
            + 0.5 * JUDGE_WEIGHTS["performance"]
            + JUDGE_WEIGHTS["stability"]
        )
        self.assertAlmostEqual(score.score, expected)

    def test_first_successful_candidate_is_accepted(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=full_objective_cases(),
                metrics=[MetricObservation("index_ms", 100.0, budget=200.0)],
            ),
            previous_run=None,
        )

        self.assertTrue(score.accepted)
        self.assertGreater(score.score, 0.9)

    def test_passed_case_score_still_discounts_non_top_rank(self) -> None:
        self.assertEqual(passing_case("rank_one", rank=1).score(), 1.0)
        self.assertEqual(passing_case("rank_two", rank=2, max_rank=3).score(), 0.5)
        self.assertEqual(passing_case("rank_five", rank=5, max_rank=5).score(), 0.2)

    def test_empty_negative_case_scores_full_when_passed(self) -> None:
        case = CaseObservation(
            "negative",
            "repo",
            True,
            rank=0,
            max_rank=1,
        )

        self.assertEqual(case.score(), 1.0)

    def test_missing_objectives_default_to_zero_not_full(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[passing_case("definition")],
                metrics=[MetricObservation("query_p95_ms", 100.0, budget=200.0)],
            ),
            previous_run=None,
        )

        self.assertEqual(score.foundational_capability, 1.0)
        self.assertEqual(score.competitive_capability, 0.0)
        self.assertEqual(score.semantic_vector, 0.0)
        self.assertIsNone(score.research_judge)
        self.assertEqual(score.accuracy, 1.0)

    def test_metric_budget_failures_are_reported(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=full_objective_cases(),
                metrics=[MetricObservation("query_p95_ms", 700.0, budget=500.0)],
            ),
            previous_run=None,
        )

        self.assertEqual(
            score.to_dict()["metric_budget_failures"],
            [
                {
                    "name": "query_p95_ms",
                    "value": 700.0,
                    "budget": 500.0,
                    "lower_is_better": True,
                    "score": 0.714286,
                }
            ],
        )

    def test_policy_rejects_previous_score_regression(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=full_objective_cases(),
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
                cases=full_objective_cases(),
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
                cases=full_objective_cases(),
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
                cases=full_objective_cases(),
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

    def test_foundational_capability_regression_is_reported_and_rejected(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[CaseObservation("case", "repo", False, rank=None, message="missing")],
                metrics=[MetricObservation("index_ms", 1.0, budget=200.0)],
            ),
            previous_run={
                "score": 0.1,
                "foundational_capability": 1.0,
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
            any(
                "protected foundational_capability objective regressed" in reason
                for reason in score.reject_reasons
            )
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
                cases=full_objective_cases(),
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
                    passing_case("competitive", objective="competitive_capability"),
                    passing_case(
                        "semantic",
                        objective="semantic_vector",
                        repository="semantic_vector",
                    ),
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
                    passing_case("competitive", objective="competitive_capability"),
                    passing_case(
                        "semantic",
                        objective="semantic_vector",
                        repository="semantic_vector",
                    ),
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

    def test_semantic_vector_objective_is_reported_separately(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[
                    CaseObservation("code", "repo", True, rank=1),
                    CaseObservation(
                        "semantic",
                        "semantic_vector",
                        False,
                        rank=None,
                        objective="semantic_vector",
                    ),
                ],
                metrics=[MetricObservation("query_p95_ms", 100.0, budget=200.0)],
            ),
            previous_run=None,
        )

        self.assertEqual(score.accuracy, 1.0)
        self.assertEqual(score.semantic_vector, 0.0)
        self.assertEqual(score.to_dict()["semantic_vector"], 0.0)

    def test_foundational_and_competitive_objectives_roll_up_to_accuracy(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[
                    CaseObservation(
                        "definition",
                        "repo",
                        True,
                        rank=1,
                        objective="foundational_capability",
                    ),
                    CaseObservation(
                        "hybrid",
                        "repo",
                        False,
                        rank=None,
                        objective="competitive_capability",
                    ),
                ],
                metrics=[MetricObservation("query_p95_ms", 100.0, budget=200.0)],
            ),
            previous_run=None,
        )

        self.assertEqual(score.foundational_capability, 1.0)
        self.assertEqual(score.competitive_capability, 0.0)
        self.assertEqual(score.accuracy, 0.5)
        self.assertEqual(score.to_dict()["competitive_capability"], 0.0)

    def test_competitive_capability_regression_is_protected(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[
                    CaseObservation(
                        "hybrid",
                        "repo",
                        False,
                        rank=None,
                        objective="competitive_capability",
                    ),
                    passing_case(
                        "semantic",
                        objective="semantic_vector",
                        repository="semantic_vector",
                    ),
                ],
                metrics=[MetricObservation("query_p95_ms", 100.0, budget=200.0)],
            ),
            previous_run={
                "score": 0.1,
                "competitive_capability": 1.0,
                "semantic_vector": 1.0,
                "stability": 1.0,
            },
        )

        self.assertFalse(score.accepted)
        self.assertTrue(
            any(
                "protected competitive_capability objective regressed" in reason
                for reason in score.reject_reasons
            )
        )

    def test_semantic_vector_regression_is_protected(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=[
                    CaseObservation("code", "repo", True, rank=1),
                    CaseObservation(
                        "semantic",
                        "semantic_vector",
                        False,
                        rank=None,
                        objective="semantic_vector",
                    ),
                ],
                metrics=[MetricObservation("query_p95_ms", 100.0, budget=200.0)],
            ),
            previous_run={
                "score": 0.1,
                "accuracy": 1.0,
                "semantic_vector": 1.0,
                "stability": 1.0,
            },
        )

        self.assertFalse(score.accepted)
        self.assertTrue(
            any(
                "protected semantic_vector objective regressed" in reason
                for reason in score.reject_reasons
            )
        )

    def test_research_judge_score_override_is_reported_when_present(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=full_objective_cases() + [research_judge_case(0.82)],
                metrics=[MetricObservation("query_p95_ms", 100.0, budget=200.0)],
            ),
            previous_run=None,
        )

        self.assertEqual(score.research_judge, 0.82)
        self.assertEqual(score.to_dict()["research_judge"], 0.82)
        self.assertTrue(score.accepted)

    def test_research_judge_regression_is_protected_when_configured(self) -> None:
        score = score_evaluation(
            EvaluationObservation(
                gates=[GateObservation("build", True)],
                cases=full_objective_cases() + [research_judge_case(0.4)],
                metrics=[MetricObservation("query_p95_ms", 100.0, budget=200.0)],
            ),
            previous_run={
                "score": 0.1,
                "foundational_capability": 1.0,
                "competitive_capability": 1.0,
                "semantic_vector": 1.0,
                "research_judge": 0.9,
                "stability": 1.0,
            },
        )

        self.assertFalse(score.accepted)
        self.assertTrue(
            any(
                "protected research_judge objective regressed" in reason
                for reason in score.reject_reasons
            )
        )


if __name__ == "__main__":
    unittest.main()
