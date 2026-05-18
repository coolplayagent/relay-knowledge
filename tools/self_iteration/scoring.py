"""Deterministic scoring policy for self-iteration runs."""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Any


DEFAULT_WEIGHTS = {
    "foundational_capability": 0.22,
    "competitive_capability": 0.22,
    "semantic_vector": 0.13,
    "performance": 0.18,
    "stability": 0.25,
}

JUDGE_WEIGHTS = {
    "foundational_capability": 0.17,
    "competitive_capability": 0.17,
    "semantic_vector": 0.10,
    "research_judge": 0.22,
    "performance": 0.15,
    "stability": 0.19,
}

SCORE_EPSILON = 0.0005
RATIO_EPSILON = 0.005
METRIC_RELATIVE_EPSILON = 0.03
METRIC_ABSOLUTE_EPSILON = 25.0
PERFORMANCE_STRATEGY = "budget_relative_v1"


@dataclass(frozen=True)
class GateObservation:
    name: str
    passed: bool
    duration_ms: int = 0
    message: str = ""


@dataclass(frozen=True)
class CaseObservation:
    case_id: str
    repository: str
    passed: bool
    rank: int | None = None
    max_rank: int = 1
    false_positive_count: int = 0
    message: str = ""
    objective: str = "foundational_capability"
    score_override: float | None = None

    def score(self) -> float:
        if not self.passed:
            return 0.0
        if self.score_override is not None:
            return clamp_score(self.score_override)
        if self.rank is None or self.rank <= 0:
            rank_score = 1.0
        else:
            rank_score = 1.0 / self.rank
        false_positive_penalty = min(0.5, self.false_positive_count * 0.1)
        return max(0.0, rank_score - false_positive_penalty)


@dataclass(frozen=True)
class MetricObservation:
    name: str
    value: float
    budget: float | None = None
    lower_is_better: bool = True
    key: bool = True

    def score(self) -> float:
        if self.value < 0:
            return 0.0
        if self.budget is None or self.budget <= 0:
            return 1.0
        if self.lower_is_better:
            return min(1.0, self.budget / max(self.value, 1.0))
        return min(1.0, self.value / self.budget)


@dataclass(frozen=True)
class EvaluationObservation:
    gates: list[GateObservation] = field(default_factory=list)
    cases: list[CaseObservation] = field(default_factory=list)
    metrics: list[MetricObservation] = field(default_factory=list)
    generated_diff: bool = True


@dataclass(frozen=True)
class ScoreBreakdown:
    score: float
    foundational_capability: float
    competitive_capability: float
    accuracy: float
    semantic_vector: float
    research_judge: float | None
    performance: float
    stability: float
    accepted: bool
    reject_reasons: list[str]
    performance_strategy: str = PERFORMANCE_STRATEGY
    degradations: list[dict[str, Any]] = field(default_factory=list)
    improvements: list[dict[str, Any]] = field(default_factory=list)
    metric_budget_failures: list[dict[str, Any]] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {
            "score": round(self.score, 6),
            "foundational_capability": round(self.foundational_capability, 6),
            "competitive_capability": round(self.competitive_capability, 6),
            "accuracy": round(self.accuracy, 6),
            "semantic_vector": round(self.semantic_vector, 6),
            "research_judge": (
                round(self.research_judge, 6)
                if self.research_judge is not None
                else None
            ),
            "performance": round(self.performance, 6),
            "performance_strategy": self.performance_strategy,
            "stability": round(self.stability, 6),
            "accepted": self.accepted,
            "reject_reasons": self.reject_reasons,
            "degradations": self.degradations,
            "improvements": self.improvements,
            "metric_budget_failures": self.metric_budget_failures,
        }


def score_evaluation(
    observation: EvaluationObservation,
    previous_run: dict[str, Any] | None,
    weights: dict[str, float] | None = None,
) -> ScoreBreakdown:
    research_judge_scores = objective_case_scores(observation.cases, "research_judge")
    active_weights = dict(JUDGE_WEIGHTS if research_judge_scores else DEFAULT_WEIGHTS)
    if weights:
        active_weights.update(weights)

    foundational_scores = objective_case_scores(
        observation.cases,
        "foundational_capability",
        aliases=("accuracy",),
    )
    competitive_scores = objective_case_scores(observation.cases, "competitive_capability")
    semantic_vector_scores = objective_case_scores(observation.cases, "semantic_vector")
    foundational_capability = average(foundational_scores, default=0.0)
    competitive_capability = average(competitive_scores, default=0.0)
    accuracy = average(
        [
            score
            for score, scores in (
                (foundational_capability, foundational_scores),
                (competitive_capability, competitive_scores),
            )
            if scores
        ],
        default=0.0,
    )
    semantic_vector = average(semantic_vector_scores, default=0.0)
    research_judge = (
        average(research_judge_scores, default=0.0)
        if research_judge_scores
        else None
    )
    performance = performance_score(observation.metrics, previous_run)
    stability = stability_score(observation.gates, observation.generated_diff)
    budget_failures = metric_budget_failures(observation.metrics)
    score = (
        foundational_capability * active_weights["foundational_capability"]
        + competitive_capability * active_weights["competitive_capability"]
        + semantic_vector * active_weights["semantic_vector"]
        + (research_judge or 0.0) * active_weights.get("research_judge", 0.0)
        + performance * active_weights["performance"]
        + stability * active_weights["stability"]
    )

    reject_reasons = acceptance_reject_reasons(
        observation=observation,
        score=score,
        foundational_capability=foundational_capability,
        competitive_capability=competitive_capability,
        accuracy=accuracy,
        semantic_vector=semantic_vector,
        research_judge=research_judge,
        performance=performance,
        stability=stability,
        previous_run=previous_run,
    )
    degradations = evaluation_degradations(
        observation=observation,
        score=score,
        foundational_capability=foundational_capability,
        competitive_capability=competitive_capability,
        accuracy=accuracy,
        semantic_vector=semantic_vector,
        research_judge=research_judge,
        performance=performance,
        stability=stability,
        previous_run=previous_run,
    )
    improvements = evaluation_improvements(
        observation=observation,
        score=score,
        foundational_capability=foundational_capability,
        competitive_capability=competitive_capability,
        accuracy=accuracy,
        semantic_vector=semantic_vector,
        research_judge=research_judge,
        performance=performance,
        stability=stability,
        previous_run=previous_run,
    )

    return ScoreBreakdown(
        score=score,
        foundational_capability=foundational_capability,
        competitive_capability=competitive_capability,
        accuracy=accuracy,
        semantic_vector=semantic_vector,
        research_judge=research_judge,
        performance=performance,
        stability=stability,
        accepted=not reject_reasons,
        reject_reasons=reject_reasons,
        degradations=degradations,
        improvements=improvements,
        metric_budget_failures=budget_failures,
    )


def acceptance_reject_reasons(
    observation: EvaluationObservation,
    score: float,
    foundational_capability: float,
    competitive_capability: float,
    accuracy: float,
    semantic_vector: float,
    research_judge: float | None,
    performance: float,
    stability: float,
    previous_run: dict[str, Any] | None,
) -> list[str]:
    reasons: list[str] = []
    if not observation.generated_diff:
        reasons.append("codex produced no candidate diff")
    failed_gates = [gate.name for gate in observation.gates if not gate.passed]
    if failed_gates:
        reasons.append("quality gates failed: " + ", ".join(failed_gates))
    if previous_run is None:
        return reasons

    protected_rejections = protected_objective_reject_reasons(
        foundational_capability=foundational_capability,
        competitive_capability=competitive_capability,
        semantic_vector=semantic_vector,
        research_judge=research_judge,
        stability=stability,
        previous_run=previous_run,
    )
    if protected_rejections:
        reasons.extend(protected_rejections)
        return reasons

    improvements = evaluation_improvements(
        observation=observation,
        score=score,
        foundational_capability=foundational_capability,
        competitive_capability=competitive_capability,
        accuracy=accuracy,
        semantic_vector=semantic_vector,
        research_judge=research_judge,
        performance=performance,
        stability=stability,
        previous_run=previous_run,
    )
    if bug_fix_priority_improved(improvements):
        return reasons

    previous_score = float(previous_run.get("score", 0.0))
    score_improved = meaningful_increase(score, previous_score, SCORE_EPSILON, 0.0)
    pareto_improved = epsilon_pareto_improved(
        observation=observation,
        score=score,
        foundational_capability=foundational_capability,
        competitive_capability=competitive_capability,
        accuracy=accuracy,
        semantic_vector=semantic_vector,
        research_judge=research_judge,
        performance=performance,
        stability=stability,
        previous_run=previous_run,
    )
    if not score_improved and not pareto_improved:
        reasons.append(epsilon_pareto_reject_reason(score, previous_score))

    return reasons


def bug_fix_priority_improved(improvements: list[dict[str, Any]]) -> bool:
    for improvement in improvements:
        if improvement.get("kind") == "gate":
            if (
                improvement.get("previous") == "failed"
                and improvement.get("current") == "passed"
            ):
                return True
        if (
            improvement.get("kind") == "case"
            and improvement.get("reason") == "failed_to_passed"
        ):
            return True
    return False


def protected_objective_reject_reasons(
    foundational_capability: float,
    competitive_capability: float,
    semantic_vector: float,
    research_judge: float | None,
    stability: float,
    previous_run: dict[str, Any],
) -> list[str]:
    reasons: list[str] = []
    for name, current in (
        ("foundational_capability", foundational_capability),
        ("competitive_capability", competitive_capability),
        ("semantic_vector", semantic_vector),
        ("stability", stability),
    ):
        previous = previous_run.get(name)
        if previous is not None and meaningful_decrease(
            current,
            float(previous),
            RATIO_EPSILON,
            0.0,
        ):
            reasons.append(
                f"protected {name} objective regressed "
                f"({current:.6f}, previous {float(previous):.6f}, "
                f"ratio_epsilon {RATIO_EPSILON:.6f})"
            )
    if research_judge is not None:
        previous = previous_run.get("research_judge")
        if previous is not None and meaningful_decrease(
            research_judge,
            float(previous),
            RATIO_EPSILON,
            0.0,
        ):
            reasons.append(
                f"protected research_judge objective regressed "
                f"({research_judge:.6f}, previous {float(previous):.6f}, "
                f"ratio_epsilon {RATIO_EPSILON:.6f})"
            )
    return reasons


def epsilon_pareto_improved(
    observation: EvaluationObservation,
    score: float,
    foundational_capability: float,
    competitive_capability: float,
    accuracy: float,
    semantic_vector: float,
    research_judge: float | None,
    performance: float,
    stability: float,
    previous_run: dict[str, Any],
) -> bool:
    degradations = evaluation_degradations(
        observation=observation,
        score=score,
        foundational_capability=foundational_capability,
        competitive_capability=competitive_capability,
        accuracy=accuracy,
        semantic_vector=semantic_vector,
        research_judge=research_judge,
        performance=performance,
        stability=stability,
        previous_run=previous_run,
    )
    improvements = evaluation_improvements(
        observation=observation,
        score=score,
        foundational_capability=foundational_capability,
        competitive_capability=competitive_capability,
        accuracy=accuracy,
        semantic_vector=semantic_vector,
        research_judge=research_judge,
        performance=performance,
        stability=stability,
        previous_run=previous_run,
    )
    return bool(improvements) and not degradations


def epsilon_pareto_reject_reason(score: float, previous_score: float) -> str:
    return (
        f"neither score nor epsilon-pareto objectives improved "
        f"(score {score:.6f}, previous {previous_score:.6f}, "
        f"score_epsilon {SCORE_EPSILON:.6f})"
    )


def evaluation_degradations(
    observation: EvaluationObservation,
    score: float,
    foundational_capability: float,
    competitive_capability: float,
    accuracy: float,
    semantic_vector: float,
    research_judge: float | None,
    performance: float,
    stability: float,
    previous_run: dict[str, Any] | None,
) -> list[dict[str, Any]]:
    if previous_run is None:
        return []

    degradations: list[dict[str, Any]] = []
    for name, current in (
        ("score", score),
        ("foundational_capability", foundational_capability),
        ("competitive_capability", competitive_capability),
        ("accuracy", accuracy),
        ("semantic_vector", semantic_vector),
        ("performance", performance),
        ("stability", stability),
    ):
        previous = previous_run.get(name)
        if previous is not None and meaningful_decrease(
            current,
            float(previous),
            RATIO_EPSILON,
            0.0,
        ):
            degradations.append(numeric_degradation("score_component", name, previous, current))
    if research_judge is not None:
        previous = previous_run.get("research_judge")
        if previous is not None and meaningful_decrease(
            research_judge,
            float(previous),
            RATIO_EPSILON,
            0.0,
        ):
            degradations.append(
                numeric_degradation(
                    "score_component",
                    "research_judge",
                    previous,
                    research_judge,
                )
            )

    previous_metrics = keyed_items(previous_run.get("metrics", []), "name")
    for metric in observation.metrics:
        previous = previous_metrics.get(metric.name)
        if previous is None:
            continue
        previous_value = float(previous.get("value", 0.0))
        worsened = metric_worsened(metric, previous_value)
        if worsened:
            degradation = numeric_degradation(
                "metric",
                metric.name,
                previous_value,
                metric.value,
            )
            degradation["lower_is_better"] = metric.lower_is_better
            degradation["budget"] = metric.budget
            degradations.append(degradation)

    previous_cases = keyed_items(previous_run.get("cases", []), "case_id")
    for case in observation.cases:
        previous = previous_cases.get(case.case_id)
        if previous is None:
            continue
        case_degradation = case_worsened(previous, case)
        if case_degradation:
            degradations.append(case_degradation)

    previous_gates = keyed_items(previous_run.get("gates", []), "name")
    for gate in observation.gates:
        previous = previous_gates.get(gate.name)
        if previous and previous.get("passed") and not gate.passed:
            degradations.append(
                {
                    "kind": "gate",
                    "name": gate.name,
                    "previous": "passed",
                    "current": "failed",
                    "message": gate.message,
                }
            )

    return degradations


def evaluation_improvements(
    observation: EvaluationObservation,
    score: float,
    foundational_capability: float,
    competitive_capability: float,
    accuracy: float,
    semantic_vector: float,
    research_judge: float | None,
    performance: float,
    stability: float,
    previous_run: dict[str, Any] | None,
) -> list[dict[str, Any]]:
    if previous_run is None:
        return []

    improvements: list[dict[str, Any]] = []
    for name, current in (
        ("score", score),
        ("foundational_capability", foundational_capability),
        ("competitive_capability", competitive_capability),
        ("accuracy", accuracy),
        ("semantic_vector", semantic_vector),
        ("performance", performance),
        ("stability", stability),
    ):
        previous = previous_run.get(name)
        if previous is not None and meaningful_increase(
            current,
            float(previous),
            RATIO_EPSILON,
            0.0,
        ):
            improvements.append(numeric_change("score_component", name, previous, current))
    if research_judge is not None:
        previous = previous_run.get("research_judge")
        if previous is not None and meaningful_increase(
            research_judge,
            float(previous),
            RATIO_EPSILON,
            0.0,
        ):
            improvements.append(
                numeric_change(
                    "score_component",
                    "research_judge",
                    previous,
                    research_judge,
                )
            )

    previous_metrics = keyed_items(previous_run.get("metrics", []), "name")
    for metric in observation.metrics:
        previous = previous_metrics.get(metric.name)
        if previous is None:
            continue
        previous_value = float(previous.get("value", 0.0))
        improved = metric_improved(metric, previous_value)
        if improved:
            improvement = numeric_change("metric", metric.name, previous_value, metric.value)
            improvement["lower_is_better"] = metric.lower_is_better
            improvement["budget"] = metric.budget
            improvements.append(improvement)

    previous_cases = keyed_items(previous_run.get("cases", []), "case_id")
    for case in observation.cases:
        previous = previous_cases.get(case.case_id)
        if previous is None:
            continue
        case_improvement = case_improved(previous, case)
        if case_improvement:
            improvements.append(case_improvement)

    previous_gates = keyed_items(previous_run.get("gates", []), "name")
    for gate in observation.gates:
        previous = previous_gates.get(gate.name)
        if previous and not previous.get("passed") and gate.passed:
            improvements.append(
                {
                    "kind": "gate",
                    "name": gate.name,
                    "previous": "failed",
                    "current": "passed",
                    "message": gate.message,
                }
            )

    return improvements


def keyed_items(items: Any, key: str) -> dict[str, dict[str, Any]]:
    if not isinstance(items, list):
        return {}
    return {
        str(item[key]): item
        for item in items
        if isinstance(item, dict) and key in item
    }


def performance_score(
    metrics: list[MetricObservation],
    previous_run: dict[str, Any] | None,
) -> float:
    if not metrics:
        return 1.0
    previous_metrics = {}
    if previous_run and previous_run.get("performance_strategy") == PERFORMANCE_STRATEGY:
        previous_metrics = keyed_items(previous_run.get("metrics", []), "name")
    return average(
        [
            metric_progress_score(metric, previous_metrics.get(metric.name))
            for metric in metrics
        ],
        default=1.0,
    )


def metric_progress_score(
    metric: MetricObservation,
    previous_metric: dict[str, Any] | None,
) -> float:
    budget_score = metric.score()
    if previous_metric is None:
        return budget_score
    previous_value = float(previous_metric.get("value", -1.0))
    if previous_value <= 0.0 or metric.value <= 0.0:
        return budget_score
    if metric.lower_is_better:
        ratio = previous_value / metric.value
    else:
        ratio = metric.value / previous_value
    progress_score = clamp_score(0.5 + math.log(max(ratio, 0.01), 2) * 0.25)
    return clamp_score((budget_score * 0.8) + (progress_score * 0.2))


def numeric_degradation(
    kind: str,
    name: str,
    previous: Any,
    current: float,
) -> dict[str, Any]:
    return numeric_change(kind, name, previous, current)


def metric_improved(metric: MetricObservation, previous_value: float) -> bool:
    threshold = metric_threshold(previous_value)
    if metric.lower_is_better:
        return meaningful_decrease(metric.value, previous_value, threshold, 0.0)
    return meaningful_increase(metric.value, previous_value, threshold, 0.0)


def metric_worsened(metric: MetricObservation, previous_value: float) -> bool:
    threshold = metric_threshold(previous_value)
    if metric.lower_is_better:
        return meaningful_increase(metric.value, previous_value, threshold, 0.0)
    return meaningful_decrease(metric.value, previous_value, threshold, 0.0)


def metric_threshold(previous_value: float) -> float:
    return max(METRIC_ABSOLUTE_EPSILON, abs(previous_value) * METRIC_RELATIVE_EPSILON)


def meaningful_increase(
    current: float,
    previous: float,
    absolute_epsilon: float,
    relative_epsilon: float,
) -> bool:
    return current - previous > epsilon_threshold(previous, absolute_epsilon, relative_epsilon)


def meaningful_decrease(
    current: float,
    previous: float,
    absolute_epsilon: float,
    relative_epsilon: float,
) -> bool:
    return previous - current > epsilon_threshold(previous, absolute_epsilon, relative_epsilon)


def epsilon_threshold(
    previous: float,
    absolute_epsilon: float,
    relative_epsilon: float,
) -> float:
    return max(absolute_epsilon, abs(previous) * relative_epsilon)


def numeric_change(
    kind: str,
    name: str,
    previous: Any,
    current: float,
) -> dict[str, Any]:
    previous_value = float(previous)
    return {
        "kind": kind,
        "name": name,
        "previous": round(previous_value, 6),
        "current": round(current, 6),
        "delta": round(current - previous_value, 6),
    }


def case_worsened(previous: dict[str, Any], current: CaseObservation) -> dict[str, Any] | None:
    previous_passed = bool(previous.get("passed"))
    previous_score = previous_case_score(previous)
    current_score = current.score()
    score_worsened = meaningful_decrease(
        current_score,
        previous_score,
        RATIO_EPSILON,
        0.0,
    )
    rank_worsened = rank_value(current.rank, current.max_rank) > rank_value(
        previous.get("rank"),
        int(previous.get("max_rank", current.max_rank)),
    )
    false_positives_worsened = current.false_positive_count > int(
        previous.get("false_positive_count", 0)
    )
    if previous_passed and not current.passed:
        reason = "passed_to_failed"
    elif rank_worsened:
        reason = "rank_worsened"
    elif false_positives_worsened:
        reason = "false_positives_increased"
    elif score_worsened:
        reason = "score_worsened"
    else:
        return None
    change = {
        "kind": "case",
        "objective": current.objective,
        "case_id": current.case_id,
        "repository": current.repository,
        "reason": reason,
        "previous": {
            "passed": previous_passed,
            "rank": previous.get("rank"),
            "false_positive_count": previous.get("false_positive_count", 0),
            "score": round(previous_score, 6),
        },
        "current": {
            "passed": current.passed,
            "rank": current.rank,
            "false_positive_count": current.false_positive_count,
            "score": round(current_score, 6),
        },
        "message": current.message,
    }
    return change


def case_improved(previous: dict[str, Any], current: CaseObservation) -> dict[str, Any] | None:
    previous_passed = bool(previous.get("passed"))
    previous_score = previous_case_score(previous)
    current_score = current.score()
    score_improved = meaningful_increase(
        current_score,
        previous_score,
        RATIO_EPSILON,
        0.0,
    )
    rank_improved = rank_value(current.rank, current.max_rank) < rank_value(
        previous.get("rank"),
        int(previous.get("max_rank", current.max_rank)),
    )
    false_positives_improved = current.false_positive_count < int(
        previous.get("false_positive_count", 0)
    )
    if not previous_passed and current.passed:
        reason = "failed_to_passed"
    elif rank_improved:
        reason = "rank_improved"
    elif false_positives_improved:
        reason = "false_positives_decreased"
    elif score_improved:
        reason = "score_improved"
    else:
        return None
    change = {
        "kind": "case",
        "objective": current.objective,
        "case_id": current.case_id,
        "repository": current.repository,
        "reason": reason,
        "previous": {
            "passed": previous_passed,
            "rank": previous.get("rank"),
            "false_positive_count": previous.get("false_positive_count", 0),
            "score": round(previous_score, 6),
        },
        "current": {
            "passed": current.passed,
            "rank": current.rank,
            "false_positive_count": current.false_positive_count,
            "score": round(current_score, 6),
        },
        "message": current.message,
    }
    return change


def previous_case_score(previous: dict[str, Any]) -> float:
    if "score_override" in previous and previous["score_override"] is not None:
        return clamp_score(float(previous["score_override"]))
    if not bool(previous.get("passed")):
        return 0.0
    rank = previous.get("rank")
    if rank is None or int(rank) <= 0:
        rank_score = 1.0
    else:
        rank_score = 1.0 / int(rank)
    false_positive_penalty = min(0.5, int(previous.get("false_positive_count", 0)) * 0.1)
    return max(0.0, rank_score - false_positive_penalty)


def rank_value(rank: Any, max_rank: int) -> int:
    if rank is None:
        return max_rank + 10_000
    return int(rank)


def stability_score(gates: list[GateObservation], generated_diff: bool) -> float:
    if not generated_diff:
        return 0.0
    if not gates:
        return 1.0
    return sum(1.0 for gate in gates if gate.passed) / len(gates)


def objective_case_scores(
    cases: list[CaseObservation],
    objective: str,
    aliases: tuple[str, ...] = (),
) -> list[float]:
    objective_names = {objective, *aliases}
    return [case.score() for case in cases if case.objective in objective_names]


def metric_budget_failures(metrics: list[MetricObservation]) -> list[dict[str, Any]]:
    failures: list[dict[str, Any]] = []
    for metric in metrics:
        if metric.budget is None or metric.budget <= 0:
            continue
        misses_budget = (
            metric.value > metric.budget
            if metric.lower_is_better
            else metric.value < metric.budget
        )
        if misses_budget:
            failures.append(
                {
                    "name": metric.name,
                    "value": round(metric.value, 6),
                    "budget": round(metric.budget, 6),
                    "lower_is_better": metric.lower_is_better,
                    "score": round(metric.score(), 6),
                }
            )
    return failures


def average(values: list[float], default: float) -> float:
    if not values:
        return default
    return sum(values) / len(values)


def clamp_score(value: float) -> float:
    return min(1.0, max(0.0, float(value)))
