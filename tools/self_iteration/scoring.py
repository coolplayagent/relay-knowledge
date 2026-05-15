"""Deterministic scoring policy for self-iteration runs."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


DEFAULT_WEIGHTS = {
    "accuracy": 0.55,
    "performance": 0.30,
    "stability": 0.15,
}

SCORE_EPSILON = 0.0005
RATIO_EPSILON = 0.005
METRIC_RELATIVE_EPSILON = 0.03
METRIC_ABSOLUTE_EPSILON = 25.0


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

    def score(self) -> float:
        if not self.passed:
            return 0.0
        rank = self.rank or self.max_rank
        rank_score = 1.0 if rank <= self.max_rank else max(0.0, self.max_rank / rank)
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
    accuracy: float
    performance: float
    stability: float
    accepted: bool
    reject_reasons: list[str]
    degradations: list[dict[str, Any]] = field(default_factory=list)
    improvements: list[dict[str, Any]] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {
            "score": round(self.score, 6),
            "accuracy": round(self.accuracy, 6),
            "performance": round(self.performance, 6),
            "stability": round(self.stability, 6),
            "accepted": self.accepted,
            "reject_reasons": self.reject_reasons,
            "degradations": self.degradations,
            "improvements": self.improvements,
        }


def score_evaluation(
    observation: EvaluationObservation,
    previous_run: dict[str, Any] | None,
    weights: dict[str, float] | None = None,
) -> ScoreBreakdown:
    active_weights = dict(DEFAULT_WEIGHTS)
    if weights:
        active_weights.update(weights)

    accuracy = average([case.score() for case in observation.cases], default=0.0)
    performance = average([metric.score() for metric in observation.metrics], default=1.0)
    stability = stability_score(observation.gates, observation.generated_diff)
    score = (
        accuracy * active_weights["accuracy"]
        + performance * active_weights["performance"]
        + stability * active_weights["stability"]
    )

    reject_reasons = acceptance_reject_reasons(
        observation=observation,
        score=score,
        accuracy=accuracy,
        performance=performance,
        stability=stability,
        previous_run=previous_run,
    )
    degradations = evaluation_degradations(
        observation=observation,
        score=score,
        accuracy=accuracy,
        performance=performance,
        stability=stability,
        previous_run=previous_run,
    )
    improvements = evaluation_improvements(
        observation=observation,
        score=score,
        accuracy=accuracy,
        performance=performance,
        stability=stability,
        previous_run=previous_run,
    )

    return ScoreBreakdown(
        score=score,
        accuracy=accuracy,
        performance=performance,
        stability=stability,
        accepted=not reject_reasons,
        reject_reasons=reject_reasons,
        degradations=degradations,
        improvements=improvements,
    )


def acceptance_reject_reasons(
    observation: EvaluationObservation,
    score: float,
    accuracy: float,
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

    previous_score = float(previous_run.get("score", 0.0))
    score_improved = meaningful_increase(score, previous_score, SCORE_EPSILON, 0.0)
    pareto_improved = epsilon_pareto_improved(
        observation=observation,
        score=score,
        accuracy=accuracy,
        performance=performance,
        stability=stability,
        previous_run=previous_run,
    )
    if not score_improved and not pareto_improved:
        reasons.append(epsilon_pareto_reject_reason(score, previous_score))

    return reasons


def epsilon_pareto_improved(
    observation: EvaluationObservation,
    score: float,
    accuracy: float,
    performance: float,
    stability: float,
    previous_run: dict[str, Any],
) -> bool:
    degradations = evaluation_degradations(
        observation=observation,
        score=score,
        accuracy=accuracy,
        performance=performance,
        stability=stability,
        previous_run=previous_run,
    )
    improvements = evaluation_improvements(
        observation=observation,
        score=score,
        accuracy=accuracy,
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
    accuracy: float,
    performance: float,
    stability: float,
    previous_run: dict[str, Any] | None,
) -> list[dict[str, Any]]:
    if previous_run is None:
        return []

    degradations: list[dict[str, Any]] = []
    for name, current in (
        ("score", score),
        ("accuracy", accuracy),
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
    accuracy: float,
    performance: float,
    stability: float,
    previous_run: dict[str, Any] | None,
) -> list[dict[str, Any]]:
    if previous_run is None:
        return []

    improvements: list[dict[str, Any]] = []
    for name, current in (
        ("score", score),
        ("accuracy", accuracy),
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
    else:
        return None
    return {
        "kind": "case",
        "case_id": current.case_id,
        "repository": current.repository,
        "reason": reason,
        "previous": {
            "passed": previous_passed,
            "rank": previous.get("rank"),
            "false_positive_count": previous.get("false_positive_count", 0),
        },
        "current": {
            "passed": current.passed,
            "rank": current.rank,
            "false_positive_count": current.false_positive_count,
        },
        "message": current.message,
    }


def case_improved(previous: dict[str, Any], current: CaseObservation) -> dict[str, Any] | None:
    previous_passed = bool(previous.get("passed"))
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
    else:
        return None
    return {
        "kind": "case",
        "case_id": current.case_id,
        "repository": current.repository,
        "reason": reason,
        "previous": {
            "passed": previous_passed,
            "rank": previous.get("rank"),
            "false_positive_count": previous.get("false_positive_count", 0),
        },
        "current": {
            "passed": current.passed,
            "rank": current.rank,
            "false_positive_count": current.false_positive_count,
        },
        "message": current.message,
    }


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


def average(values: list[float], default: float) -> float:
    if not values:
        return default
    return sum(values) / len(values)
