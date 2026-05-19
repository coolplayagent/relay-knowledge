"""Continuous ranked-hit scoring helpers for self-iteration query cases."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class RankedHitAssessment:
    rank: int | None
    false_positive_count: int
    score: float
    details: str
    failures: list[str]


def assess_ranked_hits(
    case: dict[str, Any],
    hits: list[dict[str, Any]],
    expected: list[dict[str, Any]],
    forbidden: list[dict[str, Any]],
) -> RankedHitAssessment:
    max_rank = int(case.get("max_rank", 1))
    rank = first_expected_rank(hits, expected)
    false_positive_ranks = [
        index
        for index, hit in enumerate(hits, start=1)
        if hit_matches_any(hit, forbidden)
    ]
    expected_all = case.get("expected_all", [])
    expected_sequence = case.get("expected_sequence", [])
    all_ranks = first_match_ranks(hits, expected_all)
    sequence_ranks = first_match_ranks(hits, expected_sequence)
    all_score = coverage_score(all_ranks) if expected_all else None
    sequence_score = sequence_quality_score(sequence_ranks) if expected_sequence else None
    forbidden_penalty = ranked_forbidden_penalty(
        false_positive_ranks,
        float(case.get("forbidden_rank_penalty", 0.1)),
    )
    score = ranked_case_score(
        rank=rank,
        has_primary_expected=bool(expected),
        all_score=all_score,
        sequence_score=sequence_score,
        forbidden_penalty=forbidden_penalty,
    )
    failures = ranked_case_failures(
        case,
        expected,
        rank,
        max_rank,
        false_positive_ranks,
        all_ranks,
        sequence_ranks,
        sequence_score,
        score,
    )
    return RankedHitAssessment(
        rank=rank,
        false_positive_count=len(false_positive_ranks),
        score=score,
        details=ranked_case_details(
            score,
            all_ranks,
            sequence_ranks,
            forbidden_penalty,
            failures,
        ),
        failures=failures,
    )


def ranked_case_failures(
    case: dict[str, Any],
    expected: list[dict[str, Any]],
    rank: int | None,
    max_rank: int,
    false_positive_ranks: list[int],
    all_ranks: list[int | None],
    sequence_ranks: list[int | None],
    sequence_score: float | None,
    score: float,
) -> list[str]:
    failures: list[str] = []
    if expected and (rank is None or rank > max_rank):
        failures.append(f"rank={rank} max_rank={max_rank}")
    if false_positive_ranks and not bool(case.get("forbidden_rank_penalty_only")):
        failures.append(f"false_positives={len(false_positive_ranks)}")
    if (
        all_ranks
        and bool(case.get("require_expected_all", True))
        and any(matched_rank is None for matched_rank in all_ranks)
    ):
        failures.append(f"expected_all={matched_count(all_ranks)}/{len(all_ranks)}")
    if (
        sequence_ranks
        and bool(case.get("require_expected_sequence", True))
        and sequence_score is not None
        and sequence_score < 1.0
    ):
        failures.append(
            f"expected_sequence={matched_count(sequence_ranks)}/{len(sequence_ranks)}"
        )
    if "min_score" in case and score < float(case["min_score"]):
        failures.append(f"score={score:.3f} min_score={float(case['min_score']):.3f}")
    return failures


def first_expected_rank(hits: list[dict[str, Any]], expected: list[dict[str, Any]]) -> int | None:
    for index, hit in enumerate(hits, start=1):
        if hit_matches_any(hit, expected):
            return index
    return None


def first_match_ranks(
    hits: list[dict[str, Any]],
    patterns: list[dict[str, Any]],
) -> list[int | None]:
    ranks: list[int | None] = []
    for pattern in patterns:
        rank = None
        for index, hit in enumerate(hits, start=1):
            if hit_matches(hit, pattern):
                rank = index
                break
        ranks.append(rank)
    return ranks


def coverage_score(ranks: list[int | None]) -> float:
    if not ranks:
        return 1.0
    return matched_count(ranks) / len(ranks)


def sequence_quality_score(ranks: list[int | None]) -> float:
    if not ranks:
        return 1.0
    matched_ranks = [rank for rank in ranks if rank is not None]
    coverage = len(matched_ranks) / len(ranks)
    if len(matched_ranks) <= 1:
        return coverage
    ordered_pairs = sum(
        1
        for left, right in zip(matched_ranks, matched_ranks[1:])
        if left <= right
    )
    return coverage * (ordered_pairs / (len(matched_ranks) - 1))


def matched_count(ranks: list[int | None]) -> int:
    return sum(1 for rank in ranks if rank is not None)


def ranked_forbidden_penalty(ranks: list[int], penalty_weight: float) -> float:
    return min(0.5, sum(penalty_weight / max(rank, 1) for rank in ranks))


def ranked_case_score(
    rank: int | None,
    has_primary_expected: bool,
    all_score: float | None,
    sequence_score: float | None,
    forbidden_penalty: float,
) -> float:
    if has_primary_expected:
        rank_score = 0.0 if rank is None else 1.0 / max(rank, 1)
    else:
        rank_score = 1.0
    components = [rank_score]
    if all_score is not None:
        components.append(all_score)
    if sequence_score is not None:
        components.append(sequence_score)
    return max(0.0, min(1.0, (sum(components) / len(components)) - forbidden_penalty))


def ranked_case_details(
    score: float,
    all_ranks: list[int | None],
    sequence_ranks: list[int | None],
    forbidden_penalty: float,
    failures: list[str],
) -> str:
    details = [f"score={score:.3f}"]
    if all_ranks:
        details.append(f"expected_all={matched_count(all_ranks)}/{len(all_ranks)}")
    if sequence_ranks:
        details.append(
            f"expected_sequence={matched_count(sequence_ranks)}/{len(sequence_ranks)}"
        )
    if forbidden_penalty:
        details.append(f"forbidden_penalty={forbidden_penalty:.3f}")
    if failures:
        details.append("failures=" + "; ".join(failures))
    return " ".join(details)


def hit_matches_any(hit: dict[str, Any], patterns: list[dict[str, Any]]) -> bool:
    return any(hit_matches(hit, pattern) for pattern in patterns)


def hit_matches(hit: dict[str, Any], pattern: dict[str, Any]) -> bool:
    if "repository_alias" in pattern and hit.get("repository_alias") != pattern["repository_alias"]:
        return False
    if "repository_id" in pattern and hit.get("repository_id") != pattern["repository_id"]:
        return False
    if "source_scope" in pattern and hit.get("source_scope") != pattern["source_scope"]:
        return False
    if "path" in pattern and hit.get("path") != pattern["path"]:
        return False
    if "relative_path" in pattern and hit.get("relative_path") != pattern["relative_path"]:
        return False
    if "file_name" in pattern and hit.get("file_name") != pattern["file_name"]:
        return False
    if "extension" in pattern and hit.get("extension") != pattern["extension"]:
        return False
    if "status" in pattern and hit.get("status") != pattern["status"]:
        return False
    if "line_start" in pattern:
        line_range = hit.get("line_range", {})
        start = int(line_range.get("start", -1))
        end = int(line_range.get("end", -1))
        expected = int(pattern["line_start"])
        if not (start <= expected <= end or start == expected):
            return False
    if "edge_resolution_state" in pattern:
        if hit.get("edge_resolution_state") != pattern["edge_resolution_state"]:
            return False
    if "edge_target_hint" in pattern:
        target = hit.get("edge_target_hint") or ""
        if pattern["edge_target_hint"] not in target:
            return False
    if "excerpt_contains" in pattern:
        if pattern["excerpt_contains"] not in hit.get("excerpt", ""):
            return False
    if "content_contains" in pattern:
        if pattern["content_contains"] not in hit.get("content", ""):
            return False
    if "retriever_source" in pattern:
        if pattern["retriever_source"] not in hit.get("retriever_sources", []):
            return False
    return True
