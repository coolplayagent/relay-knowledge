//! Consistency uses scoped indexed files, including empty configuration templates.
use super::*;
pub(super) fn formats(
    connection: &Connection,
    scope: &str,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
) -> Result<BTreeSet<String>, StorageError> {
    // The same path/language predicates apply to facts and the file inventory.
    let filter = feature_flag_sql_filter(scope, status, request, &[]);
    let sql = format!(
        "SELECT DISTINCT language_id FROM code_repository_files flag WHERE ({}) AND language_id IN ('java','properties','ini','gotemplate','bash') AND (language_id != 'gotemplate' OR lower(path) LIKE '%.ctmpl') LIMIT 6",
        filter.where_clause
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(filter.params.iter()), |row| {
        row.get::<_, String>(0)
    })?;
    rows.map(|row| {
        row.map(|language| match language.as_str() {
            "gotemplate" => "ctmpl".into(),
            "bash" => "shell".into(),
            _ => language,
        })
        .map_err(StorageError::from)
    })
    .collect()
}
pub(super) fn check(group: &mut CodeFeatureFlagGraph, formats: &BTreeSet<String>, fresh: bool) {
    if !fresh {
        group.analysis_complete = false;
        group
            .consistency_diagnostics
            .push("incomplete_analysis: served scope is stale or degraded".into());
        return;
    }
    let defaults = group
        .usages
        .iter()
        .filter_map(|u| u.metadata.default_value.as_ref())
        .collect::<BTreeSet<_>>();
    if defaults.len() > 1 {
        group.conflicting_default_sources = group
            .usages
            .iter()
            .filter(|u| u.metadata.default_value.is_some())
            .cloned()
            .collect();
        group.consistency_diagnostics.push(format!(
            "conflicting_defaults: {}",
            defaults.into_iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if !group.analysis_complete {
        group
            .consistency_diagnostics
            .push("incomplete_analysis: absence cannot be proven".into());
        return;
    }
    let definitions = group.usages.iter().any(|u| u.edge_kind == "defines_config");
    if matches!(group.source_kind.as_str(), "config_key" | "env_var")
        && !definitions
        && group.usages.iter().any(|u| u.edge_kind == "reads_config")
    {
        group
            .consistency_diagnostics
            .push("read_without_definition".into());
    }
    if group.source_kind == "config_key" {
        let present = group
            .usages
            .iter()
            .map(|u| &u.metadata.source_format)
            .collect::<BTreeSet<_>>();
        for format in formats {
            if !present.contains(format) {
                group
                    .consistency_diagnostics
                    .push(format!("missing_from_format: {format}"));
            }
        }
    }
}
