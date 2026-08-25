//! Extracts bounded import fallback queries and candidate source paths.

use crate::domain::{CodeRetrievalHit, CodeRetrievalRequest};

use super::identity::{definition_identity, push_candidate_path};

const MAX_IMPORT_SOURCE_CANDIDATE_PATHS: usize = 32;

pub(super) fn import_grep_query(
    request: &CodeRetrievalRequest,
    results: &[CodeRetrievalHit],
) -> Option<String> {
    if let Some(query) = results
        .iter()
        .find_map(unindexed_external_import_specifier)
        .and_then(|specifier| {
            if specifier.len() <= 128 {
                Some(specifier)
            } else {
                definition_identity(&specifier)
            }
        })
    {
        return Some(query);
    }

    local_relative_import_query(request, results)
}

fn local_relative_import_query(
    request: &CodeRetrievalRequest,
    results: &[CodeRetrievalHit],
) -> Option<String> {
    if results.is_empty() || results.len() >= request.limit {
        return None;
    }
    let specifier = import_specifier(&request.query)?;
    (specifier.len() <= 128 && relative_path_import_specifier(&specifier)).then_some(specifier)
}

pub(super) fn import_grep_candidate_paths(
    results: &[CodeRetrievalHit],
    specifier: &str,
) -> Vec<String> {
    let mut paths = Vec::new();
    for hit in results {
        let Some(candidate) = unindexed_external_import_specifier(hit) else {
            continue;
        };
        if candidate == specifier {
            push_candidate_path(&mut paths, &hit.path);
        }
        if paths.len() >= MAX_IMPORT_SOURCE_CANDIDATE_PATHS {
            break;
        }
    }

    paths
}

pub(super) fn import_graph_results_have_complete_source_surface(
    results: &[CodeRetrievalHit],
    specifier: &str,
) -> bool {
    let matching = results
        .iter()
        .filter(|hit| unindexed_external_import_specifier(hit).as_deref() == Some(specifier))
        .collect::<Vec<_>>();
    !matching.is_empty()
        && matching
            .into_iter()
            .all(|hit| import_hit_has_complete_source_statement(hit, specifier))
}

pub(super) fn import_query_requires_source_search(query: &str, specifier: &str) -> bool {
    relative_path_import_specifier(specifier) || dynamic_import_query(query)
}

fn import_hit_has_complete_source_statement(hit: &CodeRetrievalHit, specifier: &str) -> bool {
    hit.excerpt.lines().any(|line| {
        let line = line.trim();
        source_like_import_statement(line)
            && external_import_specifier(line).as_deref() == Some(specifier)
    })
}

fn source_like_import_statement(line: &str) -> bool {
    line.starts_with("import ")
        || line.starts_with("from ")
        || line.starts_with("use ")
        || line.starts_with("pub use ")
        || line.starts_with("using ")
        || line.starts_with("require ")
        || line.starts_with("#include ")
        || line.starts_with(". \"")
        || line.starts_with(". '")
}

fn dynamic_import_query(query: &str) -> bool {
    query.match_indices("import").any(|(index, _)| {
        let before = query.get(..index).unwrap_or_default();
        let after = query
            .get(index + "import".len()..)
            .unwrap_or_default()
            .trim_start();
        before.chars().next_back().is_none_or(|character| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '.'
        }) && after.starts_with('(')
    })
}

fn unindexed_external_import_specifier(hit: &CodeRetrievalHit) -> Option<String> {
    if hit.edge_kind.as_deref() != Some("import")
        || hit.edge_resolution_state.as_deref() != Some("unresolved")
    {
        return None;
    }

    hit.edge_target_hint
        .as_deref()
        .and_then(external_import_specifier)
}

fn external_import_specifier(target_hint: &str) -> Option<String> {
    let specifier = import_specifier(target_hint)?;
    (!local_import_specifier(&specifier)).then_some(specifier)
}

fn import_specifier(target_hint: &str) -> Option<String> {
    let trimmed = target_hint.trim().trim_end_matches(';').trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(quoted) = quoted_import_specifier(trimmed) {
        return Some(quoted.to_owned());
    }
    if let Some(rest) = trimmed.strip_prefix("pub use ") {
        return statement_head(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("use ") {
        return statement_head(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("from ") {
        let module = rest
            .split_once(" import ")
            .map_or(rest, |(module, _)| module);
        return statement_head(module);
    }
    if let Some(rest) = trimmed.strip_prefix("import ") {
        let module = rest.split_once(" from ").map_or(rest, |(_, module)| module);
        return statement_head(module);
    }

    statement_head(trimmed)
}

pub(super) fn quoted_import_specifier(value: &str) -> Option<&str> {
    let mut quoted = None;
    for quote in ['"', '\'', '`'] {
        if let Some(start) = value.find(quote) {
            let after_start = value.get(start + quote.len_utf8()..)?;
            if let Some(end) = after_start.find(quote) {
                quoted = Some(after_start.get(..end)?);
            }
        }
    }
    quoted.filter(|specifier| !specifier.trim().is_empty())
}

fn statement_head(value: &str) -> Option<String> {
    let head = value
        .trim()
        .trim_matches(['"', '\'', '`', '<', '>'])
        .trim_end_matches(';')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_end_matches(',')
        .trim();
    (!head.is_empty()).then(|| head.to_owned())
}

pub(super) fn local_import_specifier(specifier: &str) -> bool {
    let specifier = specifier.trim();
    specifier.starts_with('.')
        || specifier.starts_with('/')
        || matches!(specifier, "crate" | "self" | "super")
        || specifier.starts_with("crate::")
        || specifier.starts_with("self::")
        || specifier.starts_with("super::")
}

pub(super) fn relative_path_import_specifier(specifier: &str) -> bool {
    let specifier = specifier.trim();
    specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/')
}

#[cfg(test)]
#[path = "imports_tests.rs"]
mod tests;
