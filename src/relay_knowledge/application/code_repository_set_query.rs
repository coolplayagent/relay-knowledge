use std::{
    cell::OnceCell,
    collections::{BTreeMap, BTreeSet},
};

use crate::domain::{
    CodeRepositoryCrossEdge, CodeRepositorySetMemberStatus, CodeRepositorySetQueryHit,
    CodeRetrievalHit,
};

pub(super) struct OverlayEvidenceIndex<'a> {
    edges: &'a [CodeRepositoryCrossEdge],
    import_origins: OnceCell<ImportOriginIndexes>,
    target_files: BTreeMap<(String, String), Vec<usize>>,
    target_symbols: BTreeMap<(String, String), Vec<usize>>,
}

type ImportOriginLineIndex = BTreeMap<(String, String, u32, u32), Vec<usize>>;
type ImportOriginFileIndex = BTreeMap<(String, String), Vec<usize>>;

struct ImportOriginIndexes {
    lines: ImportOriginLineIndex,
    files: ImportOriginFileIndex,
}

const MAX_FILE_ORIGIN_EVIDENCE: usize = 2;
const EVIDENCE_BACKED_PRIORITY_SCORE_STEP: f64 = 0.12;
const MAX_ABSOLUTE_MEMBER_PRIORITY_SCORE: i32 = 10;
const RESOLVED_BRIDGE_SUPPORT_BONUS: f64 = 0.35;
const MAX_BRIDGE_SUPPORT_BONUS: f64 = 0.70;

impl<'a> OverlayEvidenceIndex<'a> {
    pub(super) fn new(edges: &'a [CodeRepositoryCrossEdge]) -> Self {
        let mut index = Self {
            edges,
            import_origins: OnceCell::new(),
            target_files: BTreeMap::new(),
            target_symbols: BTreeMap::new(),
        };
        for (position, edge) in edges.iter().enumerate() {
            let Some(target_scope) = edge.to_source_scope.clone() else {
                continue;
            };
            let Some(target_record_id) = edge.to_record_id.clone() else {
                continue;
            };
            match edge.to_record_kind.as_str() {
                "code_file" => index
                    .target_files
                    .entry((target_scope, target_record_id))
                    .or_default()
                    .push(position),
                "code_symbol_snapshot" => index
                    .target_symbols
                    .entry((target_scope, target_record_id))
                    .or_default()
                    .push(position),
                _ => {}
            }
        }

        index
    }

    pub(super) fn evidence_for_hit(&self, hit: &CodeRetrievalHit) -> Vec<CodeRepositoryCrossEdge> {
        let mut matches = BTreeMap::<usize, ()>::new();
        if hit.edge_kind.as_deref() == Some("import") {
            let import_origins = self
                .import_origins
                .get_or_init(|| self.build_import_origin_indexes());
            self.collect(
                import_origins.lines.get(&(
                    hit.scope_id.clone(),
                    hit.path.clone(),
                    hit.line_range.start,
                    hit.line_range.end,
                )),
                &mut matches,
            );
        } else {
            let import_origins = self
                .import_origins
                .get_or_init(|| self.build_import_origin_indexes());
            self.collect(
                import_origins
                    .files
                    .get(&(hit.scope_id.clone(), hit.path.clone())),
                &mut matches,
            );
        }
        if let Some(symbol_id) = &hit.symbol_snapshot_id {
            self.collect(
                self.target_symbols
                    .get(&(hit.scope_id.clone(), symbol_id.clone())),
                &mut matches,
            );
        }
        if let Some(file_id) = &hit.file_id {
            self.collect(
                self.target_files
                    .get(&(hit.scope_id.clone(), file_id.clone())),
                &mut matches,
            );
        }

        matches
            .into_keys()
            .take(5)
            .map(|position| self.edges[position].clone())
            .collect()
    }

    fn build_import_origin_indexes(&self) -> ImportOriginIndexes {
        let mut lines = BTreeMap::new();
        let mut files = BTreeMap::new();
        for (position, edge) in self.edges.iter().enumerate() {
            if edge.from_record_kind != "module_reference" {
                continue;
            }
            if let Some((path, line_start, line_end)) = evidence_origin(&edge.evidence_json) {
                lines
                    .entry((
                        edge.from_source_scope.clone(),
                        path.clone(),
                        line_start,
                        line_end,
                    ))
                    .or_insert_with(Vec::new)
                    .push(position);
                files
                    .entry((edge.from_source_scope.clone(), path))
                    .or_insert_with(Vec::new)
                    .push(position);
            }
        }
        for positions in files.values_mut() {
            positions.sort_by(|left, right| {
                self.edges[*right]
                    .confidence_basis_points
                    .cmp(&self.edges[*left].confidence_basis_points)
                    .then_with(|| left.cmp(right))
            });
            positions.truncate(MAX_FILE_ORIGIN_EVIDENCE);
            positions.sort_unstable();
        }

        ImportOriginIndexes { lines, files }
    }

    fn collect(&self, edges: Option<&Vec<usize>>, matches: &mut BTreeMap<usize, ()>) {
        let Some(edges) = edges else {
            return;
        };
        for position in edges {
            matches.insert(*position, ());
        }
    }
}

const MAX_REPOSITORY_SET_CANDIDATES_PER_MEMBER: usize = 50;
const MAX_MULTI_MEMBER_MINIMUM_CANDIDATES: usize = 15;
const MIN_REPOSITORY_SET_CANDIDATES_PER_MEMBER: usize = 6;
const REPOSITORY_SET_TOTAL_FANOUT_MULTIPLIER: usize = 3;
const MAX_DIVERSIFIED_RESULTS_PER_MEMBER: usize = 3;
const DIVERSITY_MIN_RELATIVE_SCORE: f64 = 0.45;
const DIVERSITY_MAX_SCORE_GAP: f64 = 10.0;

pub(super) fn per_member_candidate_limit(limit: usize, member_count: usize) -> usize {
    if member_count == 0 {
        return 0;
    }

    let requested = limit.max(1);
    let single_member_depth = requested
        .saturating_mul(REPOSITORY_SET_TOTAL_FANOUT_MULTIPLIER)
        .max(requested.saturating_add(5));
    if member_count == 1 {
        return single_member_depth.min(MAX_REPOSITORY_SET_CANDIDATES_PER_MEMBER);
    }

    let minimum = requested.saturating_add(5).clamp(
        MIN_REPOSITORY_SET_CANDIDATES_PER_MEMBER,
        MAX_MULTI_MEMBER_MINIMUM_CANDIDATES,
    );
    let shared_budget = requested
        .saturating_mul(REPOSITORY_SET_TOTAL_FANOUT_MULTIPLIER)
        .max(minimum);
    shared_budget
        .div_ceil(member_count)
        .max(minimum)
        .min(MAX_REPOSITORY_SET_CANDIDATES_PER_MEMBER)
}

pub(super) fn repository_set_score(
    hit: &CodeRetrievalHit,
    member: &CodeRepositorySetMemberStatus,
    overlay_evidence: &[CodeRepositoryCrossEdge],
) -> f64 {
    let freshness_penalty = if hit.stale || member.stale { 0.5 } else { 0.0 };
    let priority_bonus = member_priority_bonus(
        member.member.priority,
        freshness_penalty == 0.0,
        overlay_evidence,
    );
    let edge_bonus = overlay_evidence
        .iter()
        .map(|edge| f64::from(edge.confidence_basis_points) / 10_000.0)
        .fold(0.0, f64::max);

    hit.score + priority_bonus + edge_bonus - freshness_penalty
}

pub(super) fn apply_bridge_support_bonus(results: &mut [CodeRepositorySetQueryHit]) {
    let mut origin_files = BTreeSet::new();
    let mut target_records = BTreeSet::new();
    let mut bridge_edges = BTreeMap::new();
    for result in results.iter() {
        origin_files.insert((result.hit.scope_id.clone(), result.hit.path.clone()));
        target_records.extend(hit_target_records(&result.hit));
        for edge in &result.overlay_evidence {
            if bridge_support_bonus(edge) > 0.0 {
                bridge_edges.insert(edge.edge_id.clone(), edge.clone());
            }
        }
    }

    let mut supported_origin_files = BTreeMap::new();
    let mut supported_target_records = BTreeMap::new();
    for edge in bridge_edges.values() {
        let Some((origin_path, _, _)) = evidence_origin(&edge.evidence_json) else {
            continue;
        };
        let Some(target_record) = edge_target_record(edge) else {
            continue;
        };
        let origin_file = (edge.from_source_scope.clone(), origin_path);
        if !origin_files.contains(&origin_file) || !target_records.contains(&target_record) {
            continue;
        }
        let bonus = bridge_support_bonus(edge);
        add_capped_bonus(&mut supported_origin_files, origin_file, bonus);
        add_capped_bonus(&mut supported_target_records, target_record, bonus);
    }

    for result in results {
        let mut bonus = supported_origin_files
            .get(&(result.hit.scope_id.clone(), result.hit.path.clone()))
            .copied()
            .unwrap_or(0.0);
        for target_record in hit_target_records(&result.hit) {
            if let Some(target_bonus) = supported_target_records.get(&target_record) {
                bonus += *target_bonus;
            }
        }
        result.score += bonus.min(MAX_BRIDGE_SUPPORT_BONUS);
    }
}

pub(super) fn prune_returned_overlay_evidence(results: &mut [CodeRepositorySetQueryHit]) {
    let retained_bridge_edges = retained_bridge_edge_ids(results);
    for result in results {
        let hit_is_import = hit_is_import_edge(&result.hit);
        let target_records = hit_target_records(&result.hit);
        result.overlay_evidence.retain(|edge| {
            hit_is_import
                || target_records
                    .iter()
                    .any(|target_record| edge_targets_record(edge, target_record))
                || retained_bridge_edges.contains(&edge.edge_id)
        });
    }
}

fn retained_bridge_edge_ids(results: &[CodeRepositorySetQueryHit]) -> BTreeSet<String> {
    let mut origin_files = BTreeSet::new();
    let mut target_records = BTreeSet::new();
    for result in results {
        origin_files.insert((result.hit.scope_id.clone(), result.hit.path.clone()));
        target_records.extend(hit_target_records(&result.hit));
    }

    let mut retained = BTreeSet::new();
    for result in results {
        for edge in &result.overlay_evidence {
            let Some((origin_path, _, _)) = evidence_origin(&edge.evidence_json) else {
                continue;
            };
            let Some(target_record) = edge_target_record(edge) else {
                continue;
            };
            let origin_file = (edge.from_source_scope.clone(), origin_path);
            if bridge_support_bonus(edge) > 0.0
                && origin_files.contains(&origin_file)
                && target_records.contains(&target_record)
            {
                retained.insert(edge.edge_id.clone());
            }
        }
    }

    retained
}

pub(super) fn dedupe_sort_truncate(
    results: &mut Vec<CodeRepositorySetQueryHit>,
    limit: usize,
) -> bool {
    let mut best =
        BTreeMap::<(String, String, String, u32, u32, String), CodeRepositorySetQueryHit>::new();
    for result in results.drain(..) {
        let key = (
            result.hit.repository_id.clone(),
            result.hit.scope_id.clone(),
            result.hit.path.clone(),
            result.hit.line_range.start,
            result.hit.line_range.end,
            result.hit.excerpt.clone(),
        );
        match best.get(&key) {
            Some(existing) if existing.score >= result.score => {}
            _ => {
                best.insert(key, result);
            }
        }
    }
    results.extend(best.into_values());
    sort_repository_set_results(results);
    let truncated = results.len() > limit;
    if truncated {
        diversify_repository_set_results(results, limit);
    }
    results.truncate(limit);
    truncated
}

fn sort_repository_set_results(results: &mut [CodeRepositorySetQueryHit]) {
    results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| {
                left.member
                    .repository_alias
                    .cmp(&right.member.repository_alias)
            })
            .then_with(|| {
                path_specificity_key(&left.hit.path).cmp(&path_specificity_key(&right.hit.path))
            })
            .then_with(|| left.hit.path.cmp(&right.hit.path))
            .then_with(|| left.hit.line_range.start.cmp(&right.hit.line_range.start))
    });
}

fn diversify_repository_set_results(results: &mut Vec<CodeRepositorySetQueryHit>, limit: usize) {
    if limit == 0 {
        return;
    }
    let member_order = repository_set_member_order(results);
    if member_order.len() <= 1 {
        return;
    }

    let target_per_member =
        (limit / member_order.len()).clamp(1, MAX_DIVERSIFIED_RESULTS_PER_MEMBER);
    let score_floor = diversified_member_score_floor(results[0].score);
    let mut selected = BTreeSet::new();
    let mut counts = BTreeMap::<(String, String), usize>::new();
    for member_key in &member_order {
        while selected.len() < limit
            && counts.get(member_key).copied().unwrap_or(0) < target_per_member
        {
            let Some(index) = results.iter().enumerate().position(|(index, result)| {
                !selected.contains(&index)
                    && repository_set_member_key(result) == *member_key
                    && result.score >= score_floor
            }) else {
                break;
            };
            selected.insert(index);
            *counts.entry(member_key.clone()).or_insert(0) += 1;
        }
    }

    for index in 0..results.len() {
        if selected.len() >= limit {
            break;
        }
        selected.insert(index);
    }

    *results = selected
        .into_iter()
        .map(|index| results[index].clone())
        .collect();
}

fn repository_set_member_order(results: &[CodeRepositorySetQueryHit]) -> Vec<(String, String)> {
    let mut members = Vec::new();
    for result in results {
        let key = repository_set_member_key(result);
        if !members.contains(&key) {
            members.push(key);
        }
    }

    members
}

fn repository_set_member_key(result: &CodeRepositorySetQueryHit) -> (String, String) {
    (
        result.member.repository_id.clone(),
        result.member.source_scope.clone(),
    )
}

fn diversified_member_score_floor(best_score: f64) -> f64 {
    if best_score <= 0.0 {
        return f64::INFINITY;
    }

    (best_score * DIVERSITY_MIN_RELATIVE_SCORE).max(best_score - DIVERSITY_MAX_SCORE_GAP)
}

fn member_priority_bonus(
    priority: i32,
    fresh: bool,
    overlay_evidence: &[CodeRepositoryCrossEdge],
) -> f64 {
    if !fresh || !has_resolved_overlay_evidence(overlay_evidence) {
        return f64::from(priority) * 0.01;
    }

    f64::from(priority.clamp(
        -MAX_ABSOLUTE_MEMBER_PRIORITY_SCORE,
        MAX_ABSOLUTE_MEMBER_PRIORITY_SCORE,
    )) * EVIDENCE_BACKED_PRIORITY_SCORE_STEP
}

fn has_resolved_overlay_evidence(overlay_evidence: &[CodeRepositoryCrossEdge]) -> bool {
    overlay_evidence.iter().any(|edge| {
        edge.resolution_state == "resolved"
            && edge.confidence_basis_points > 0
            && edge.to_source_scope.is_some()
    })
}

fn path_specificity_key(path: &str) -> (usize, usize) {
    (path_specialization_count(path), path.split('/').count())
}

fn path_specialization_count(path: &str) -> usize {
    path.split('/')
        .map(|segment| segment.rsplit_once('.').map_or(segment, |(stem, _)| stem))
        .map(|stem| {
            stem.chars()
                .filter(|character| matches!(character, '-' | '_'))
                .count()
        })
        .sum()
}

fn bridge_support_bonus(edge: &CodeRepositoryCrossEdge) -> f64 {
    if edge.resolution_state != "resolved" || edge.to_source_scope.is_none() {
        return 0.0;
    }

    RESOLVED_BRIDGE_SUPPORT_BONUS * f64::from(edge.confidence_basis_points) / 10_000.0
}

fn add_capped_bonus<K: Ord>(bonuses: &mut BTreeMap<K, f64>, key: K, bonus: f64) {
    let entry = bonuses.entry(key).or_insert(0.0);
    *entry = (*entry + bonus).min(MAX_BRIDGE_SUPPORT_BONUS);
}

fn edge_target_record(edge: &CodeRepositoryCrossEdge) -> Option<(String, String, String)> {
    Some((
        edge.to_source_scope.clone()?,
        edge.to_record_kind.clone(),
        edge.to_record_id.clone()?,
    ))
}

fn edge_targets_record(edge: &CodeRepositoryCrossEdge, record: &(String, String, String)) -> bool {
    edge_target_record(edge).as_ref() == Some(record)
}

fn hit_target_records(hit: &CodeRetrievalHit) -> Vec<(String, String, String)> {
    let mut records = Vec::with_capacity(2);
    if let Some(symbol_id) = &hit.symbol_snapshot_id {
        records.push((
            hit.scope_id.clone(),
            "code_symbol_snapshot".to_owned(),
            symbol_id.clone(),
        ));
    }
    if let Some(file_id) = &hit.file_id {
        records.push((
            hit.scope_id.clone(),
            "code_file".to_owned(),
            file_id.clone(),
        ));
    }

    records
}

fn hit_is_import_edge(hit: &CodeRetrievalHit) -> bool {
    hit.edge_kind.as_deref() == Some("import")
}

fn evidence_origin(evidence_json: &str) -> Option<(String, u32, u32)> {
    serde_json::from_str::<serde_json::Value>(evidence_json)
        .ok()
        .and_then(|value| {
            let path = value
                .get("from_path")
                .and_then(|path| path.as_str())
                .map(str::to_owned)?;
            let line_start = value
                .get("from_line_start")
                .and_then(|line| line.as_u64())
                .and_then(|line| u32::try_from(line).ok())?;
            let line_end = value
                .get("from_line_end")
                .and_then(|line| line.as_u64())
                .and_then(|line| u32::try_from(line).ok())?;

            Some((path, line_start, line_end))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRepositorySetMember, CodeRetrievalLayer, RepositoryCodeRange};

    #[test]
    fn overlay_index_attaches_target_and_import_origin_evidence_in_edge_order() {
        let inbound = edge(
            "edge-in",
            "scope-service",
            Some("scope-app"),
            r#"{"from_path":"src/service.rs"}"#,
            9_000,
        );
        let outbound = edge(
            "edge-out",
            "scope-app",
            Some("scope-service"),
            r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
            6_000,
        );
        let unrelated = edge(
            "edge-other",
            "scope-other",
            Some("scope-service"),
            r#"{"from_path":"src/other.rs","from_line_start":1,"from_line_end":1}"#,
            10_000,
        );
        let mut wrong_target = edge(
            "edge-wrong",
            "scope-service",
            Some("scope-app"),
            "{}",
            8_000,
        );
        wrong_target.to_record_id = Some("symbol-other".to_owned());
        let edges = vec![inbound.clone(), outbound.clone(), unrelated, wrong_target];
        let index = OverlayEvidenceIndex::new(&edges);

        let evidence =
            index.evidence_for_hit(&hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false));
        assert_eq!(evidence, vec![inbound, outbound.clone()]);

        let mut import_hit = hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false);
        import_hit.symbol_snapshot_id = None;
        import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
        import_hit.edge_kind = Some("import".to_owned());
        assert_eq!(index.evidence_for_hit(&import_hit), vec![outbound]);
    }

    #[test]
    fn overlay_index_caps_file_origin_evidence_for_non_import_hits() {
        let mut edges = Vec::new();
        for (index, confidence) in [0, 5_000, 10_000, 7_000].into_iter().enumerate() {
            edges.push(edge(
                &format!("edge-origin-{index}"),
                "scope-app",
                Some("scope-service"),
                r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
                confidence,
            ));
        }
        let index = OverlayEvidenceIndex::new(&edges);

        let evidence = index.evidence_for_hit(&hit(
            "repo-a",
            "scope-app",
            "src/client.rs",
            20,
            0.75,
            false,
        ));

        assert_eq!(evidence.len(), 2);
        assert_eq!(evidence[0].edge_id, "edge-origin-2");
        assert_eq!(evidence[1].edge_id, "edge-origin-3");
    }

    #[test]
    fn overlay_index_dedupes_and_caps_multi_key_evidence() {
        let mut edges = Vec::new();
        for index in 0..8 {
            edges.push(edge(
                &format!("edge-{index}"),
                "scope-service",
                Some("scope-app"),
                r#"{"from_path":"src/service.rs"}"#,
                5_000,
            ));
        }
        let mut file_edge = edge(
            "edge-file",
            "scope-service",
            Some("scope-app"),
            r#"{"from_path":"src/service.rs"}"#,
            4_000,
        );
        file_edge.to_record_kind = "code_file".to_owned();
        file_edge.to_record_id = Some("file-1".to_owned());
        edges.insert(2, file_edge.clone());
        let index = OverlayEvidenceIndex::new(&edges);

        let evidence =
            index.evidence_for_hit(&hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false));

        assert_eq!(evidence.len(), 5);
        assert_eq!(evidence[2], file_edge);
    }

    #[test]
    fn ranking_helpers_keep_existing_merge_policy() {
        let member = member_status("app", "scope-app", 7);
        let base_hit = hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false);
        let evidence = vec![edge(
            "edge-in",
            "scope-service",
            Some("scope-app"),
            r#"{"from_path":"src/service.rs"}"#,
            9_000,
        )];
        assert!(repository_set_score(&base_hit, &member, &evidence) > base_hit.score);
        assert!(
            repository_set_score(
                &hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, true),
                &member,
                &[]
            ) < base_hit.score
        );

        let mut results = vec![
            CodeRepositorySetQueryHit {
                member: member.member.clone(),
                hit: hit("repo-a", "scope-app", "src/client.rs", 1, 0.50, false),
                overlay_evidence: Vec::new(),
                score: 0.50,
            },
            CodeRepositorySetQueryHit {
                member: member.member.clone(),
                hit: hit("repo-a", "scope-app", "src/client.rs", 1, 0.90, false),
                overlay_evidence: evidence,
                score: 0.90,
            },
            CodeRepositorySetQueryHit {
                member: member.member.clone(),
                hit: hit("repo-a", "scope-app", "src/client.rs", 2, 0.80, false),
                overlay_evidence: Vec::new(),
                score: 0.80,
            },
        ];
        assert!(dedupe_sort_truncate(&mut results, 1));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 0.90);

        assert!(evidence_origin("not-json").is_none());
        assert!(evidence_origin("{}").is_none());
    }

    #[test]
    fn candidate_limit_keeps_single_member_depth_and_shares_multi_member_budget() {
        assert_eq!(per_member_candidate_limit(10, 0), 0);
        assert_eq!(per_member_candidate_limit(1, 1), 6);
        assert_eq!(per_member_candidate_limit(20, 1), 50);
        assert_eq!(per_member_candidate_limit(10, 2), 15);
        assert_eq!(per_member_candidate_limit(20, 2), 30);
        assert_eq!(per_member_candidate_limit(20, 4), 15);
    }

    #[test]
    fn evidence_backed_member_priority_is_bounded_workspace_ranking_intent() {
        let preferred = member_status("app", "scope-app", 10);
        let dependency = member_status("sdk", "scope-sdk", 0);
        let preferred_hit = hit("repo-app", "scope-app", "src/client.rs", 1, 11.20, false);
        let dependency_hit = hit("repo-sdk", "scope-sdk", "src/client.rs", 1, 12.20, false);
        let evidence = vec![edge(
            "edge-in",
            "scope-app",
            Some("scope-sdk"),
            r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
            10_000,
        )];

        assert!(
            repository_set_score(&preferred_hit, &preferred, &evidence)
                > repository_set_score(&dependency_hit, &dependency, &[])
        );
        assert!(
            repository_set_score(&preferred_hit, &preferred, &[])
                < repository_set_score(&dependency_hit, &dependency, &[])
        );
        assert_eq!(
            member_priority_bonus(100, true, &evidence),
            member_priority_bonus(10, true, &evidence)
        );
        assert_eq!(
            member_priority_bonus(-100, true, &evidence),
            member_priority_bonus(-10, true, &evidence)
        );
    }

    #[test]
    fn repository_set_ties_prefer_less_specialized_paths() {
        let member = member_status("app", "scope-app", 0);
        let mut results = vec![
            CodeRepositorySetQueryHit {
                member: member.member.clone(),
                hit: hit(
                    "repo-app",
                    "scope-app",
                    "samples/verbose_client/main.rs",
                    1,
                    1.0,
                    false,
                ),
                overlay_evidence: Vec::new(),
                score: 2.0,
            },
            CodeRepositorySetQueryHit {
                member: member.member.clone(),
                hit: hit("repo-app", "scope-app", "samples/client.rs", 1, 1.0, false),
                overlay_evidence: Vec::new(),
                score: 2.0,
            },
        ];

        assert!(!dedupe_sort_truncate(&mut results, 2));

        assert_eq!(results[0].hit.path, "samples/client.rs");
    }

    #[test]
    fn repository_set_top_k_diversifies_relevant_members() {
        let app = member_status("app", "scope-app", 0);
        let sdk = member_status("sdk", "scope-sdk", 0);
        let mut results = vec![
            set_hit(&app, 1, 12.0),
            set_hit(&app, 2, 11.8),
            set_hit(&app, 3, 11.6),
            set_hit(&app, 4, 11.4),
            set_hit(&sdk, 10, 8.9),
            set_hit(&sdk, 11, 8.7),
        ];

        assert!(dedupe_sort_truncate(&mut results, 5));

        assert_eq!(results[0].member.repository_alias, "app");
        assert_eq!(
            results
                .iter()
                .filter(|result| result.member.repository_alias == "sdk")
                .count(),
            2
        );
    }

    #[test]
    fn bridge_support_bonus_promotes_present_usage_and_target_pair() {
        let app = member_status("app", "scope-app", 0);
        let service = member_status("svc", "scope-service", 0);
        let bridge = edge(
            "edge-bridge",
            "scope-app",
            Some("scope-service"),
            r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
            10_000,
        );
        let mut results = vec![
            CodeRepositorySetQueryHit {
                member: app.member.clone(),
                hit: hit("repo-app", "scope-app", "src/client.rs", 20, 0.80, false),
                overlay_evidence: vec![bridge.clone()],
                score: 0.80,
            },
            CodeRepositorySetQueryHit {
                member: service.member.clone(),
                hit: hit(
                    "repo-service",
                    "scope-service",
                    "src/service.rs",
                    1,
                    0.70,
                    false,
                ),
                overlay_evidence: vec![bridge.clone()],
                score: 0.70,
            },
        ];

        apply_bridge_support_bonus(&mut results);
        prune_returned_overlay_evidence(&mut results);

        assert!(results[0].score > 0.80);
        assert!(results[1].score > 0.70);
        assert_eq!(results[0].overlay_evidence, vec![bridge.clone()]);
        assert_eq!(results[1].overlay_evidence, vec![bridge]);
    }

    #[test]
    fn bridge_support_bonus_requires_both_resolved_endpoints() {
        let app = member_status("app", "scope-app", 0);
        let missing_target = edge(
            "edge-missing",
            "scope-app",
            Some("scope-service"),
            r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
            10_000,
        );
        let mut unresolved = missing_target.clone();
        unresolved.resolution_state = "unresolved".to_owned();
        let mut results = vec![
            CodeRepositorySetQueryHit {
                member: app.member.clone(),
                hit: hit("repo-app", "scope-app", "src/client.rs", 20, 0.80, false),
                overlay_evidence: vec![missing_target],
                score: 0.80,
            },
            CodeRepositorySetQueryHit {
                member: app.member.clone(),
                hit: hit("repo-app", "scope-app", "src/other.rs", 30, 0.70, false),
                overlay_evidence: vec![unresolved],
                score: 0.70,
            },
        ];

        apply_bridge_support_bonus(&mut results);

        assert_eq!(results[0].score, 0.80);
        assert_eq!(results[1].score, 0.70);
    }

    #[test]
    fn returned_overlay_evidence_prunes_origin_only_file_noise() {
        let app = member_status("app", "scope-app", 0);
        let origin_only = edge(
            "edge-origin-only",
            "scope-app",
            Some("scope-service"),
            r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
            10_000,
        );
        let mut results = vec![CodeRepositorySetQueryHit {
            member: app.member.clone(),
            hit: hit("repo-app", "scope-app", "src/client.rs", 20, 0.80, false),
            overlay_evidence: vec![origin_only],
            score: 0.80,
        }];

        prune_returned_overlay_evidence(&mut results);

        assert!(results[0].overlay_evidence.is_empty());
    }

    fn member_status(
        repository_alias: &str,
        source_scope: &str,
        priority: i32,
    ) -> CodeRepositorySetMemberStatus {
        CodeRepositorySetMemberStatus {
            member: CodeRepositorySetMember {
                set_id: "set-workspace".to_owned(),
                repository_id: format!("repo-{repository_alias}"),
                repository_alias: repository_alias.to_owned(),
                ref_selector: "HEAD".to_owned(),
                resolved_commit_sha: format!("commit-{source_scope}"),
                source_scope: source_scope.to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
                priority,
            },
            tree_hash: format!("tree-{source_scope}"),
            freshness_state: "fresh".to_owned(),
            stale: false,
            indexed_file_count: 1,
            symbol_count: 1,
            reference_count: 0,
            chunk_count: 1,
            degraded_reason: None,
        }
    }

    fn hit(
        repository_id: &str,
        scope_id: &str,
        path: &str,
        line: u32,
        score: f64,
        stale: bool,
    ) -> CodeRetrievalHit {
        CodeRetrievalHit {
            repository_id: repository_id.to_owned(),
            scope_id: scope_id.to_owned(),
            resolved_commit_sha: format!("commit-{scope_id}"),
            tree_hash: format!("tree-{scope_id}"),
            path: path.to_owned(),
            language_id: "rust".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 10 },
            line_range: RepositoryCodeRange {
                start: line,
                end: line,
            },
            symbol_snapshot_id: Some(format!("symbol-{line}")),
            canonical_symbol_id: None,
            file_id: Some("file-1".to_owned()),
            retrieval_layers: vec![CodeRetrievalLayer::Symbol],
            index_versions: vec!["code:1".to_owned()],
            stale,
            degraded_reason: None,
            edge_kind: None,
            edge_resolution_state: None,
            edge_target_hint: None,
            edge_confidence_basis_points: None,
            edge_confidence_tier: None,
            score,
            excerpt: format!("excerpt {line}"),
        }
    }

    fn set_hit(
        member: &CodeRepositorySetMemberStatus,
        line: u32,
        score: f64,
    ) -> CodeRepositorySetQueryHit {
        CodeRepositorySetQueryHit {
            member: member.member.clone(),
            hit: hit(
                &member.member.repository_id,
                &member.member.source_scope,
                &format!("src/{}.rs", line),
                line,
                score,
                false,
            ),
            overlay_evidence: Vec::new(),
            score,
        }
    }

    fn edge(
        edge_id: &str,
        from_scope: &str,
        to_scope: Option<&str>,
        evidence_json: &str,
        confidence: u16,
    ) -> CodeRepositoryCrossEdge {
        CodeRepositoryCrossEdge {
            edge_id: edge_id.to_owned(),
            set_id: "set-workspace".to_owned(),
            from_source_scope: from_scope.to_owned(),
            from_repository_id: "repo-from".to_owned(),
            from_record_kind: "module_reference".to_owned(),
            from_record_id: "import-1".to_owned(),
            to_source_scope: to_scope.map(str::to_owned),
            to_repository_id: to_scope.map(|_| "repo-to".to_owned()),
            to_record_kind: "code_symbol_snapshot".to_owned(),
            to_record_id: to_scope.map(|_| "symbol-1".to_owned()),
            edge_kind: "imports".to_owned(),
            resolution_state: "resolved".to_owned(),
            confidence_basis_points: confidence,
            confidence_tier: "explicit".to_owned(),
            evidence_json: evidence_json.to_owned(),
            created_at_ms: 10,
        }
    }
}
