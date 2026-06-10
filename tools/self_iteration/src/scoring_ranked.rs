#[derive(Debug, Clone)]
pub struct RankedAssessment {
    pub rank: Option<usize>,
    pub false_positive_count: usize,
    pub score: f64,
    pub details: String,
    pub failures: Vec<String>,
}

pub fn assess_ranked_hits(
    case: &Value,
    hits: &[Value],
    expected: &[Value],
    forbidden: &[Value],
) -> RankedAssessment {
    let max_rank = usize_field(case, "max_rank", 1);
    let rank = first_expected_rank(hits, expected);
    let false_positive_ranks = hits
        .iter()
        .enumerate()
        .filter_map(|(index, hit)| hit_matches_any(hit, forbidden).then_some(index + 1))
        .collect::<Vec<_>>();
    let expected_all = array_field(case, "expected_all");
    let expected_sequence = array_field(case, "expected_sequence");
    let all_ranks = first_match_ranks(hits, expected_all);
    let sequence_ranks = first_match_ranks(hits, expected_sequence);
    let all_score = (!expected_all.is_empty()).then(|| coverage_score(&all_ranks));
    let sequence_score =
        (!expected_sequence.is_empty()).then(|| sequence_quality_score(&sequence_ranks));
    let forbidden_penalty = ranked_forbidden_penalty(
        &false_positive_ranks,
        case.get("forbidden_rank_penalty")
            .and_then(Value::as_f64)
            .unwrap_or(0.1),
    );
    let score = ranked_case_score(
        rank,
        !expected.is_empty(),
        all_score,
        sequence_score,
        forbidden_penalty,
    );
    let failures = ranked_failures(RankedFailureInput {
        case,
        expected,
        rank,
        max_rank,
        false_positive_ranks: &false_positive_ranks,
        all_ranks: &all_ranks,
        sequence_ranks: &sequence_ranks,
        sequence_score,
        score,
    });
    RankedAssessment {
        rank,
        false_positive_count: false_positive_ranks.len(),
        score,
        details: ranked_details(
            score,
            &all_ranks,
            &sequence_ranks,
            forbidden_penalty,
            &failures,
        ),
        failures,
    }
}

pub fn hit_matches_any(hit: &Value, patterns: &[Value]) -> bool {
    patterns.iter().any(|pattern| hit_matches(hit, pattern))
}

fn hit_matches(hit: &Value, pattern: &Value) -> bool {
    for key in [
        "path",
        "relative_path",
        "file_name",
        "extension",
        "status",
        "software_slice",
        "name",
        "ecosystem",
        "language_id",
        "kind",
        "command",
        "output_hint",
        "source_kind",
        "relationship_state",
        "resolution_state",
        "target_hint",
        "file_role",
        "parse_status",
        "topic_kind",
        "source_path",
        "relationship_kind",
        "target_kind",
        "provider",
        "resource_kind",
        "element_kind",
        "parent",
        "scope_hint",
        "evidence_path",
        "package_name",
        "module",
        "dependency_group",
        "requirement",
        "resolved_version",
        "edge_resolution_state",
        "repository_alias",
        "source_scope",
    ] {
        if let Some(expected) = pattern.get(key).and_then(Value::as_str) {
            if hit.get(key).and_then(Value::as_str) != Some(expected) {
                return false;
            }
        }
    }
    if let Some(expected) = pattern.get("line_start").and_then(Value::as_i64) {
        if !line_matches_any(hit, expected, &["line_range", "evidence_line_range"]) {
            return false;
        }
    }
    if let Some(expected) = pattern.get("evidence_line_start").and_then(Value::as_i64) {
        if !line_matches_any(hit, expected, &["evidence_line_range"]) {
            return false;
        }
    }
    if let Some(expected) = pattern.get("edge_target_hint").and_then(Value::as_str) {
        if !hit
            .get("edge_target_hint")
            .and_then(Value::as_str)
            .unwrap_or("")
            .contains(expected)
        {
            return false;
        }
    }
    for (key, hit_key) in [
        ("excerpt_contains", "excerpt"),
        ("content_contains", "content"),
        ("summary_contains", "summary"),
        ("command_contains", "command"),
        ("target_hint_contains", "target_hint"),
        ("module_contains", "module"),
    ] {
        if let Some(expected) = pattern.get(key).and_then(Value::as_str) {
            if !hit
                .get(hit_key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .contains(expected)
            {
                return false;
            }
        }
    }
    if let Some(expected) = pattern.get("retriever_source").and_then(Value::as_str) {
        let sources = hit
            .get("retriever_sources")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if !sources
            .iter()
            .any(|source| source.as_str() == Some(expected))
        {
            return false;
        }
    }
    if let Some(expected) = pattern.get("retrieval_layer").and_then(Value::as_str) {
        let layers = hit
            .get("retrieval_layers")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if !layers.iter().any(|layer| layer.as_str() == Some(expected)) {
            return false;
        }
    }
    if pattern
        .get("edge_confidence_absent")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && (hit.get("edge_confidence_basis_points").is_some()
            || hit.get("edge_confidence_tier").is_some())
    {
        return false;
    }
    true
}

fn line_matches_any(hit: &Value, expected: i64, fields: &[&str]) -> bool {
    fields.iter().any(|field| {
        let start = hit
            .get(*field)
            .and_then(|range| range.get("start"))
            .and_then(Value::as_i64)
            .unwrap_or(-1);
        let end = hit
            .get(*field)
            .and_then(|range| range.get("end"))
            .and_then(Value::as_i64)
            .unwrap_or(-1);
        start <= expected && expected <= end || start == expected
    })
}

fn first_expected_rank(hits: &[Value], expected: &[Value]) -> Option<usize> {
    hits.iter()
        .enumerate()
        .find_map(|(index, hit)| hit_matches_any(hit, expected).then_some(index + 1))
}

fn first_match_ranks(hits: &[Value], patterns: &[Value]) -> Vec<Option<usize>> {
    patterns
        .iter()
        .map(|pattern| {
            hits.iter()
                .enumerate()
                .find_map(|(index, hit)| hit_matches(hit, pattern).then_some(index + 1))
        })
        .collect()
}

struct RankedFailureInput<'a> {
    case: &'a Value,
    expected: &'a [Value],
    rank: Option<usize>,
    max_rank: usize,
    false_positive_ranks: &'a [usize],
    all_ranks: &'a [Option<usize>],
    sequence_ranks: &'a [Option<usize>],
    sequence_score: Option<f64>,
    score: f64,
}

fn ranked_failures(input: RankedFailureInput<'_>) -> Vec<String> {
    let mut failures = Vec::new();
    if !input.expected.is_empty() && input.rank.is_none_or(|value| value > input.max_rank) {
        failures.push(format!("rank={:?} max_rank={}", input.rank, input.max_rank));
    }
    if !input.false_positive_ranks.is_empty()
        && !input
            .case
            .get("forbidden_rank_penalty_only")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        failures.push(format!(
            "false_positives={}",
            input.false_positive_ranks.len()
        ));
    }
    if !input.all_ranks.is_empty()
        && input
            .case
            .get("require_expected_all")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        && input.all_ranks.iter().any(Option::is_none)
    {
        failures.push(format!(
            "expected_all={}/{}",
            matched_count(input.all_ranks),
            input.all_ranks.len()
        ));
    }
    if !input.sequence_ranks.is_empty()
        && input
            .case
            .get("require_expected_sequence")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        && input.sequence_score.is_some_and(|value| value < 1.0)
    {
        failures.push(format!(
            "expected_sequence={}/{}",
            matched_count(input.sequence_ranks),
            input.sequence_ranks.len()
        ));
    }
    if let Some(min_score) = input.case.get("min_score").and_then(Value::as_f64) {
        if input.score < min_score {
            failures.push(format!("score={:.3} min_score={min_score:.3}", input.score));
        }
    }
    failures
}

fn ranked_details(
    score: f64,
    all_ranks: &[Option<usize>],
    sequence_ranks: &[Option<usize>],
    forbidden_penalty: f64,
    failures: &[String],
) -> String {
    let mut details = vec![format!("score={score:.3}")];
    if !all_ranks.is_empty() {
        details.push(format!(
            "expected_all={}/{}",
            matched_count(all_ranks),
            all_ranks.len()
        ));
    }
    if !sequence_ranks.is_empty() {
        details.push(format!(
            "expected_sequence={}/{}",
            matched_count(sequence_ranks),
            sequence_ranks.len()
        ));
    }
    if forbidden_penalty > 0.0 {
        details.push(format!("forbidden_penalty={forbidden_penalty:.3}"));
    }
    if !failures.is_empty() {
        details.push(format!("failures={}", failures.join("; ")));
    }
    details.join(" ")
}

fn ranked_case_score(
    rank: Option<usize>,
    has_primary_expected: bool,
    all_score: Option<f64>,
    sequence_score: Option<f64>,
    forbidden_penalty: f64,
) -> f64 {
    let rank_score = if has_primary_expected {
        rank.map(|value| 1.0 / value.max(1) as f64).unwrap_or(0.0)
    } else {
        1.0
    };
    let mut components = vec![rank_score];
    if let Some(score) = all_score {
        components.push(score);
    }
    if let Some(score) = sequence_score {
        components.push(score);
    }
    clamp(average(&components, 1.0) - forbidden_penalty)
}

fn coverage_score(ranks: &[Option<usize>]) -> f64 {
    if ranks.is_empty() {
        return 1.0;
    }
    matched_count(ranks) as f64 / ranks.len() as f64
}

fn sequence_quality_score(ranks: &[Option<usize>]) -> f64 {
    if ranks.is_empty() {
        return 1.0;
    }
    let matched = ranks.iter().flatten().copied().collect::<Vec<_>>();
    let coverage = matched.len() as f64 / ranks.len() as f64;
    if matched.len() <= 1 {
        return coverage;
    }
    let ordered_pairs = matched.windows(2).filter(|pair| pair[0] <= pair[1]).count();
    coverage * ordered_pairs as f64 / (matched.len() - 1) as f64
}

fn ranked_forbidden_penalty(ranks: &[usize], weight: f64) -> f64 {
    ranks
        .iter()
        .map(|rank| weight / (*rank).max(1) as f64)
        .sum::<f64>()
        .min(0.5)
}

fn matched_count(ranks: &[Option<usize>]) -> usize {
    ranks.iter().filter(|rank| rank.is_some()).count()
}
