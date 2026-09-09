//! Bounded configuration fact analysis over one persisted, authorized source scope.
use std::collections::{BTreeMap, BTreeSet};

use super::candidates::load;
use crate::{
    domain::{
        CodeConfigurationReadKind, CodeFeatureFlagGraph, CodeFeatureFlagRecord,
        CodeFeatureFlagRequest, CodeFeatureFlagUsage, CodeRepositoryStatus,
    },
    storage::StorageError,
};
use rusqlite::Connection;

pub(super) fn search(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    request
        .validate_query()
        .map_err(|error| StorageError::InvalidQueryArgument(error.to_string()))?;
    super::query_budget::run(connection, || {
        search_with_budget(connection, status, request)
    })
}

fn search_with_budget(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    const MAX_SEED_IDENTITIES: usize = 1000;
    let mut seeds = request.clone();
    seeds.limit = seeds.limit.min(MAX_SEED_IDENTITIES);
    loop {
        let (records, exhausted) = load(connection, status, &seeds)?;
        let mut result = assemble(records, status, request);
        if exhausted || result.len() >= request.limit {
            result.truncate(request.limit);
            attach_symbols(connection, status, &mut result)?;
            return Ok(result);
        }
        if seeds.limit == MAX_SEED_IDENTITIES {
            return Err(super::candidates::incomplete(
                "resolved result groups exhausted the 1000-seed budget",
            ));
        }
        seeds.limit = seeds.limit.saturating_mul(2).min(MAX_SEED_IDENTITIES);
    }
}

fn assemble(
    mut records: Vec<CodeFeatureFlagRecord>,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
) -> Vec<CodeFeatureFlagGraph> {
    let states = resolve(&mut records);
    let records = promote_bound_declarations(records, states);
    let mut formats = BTreeMap::<String, BTreeSet<String>>::new();
    for (record, _) in &records {
        if !record.metadata.source_format.is_empty() {
            formats
                .entry(record.source_kind.clone())
                .or_default()
                .insert(record.metadata.source_format.clone());
        }
    }
    let mut groups = BTreeMap::<(String, String), CodeFeatureFlagGraph>::new();
    for (record, resolution_state) in records {
        let key = (record.source_kind.clone(), record.source_key.clone());
        let group = groups.entry(key).or_insert_with(|| CodeFeatureFlagGraph {
            feature_flag_id: record.feature_flag_id.clone(),
            name: record.name.clone(),
            source_kind: record.source_kind.clone(),
            source_key: record.source_key.clone(),
            score: 0.0,
            usages: Vec::new(),
            consistency_diagnostics: Vec::new(),
            analysis_complete: true,
        });
        let score = match record.edge_kind.as_str() {
            "guards_code" => 20.0,
            "defines_config" => 16.0,
            _ => 12.0,
        } + f64::from(record.confidence_basis_points) / 1000.0;
        group.score = group.score.max(score);
        group.analysis_complete &= !matches!(resolution_state.as_str(), "ambiguous" | "unresolved");
        group.usages.push(CodeFeatureFlagUsage {
            metadata: record.metadata,
            resolution_state,
            usage_id: record.usage_id,
            path: record.path,
            language_id: record.language_id,
            file_id: record.file_id,
            byte_range: record.byte_range,
            line_range: record.line_range,
            edge_kind: record.edge_kind,
            related_symbol_snapshot_id: None,
            related_symbol_name: None,
            confidence_basis_points: record.confidence_basis_points,
            confidence_tier: record.confidence_tier,
            excerpt: record.excerpt,
        });
    }
    let mut result = Vec::new();
    for mut group in groups.into_values() {
        if !matches_filters(&group, request) {
            continue;
        }
        if request.consistency {
            let namespace_formats = formats.get(&group.source_kind).cloned().unwrap_or_default();
            diagnose(&mut group, &namespace_formats, status);
        }
        group.usages.sort_by(|a, b| {
            edge_priority(&a.edge_kind)
                .cmp(&edge_priority(&b.edge_kind))
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.line_range.start.cmp(&b.line_range.start))
        });
        result.push(group);
    }
    result.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.source_key.cmp(&b.source_key))
    });
    result
}

fn resolve(records: &mut [CodeFeatureFlagRecord]) -> Vec<String> {
    type Target = (String, String, String);
    let mut bindings = BTreeMap::<String, Vec<usize>>::new();
    for (index, record) in records.iter().enumerate() {
        for binding in &record.metadata.bindings {
            bindings.entry(binding.clone()).or_default().push(index);
        }
    }
    // Evaluate the immutable binding graph. Resolving a record early must never hide
    // a conflicting or unresolved alternate definition discovered at the next hop.
    let mut previous = BTreeMap::<String, (BTreeSet<Target>, bool)>::new();
    for _ in 0..2 {
        let mut next = BTreeMap::new();
        for (symbol, indices) in &bindings {
            let mut targets = BTreeSet::new();
            let mut unknown = false;
            for &index in indices {
                let record = &records[index];
                if !matches!(
                    record.source_kind.as_str(),
                    "config_symbol" | "config_getter"
                ) {
                    insert_distinct_binding_target(
                        &mut targets,
                        (
                            record.source_kind.clone(),
                            record.source_key.clone(),
                            record.feature_flag_id.clone(),
                        ),
                    );
                } else {
                    let reference = record
                        .metadata
                        .referenced_symbol
                        .as_ref()
                        .unwrap_or(&record.source_key);
                    if let Some((resolved, incomplete)) = previous.get(reference) {
                        for target in resolved {
                            insert_distinct_binding_target(
                                &mut targets,
                                target_in_read_namespace(record, target.clone()),
                            );
                        }
                        unknown |= incomplete;
                    } else {
                        unknown = true;
                    }
                }
            }
            next.insert(symbol.clone(), (targets, unknown));
        }
        previous = next;
    }
    records
        .iter_mut()
        .map(|record| {
            if !matches!(
                record.source_kind.as_str(),
                "config_symbol" | "config_getter"
            ) {
                return "literal".to_owned();
            }
            let symbol = record
                .metadata
                .referenced_symbol
                .get_or_insert_with(|| record.source_key.clone());
            let Some((targets, unknown)) = previous.get(symbol) else {
                return "unresolved".to_owned();
            };
            let mut projected = BTreeSet::new();
            for target in targets {
                insert_distinct_binding_target(
                    &mut projected,
                    target_in_read_namespace(record, target.clone()),
                );
            }
            if projected.len() > 1 {
                return "ambiguous".to_owned();
            }
            if *unknown || projected.is_empty() {
                return "unresolved".to_owned();
            }
            let (kind, key, id) = projected.first().expect("one proven target");
            record.source_kind.clone_from(kind);
            record.source_key.clone_from(key);
            record.name.clone_from(key);
            record.feature_flag_id.clone_from(id);
            "resolved".to_owned()
        })
        .collect()
}

fn target_in_read_namespace(
    record: &CodeFeatureFlagRecord,
    mut target: (String, String, String),
) -> (String, String, String) {
    if let Some(kind) = record.metadata.read_source_kind {
        if target.0 != kind.as_str() {
            target.0 = kind.as_str().to_owned();
            target.2 = crate::identity::stable_id(
                "feature_flag",
                [
                    record.repository_id.as_str(),
                    record.source_scope.as_str(),
                    target.0.as_str(),
                    target.1.as_str(),
                ],
            );
        }
    }
    target
}

fn promote_bound_declarations(
    records: Vec<CodeFeatureFlagRecord>,
    states: Vec<String>,
) -> Vec<(CodeFeatureFlagRecord, String)> {
    let observed = super::binding_provenance::used_symbols(&records, &states);
    let mut projected = Vec::new();
    for (record, state) in records.into_iter().zip(states) {
        if record.source_kind == "config_getter" && state != "ambiguous" {
            continue;
        }
        if record.edge_kind != "binds_config_symbol" {
            projected.push((record, state));
            continue;
        }
        // A constant supplies a string, not a namespace. The two supported Java
        // read APIs can each establish a declaration relationship for that string.
        for kind in [
            CodeConfigurationReadKind::ConfigKey,
            CodeConfigurationReadKind::EnvVar,
        ] {
            let used = observed.get(&(kind.as_str().to_owned(), record.source_key.clone()));
            if !used
                .is_some_and(|symbols| record.metadata.bindings.iter().any(|s| symbols.contains(s)))
            {
                continue;
            }
            let mut declaration = record.clone();
            declaration.source_kind = kind.as_str().to_owned();
            declaration.edge_kind = "declares_config_key".to_owned();
            declaration.feature_flag_id = crate::identity::stable_id(
                "feature_flag",
                [
                    &record.repository_id,
                    &record.source_scope,
                    kind.as_str(),
                    &record.source_key,
                ],
            );
            declaration.usage_id = crate::identity::stable_id(
                "feature_flag_usage",
                [
                    record.usage_id.as_str(),
                    kind.as_str(),
                    "declares_config_key",
                ],
            );
            projected.push((declaration, state.clone()));
        }
    }
    projected
}

fn insert_distinct_binding_target(
    targets: &mut BTreeSet<(String, String, String)>,
    target: (String, String, String),
) {
    // Incremental scope copies retain occurrence IDs. Those IDs do not make two
    // declarations of the same logical source/key into conflicting destinations.
    let previous = targets
        .iter()
        .find(|existing| existing.0 == target.0 && existing.1 == target.1)
        .cloned();
    if let Some(previous) = previous {
        if previous.2 <= target.2 {
            return;
        }
        targets.remove(&previous);
    }
    targets.insert(target);
    while targets.len() > 2 {
        targets.pop_last();
    }
}

fn matches_filters(group: &CodeFeatureFlagGraph, request: &CodeFeatureFlagRequest) -> bool {
    if !group.usages.iter().any(|usage| {
        request
            .domain
            .as_ref()
            .is_none_or(|v| usage.metadata.domain.as_ref() == Some(v))
            && request
                .source
                .as_ref()
                .is_none_or(|v| &usage.metadata.source_format == v)
            && request
                .hot_reload
                .is_none_or(|v| usage.metadata.hot_reload == Some(v))
    }) {
        return false;
    }
    let Some(query) = &request.query else {
        return true;
    };
    let haystack = format!(
        "{} {} {} {}",
        group.name,
        group.source_kind,
        group.source_key,
        group
            .usages
            .iter()
            .map(|u| format!(
                "{} {} {} {} {}",
                u.path,
                u.edge_kind,
                u.excerpt,
                u.metadata.bindings.join(" "),
                u.metadata.referenced_symbol.as_deref().unwrap_or_default()
            ))
            .collect::<Vec<_>>()
            .join(" ")
    )
    .to_ascii_lowercase();
    CodeFeatureFlagRequest::query_terms(query)
        .all(|term| haystack.contains(&term.to_ascii_lowercase()))
}

fn diagnose(
    group: &mut CodeFeatureFlagGraph,
    formats: &BTreeSet<String>,
    status: &CodeRepositoryStatus,
) {
    if status.stale
        || status.degraded_reason.is_some()
        || group.source_kind == "config_symbol"
        || group
            .usages
            .iter()
            .any(|usage| matches!(usage.resolution_state.as_str(), "ambiguous" | "unresolved"))
        || group
            .usages
            .iter()
            .any(|u| u.metadata.source_format.is_empty())
    {
        group.analysis_complete = false;
        group.consistency_diagnostics.push("unknown: stale/degraded scope or unresolved configuration symbol; absence is not proven".to_owned());
        return;
    }
    let definitions = group
        .usages
        .iter()
        .filter(|u| {
            matches!(
                u.edge_kind.as_str(),
                "defines_config" | "declares_config_key"
            )
        })
        .map(|u| u.metadata.source_format.clone())
        .collect::<BTreeSet<_>>();
    if definitions.is_empty() {
        group.consistency_diagnostics.push(
            "read_without_definition: no definition in the selected indexed scope".to_owned(),
        );
    }
    let present_formats = group
        .usages
        .iter()
        .map(|usage| usage.metadata.source_format.clone())
        .collect::<BTreeSet<_>>();
    for format in formats.difference(&present_formats) {
        group.consistency_diagnostics.push(format!(
            "missing_from_format: {format}; comparison covers only selected indexed facts"
        ));
    }
    let defaults = group
        .usages
        .iter()
        .filter_map(|u| u.metadata.default_value.as_ref())
        .collect::<BTreeSet<_>>();
    if defaults.len() > 1 {
        group.consistency_diagnostics.push(
            "conflicting_defaults: inspect per-usage metadata and source evidence".to_owned(),
        );
    }
}

fn edge_priority(kind: &str) -> usize {
    match kind {
        "guards_code" => 0,
        "defines_config" => 1,
        _ => 2,
    }
}

fn attach_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    groups: &mut [CodeFeatureFlagGraph],
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "SELECT symbol_snapshot_id, name FROM code_repository_symbols
        WHERE source_scope = ?1 AND path = ?2 AND line_start <= ?3 AND line_end >= ?3
        ORDER BY line_start DESC, line_end ASC LIMIT 1",
    )?;
    for usage in groups.iter_mut().flat_map(|g| &mut g.usages) {
        let mut rows = statement.query(rusqlite::params![
            status.last_indexed_scope_id,
            usage.path,
            usage.line_range.start
        ])?;
        if let Some(row) = rows.next()? {
            usage.related_symbol_snapshot_id = Some(row.get(0)?);
            usage.related_symbol_name = Some(row.get(1)?);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "knowledge_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "knowledge_contract_tests.rs"]
mod contract_tests;
