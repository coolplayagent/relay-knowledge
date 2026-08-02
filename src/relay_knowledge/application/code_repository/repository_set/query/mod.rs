use std::{
    cell::OnceCell,
    collections::{BTreeMap, BTreeSet},
};

mod domain_affinity;
mod identity_coverage;
mod plan;
mod workflow;

use crate::domain::{
    CodeRepositoryCrossEdge, CodeRepositorySetMemberStatus, CodeRepositorySetQueryHit,
    CodeRetrievalHit,
};
use domain_affinity::priority_domain_affinity_bonus;
use identity_coverage::select_identity_coverage_results;

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

const MAX_RETURNED_OVERLAY_EVIDENCE: usize = 5;
const MAX_TARGET_EVIDENCE_PER_RECORD: usize = MAX_RETURNED_OVERLAY_EVIDENCE;
const MAX_LINE_ORIGIN_EVIDENCE: usize = MAX_RETURNED_OVERLAY_EVIDENCE;
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
                "code_file" => push_ordered_position(
                    index
                        .target_files
                        .entry((target_scope, target_record_id))
                        .or_default(),
                    position,
                    MAX_TARGET_EVIDENCE_PER_RECORD,
                ),
                "code_symbol_snapshot" => push_ordered_position(
                    index
                        .target_symbols
                        .entry((target_scope, target_record_id))
                        .or_default(),
                    position,
                    MAX_TARGET_EVIDENCE_PER_RECORD,
                ),
                _ => {}
            }
        }

        index
    }

    pub(super) fn evidence_for_hit(&self, hit: &CodeRetrievalHit) -> Vec<CodeRepositoryCrossEdge> {
        let mut matches = BTreeSet::new();
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
            .into_iter()
            .take(MAX_RETURNED_OVERLAY_EVIDENCE)
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
                push_ordered_position(
                    lines
                        .entry((
                            edge.from_source_scope.clone(),
                            path.clone(),
                            line_start,
                            line_end,
                        ))
                        .or_insert_with(Vec::new),
                    position,
                    MAX_LINE_ORIGIN_EVIDENCE,
                );
                push_file_origin_position(
                    self.edges,
                    files
                        .entry((edge.from_source_scope.clone(), path))
                        .or_insert_with(Vec::new),
                    position,
                );
            }
        }
        for positions in files.values_mut() {
            positions.sort_unstable();
        }

        ImportOriginIndexes { lines, files }
    }

    fn collect(&self, edges: Option<&Vec<usize>>, matches: &mut BTreeSet<usize>) {
        let Some(edges) = edges else {
            return;
        };
        for position in edges {
            matches.insert(*position);
        }
    }
}

fn push_ordered_position(positions: &mut Vec<usize>, position: usize, cap: usize) {
    if positions.len() < cap {
        positions.push(position);
    }
}

fn push_file_origin_position(
    edges: &[CodeRepositoryCrossEdge],
    positions: &mut Vec<usize>,
    position: usize,
) {
    positions.push(position);
    positions.sort_by(|left, right| {
        edges[*right]
            .confidence_basis_points
            .cmp(&edges[*left].confidence_basis_points)
            .then_with(|| left.cmp(right))
    });
    positions.truncate(MAX_FILE_ORIGIN_EVIDENCE);
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
    query: &str,
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
    let priority_supported =
        freshness_penalty == 0.0 && has_priority_supporting_overlay_evidence(overlay_evidence);
    let domain_affinity_bonus = if priority_supported {
        priority_domain_affinity_bonus(query, hit, member)
    } else {
        0.0
    };
    let edge_bonus = overlay_evidence
        .iter()
        .map(|edge| f64::from(edge.confidence_basis_points) / 10_000.0)
        .fold(0.0, f64::max);

    hit.score + priority_bonus + edge_bonus + domain_affinity_bonus - freshness_penalty
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
    query: &str,
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
        diversify_repository_set_results(results, limit, query);
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

fn diversify_repository_set_results(
    results: &mut Vec<CodeRepositorySetQueryHit>,
    limit: usize,
    query: &str,
) {
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

    select_identity_coverage_results(results, query, limit, &mut selected);

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
    if !fresh || !has_priority_supporting_overlay_evidence(overlay_evidence) {
        return f64::from(priority) * 0.01;
    }

    f64::from(priority.clamp(
        -MAX_ABSOLUTE_MEMBER_PRIORITY_SCORE,
        MAX_ABSOLUTE_MEMBER_PRIORITY_SCORE,
    )) * EVIDENCE_BACKED_PRIORITY_SCORE_STEP
}

fn has_priority_supporting_overlay_evidence(overlay_evidence: &[CodeRepositoryCrossEdge]) -> bool {
    overlay_evidence.iter().any(|edge| {
        edge.confidence_basis_points > 0
            && match edge.resolution_state.as_str() {
                "resolved" => edge.to_source_scope.is_some(),
                "ambiguous" => true,
                _ => false,
            }
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
mod test_support;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
