"""Deterministic scoring policy for self-iteration runs."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


DEFAULT_WEIGHTS = {
    "accuracy": 0.55,
    "performance": 0.30,
    "stability": 0.15,
}


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

    def to_dict(self) -> dict[str, Any]:
        return {
            "score": round(self.score, 6),
            "accuracy": round(self.accuracy, 6),
            "performance": round(self.performance, 6),
            "stability": round(self.stability, 6),
            "accepted": self.accepted,
            "reject_reasons": self.reject_reasons,
        }


def score_evaluation(
    observation: EvaluationObservation,
    best_previous: dict[str, Any] | None,
    weights: dict[str, float] | None = None,
    min_delta: float = 0.001,
    max_key_regression_ratio: float = 1.05,
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
        best_previous=best_previous,
        min_delta=min_delta,
        max_key_regression_ratio=max_key_regression_ratio,
    )

    return ScoreBreakdown(
        score=score,
        accuracy=accuracy,
        performance=performance,
        stability=stability,
        accepted=not reject_reasons,
        reject_reasons=reject_reasons,
    )


def acceptance_reject_reasons(
    observation: EvaluationObservation,
    score: float,
    accuracy: float,
    best_previous: dict[str, Any] | None,
    min_delta: float,
    max_key_regression_ratio: float,
) -> list[str]:
    reasons: list[str] = []
    if not observation.generated_diff:
        reasons.append("codex produced no candidate diff")
    failed_gates = [gate.name for gate in observation.gates if not gate.passed]
    if failed_gates:
        reasons.append("quality gates failed: " + ", ".join(failed_gates))
    failed_cases = [case.case_id for case in observation.cases if not case.passed]
    if failed_cases:
        reasons.append("accuracy cases failed: " + ", ".join(failed_cases[:8]))

    if best_previous is None:
        return reasons

    best_score = float(best_previous.get("score", 0.0))
    best_accuracy = float(best_previous.get("accuracy", 0.0))
    if score <= best_score + min_delta:
        reasons.append(
            f"score {score:.6f} did not strictly improve best {best_score:.6f}"
        )
    if accuracy + 1e-9 < best_accuracy:
        reasons.append(
            f"accuracy {accuracy:.6f} regressed below best {best_accuracy:.6f}"
        )

    best_metrics = {
        metric["name"]: metric
        for metric in best_previous.get("metrics", [])
        if isinstance(metric, dict)
    }
    for metric in observation.metrics:
        if not metric.key:
            continue
        previous = best_metrics.get(metric.name)
        if not previous:
            continue
        previous_value = float(previous.get("value", 0.0))
        if previous_value <= 0:
            continue
        if metric.lower_is_better and metric.value > previous_value * max_key_regression_ratio:
            reasons.append(
                f"{metric.name} regressed from {previous_value:.3f} to {metric.value:.3f}"
            )
        if not metric.lower_is_better and metric.value * max_key_regression_ratio < previous_value:
            reasons.append(
                f"{metric.name} regressed from {previous_value:.3f} to {metric.value:.3f}"
            )

    return reasons


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
