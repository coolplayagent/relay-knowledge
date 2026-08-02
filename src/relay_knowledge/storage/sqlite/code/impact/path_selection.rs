//! Changed-path filtering, language inference, and bounded SQLite language lookup.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::{CodeImpactRequest, CodeRepositoryStatus},
    storage::StorageError,
};

use super::super::query::{language_filter_allows, path_filter_allows, required_scope};

const CODE_PATH_LANGUAGE_SUFFIXES: &[(&str, &str)] = &[
    (".tsx", "tsx"),
    (".jsx", "jsx"),
    (".phtml", "php"),
    (".mts", "typescript"),
    (".cts", "typescript"),
    (".mjs", "javascript"),
    (".cjs", "javascript"),
    (".pyw", "python"),
    (".kts", "kotlin"),
    (".scala", "scala"),
    (".swift", "swift"),
    (".bash", "bash"),
    (".bats", "bash"),
    (".java", "java"),
    (".cpp", "cpp"),
    (".cxx", "cpp"),
    (".c++", "cpp"),
    (".hpp", "cpp"),
    (".hxx", "cpp"),
    (".h++", "cpp"),
    (".rs", "rust"),
    (".py", "python"),
    (".ts", "typescript"),
    (".js", "javascript"),
    (".go", "go"),
    (".kt", "kotlin"),
    (".sc", "scala"),
    (".cc", "cpp"),
    (".hh", "cpp"),
    (".cs", "csharp"),
    (".rb", "ruby"),
    (".php", "php"),
    (".sh", "bash"),
    (".c", "c"),
    (".h", "c"),
];
pub(super) const SQLITE_BIND_BATCH_SIZE: usize = 500;

pub(super) fn selected_changed_paths(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeImpactRequest,
    changed_paths: Vec<String>,
) -> Result<BTreeSet<String>, StorageError> {
    let source_scope = required_scope(status)?;
    let candidate_paths = changed_paths
        .into_iter()
        .filter(|path| {
            path_filter_allows(path, &status.path_filters)
                && path_filter_allows(path, &request.repository.path_filters)
        })
        .collect::<BTreeSet<_>>();
    let stored_languages = stored_languages_for_paths(connection, source_scope, &candidate_paths)?;
    let selected = candidate_paths
        .into_iter()
        .filter(|path| {
            stored_languages
                .get(path)
                .cloned()
                .or_else(|| language_id_for_path(path))
                .as_deref()
                .map(|language| {
                    language_filter_allows(language, &status.language_filters)
                        && language_filter_allows(language, &request.repository.language_filters)
                })
                .unwrap_or_else(|| {
                    status.language_filters.is_empty()
                        && request.repository.language_filters.is_empty()
                })
        })
        .collect();

    Ok(selected)
}

fn stored_languages_for_paths(
    connection: &Connection,
    source_scope: &str,
    paths: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, StorageError> {
    let mut languages = BTreeMap::new();
    for batch in batched_path_values(paths) {
        let path_clause = std::iter::repeat_n("?", batch.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "
            SELECT path, language_id
            FROM code_repository_files
            WHERE source_scope = ?1
              AND path IN ({path_clause})
            ",
        );
        let mut values = vec![Value::Text(source_scope.to_owned())];
        values.extend(batch.into_iter().map(Value::Text));
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (path, language_id) = row?;
            languages.insert(path, language_id);
        }
    }

    Ok(languages)
}

pub(super) fn batched_path_values(paths: &BTreeSet<String>) -> Vec<Vec<String>> {
    paths
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .chunks(SQLITE_BIND_BATCH_SIZE)
        .map(<[String]>::to_vec)
        .collect()
}

pub(super) fn impact_row_allowed(
    path: &str,
    language_id: &str,
    status: &CodeRepositoryStatus,
    request: &CodeImpactRequest,
) -> bool {
    path_filter_allows(path, &status.path_filters)
        && path_filter_allows(path, &request.repository.path_filters)
        && language_filter_allows(language_id, &status.language_filters)
        && language_filter_allows(language_id, &request.repository.language_filters)
}

pub(super) fn language_id_for_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let file_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    match file_name {
        ".bash_profile" | ".bashrc" | ".profile" | "bash_profile" | "bashrc" => {
            return Some("bash".to_owned());
        }
        "Gemfile" | "Rakefile" => return Some("ruby".to_owned()),
        _ => {}
    }
    language_suffix_for_path(&normalized).map(|(_, language_id)| language_id.to_owned())
}

pub(super) fn language_suffix_for_path(path: &str) -> Option<(&'static str, &'static str)> {
    let lower = path.to_ascii_lowercase();
    CODE_PATH_LANGUAGE_SUFFIXES
        .iter()
        .copied()
        .find(|(suffix, _)| lower.ends_with(suffix))
}
