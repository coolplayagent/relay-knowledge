from __future__ import annotations

import math
import random
from collections.abc import Sequence

from relay_knowledge_skill_eval.models import Condition, EvalResult


def percentile(values: Sequence[float], fraction: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    weight = position - lower
    return ordered[lower] * (1 - weight) + ordered[upper] * weight


def distribution(values: Sequence[float]) -> dict[str, float]:
    return {
        "total": sum(values),
        "mean": sum(values) / len(values) if values else 0.0,
        "p50": percentile(values, 0.50),
        "p95": percentile(values, 0.95),
    }


def paired_results(
    results: Sequence[EvalResult],
) -> list[tuple[EvalResult, EvalResult]]:
    grouped: dict[str, dict[Condition, EvalResult]] = {}
    for result in results:
        grouped.setdefault(result.instance_id, {})[result.condition] = result
    return [
        (conditions[Condition.BASELINE], conditions[Condition.SKILL])
        for conditions in grouped.values()
        if Condition.BASELINE in conditions and Condition.SKILL in conditions
    ]


def mcnemar_exact(skill_only: int, baseline_only: int) -> float:
    discordant = skill_only + baseline_only
    if discordant == 0:
        return 1.0
    tail = sum(
        math.comb(discordant, index)
        for index in range(min(skill_only, baseline_only) + 1)
    )
    return min(1.0, 2.0 * tail / (2**discordant))


def paired_pass_delta_ci(
    pairs: Sequence[tuple[EvalResult, EvalResult]],
    *,
    samples: int = 10_000,
    seed: int = 0,
) -> tuple[float, float]:
    if not pairs:
        return (0.0, 0.0)
    deltas = [
        float(skill.benchmark_resolved) - float(baseline.benchmark_resolved)
        for baseline, skill in pairs
    ]
    generator = random.Random(seed)
    means = [
        sum(generator.choice(deltas) for _ in deltas) / len(deltas)
        for _ in range(samples)
    ]
    return (percentile(means, 0.025), percentile(means, 0.975))


def paired_pass_delta_normal_ci(
    pairs: Sequence[tuple[EvalResult, EvalResult]],
) -> tuple[float, float]:
    """Return a cheap live-report CI for the mean paired pass delta."""
    if not pairs:
        return (0.0, 0.0)
    deltas = [
        float(skill.benchmark_resolved) - float(baseline.benchmark_resolved)
        for baseline, skill in pairs
    ]
    mean = sum(deltas) / len(deltas)
    if len(deltas) == 1:
        return (mean, mean)
    variance = sum((value - mean) ** 2 for value in deltas) / (len(deltas) - 1)
    margin = 1.96 * math.sqrt(variance / len(deltas))
    return (max(-1.0, mean - margin), min(1.0, mean + margin))
