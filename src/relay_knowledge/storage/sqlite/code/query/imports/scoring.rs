use crate::domain::CodeQueryKind;

use super::super::{
    identifiers::identifier_terms_equivalent,
    scoring::path_ranking::path_looks_like_test_or_benchmark,
};
use super::{
    binding_terms::{
        camel_case_terms, identifier_tokens, import_usage_identifier_terms,
        named_import_binding_count_for_query, query_local_binding_terms, query_terms,
    },
    path_context::{
        file_stem, import_target_mentions_query, parent_dir, path_has_header_extension,
        query_contains_file_extension, query_looks_like_import_path,
        source_file_can_implement_header, stem_terms, target_stem, target_stem_terms,
    },
};

pub(super) fn import_line_priority(base_score: f64, line_start: u32, query: &str) -> f64 {
    if base_score <= 0.0 || !query_looks_like_import_path(query) {
        return 0.0;
    }

    let query = query.trim();
    let weight = if (query.starts_with("./") || query.starts_with("../"))
        && !query_contains_file_extension(query)
    {
        5.25
    } else {
        1.0
    };
    weight / f64::from(line_start.clamp(1, 1_000))
}

pub(in super::super) fn import_surface_bonus(
    base_score: f64,
    path: &str,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Hybrid {
        return 0.0;
    }
    if path
        .split('/')
        .any(|segment| matches!(segment, "test" | "tests" | "__tests__"))
    {
        return 0.0;
    }
    match path.rsplit('/').next().unwrap_or(path) {
        "__init__.py" | "mod.rs" | "lib.rs" | "index.js" | "index.jsx" | "index.ts"
        | "index.tsx" => 0.2,
        _ => 0.0,
    }
}

pub(super) fn import_statement_shape_bonus(
    base_score: f64,
    query: &str,
    module: &str,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Imports || !query_looks_like_import_path(query) {
        return 0.0;
    }
    let module = module.trim_start();
    if query_looks_like_bare_import(query) {
        return import_expression_or_side_effect_bonus(module);
    }
    if module.starts_with("import ") && module.contains(" from ") {
        0.25
    } else {
        0.0
    }
}

pub(in super::super) fn import_public_dependency_surface_bonus(
    base_score: f64,
    query: &str,
    path: &str,
    target_hint: Option<&str>,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0
        || !matches!(kind, CodeQueryKind::Hybrid | CodeQueryKind::Imports)
        || !query_looks_like_import_path(query)
    {
        return 0.0;
    }
    let target_is_header =
        target_hint.is_some_and(path_has_header_extension) || path_has_header_extension(query);
    if !target_is_header
        || !path_has_header_extension(path)
        || path_looks_like_test_or_benchmark(path)
    {
        return 0.0;
    }

    let same_public_directory_bonus = target_hint
        .and_then(parent_dir)
        .filter(|target_parent| parent_dir(path) == Some(*target_parent))
        .map_or(0.0, |_| 0.75);
    1.15 + same_public_directory_bonus
}

pub(in super::super) fn import_source_path_query_overlap_bonus(
    base_score: f64,
    query: &str,
    path: &str,
    target_hint: Option<&str>,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0
        || kind != CodeQueryKind::Imports
        || !query_looks_like_import_path(query)
        || path_looks_like_test_or_benchmark(path)
    {
        return 0.0;
    }
    let target_terms = target_stem_terms(query, target_hint);
    if target_terms.is_empty() {
        return 0.0;
    }
    let source_terms = stem_terms(file_stem(path.rsplit('/').next().unwrap_or(path)));
    let overlap = target_terms
        .iter()
        .filter(|target| source_terms.iter().any(|source| source == *target))
        .count();

    (overlap as f64 * 1.0).clamp(0.0, 1.2)
}

pub(in super::super) fn import_self_implementation_penalty(
    base_score: f64,
    query: &str,
    path: &str,
    target_hint: Option<&str>,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Imports || !query_looks_like_import_path(query) {
        return 0.0;
    }
    let target_stem = target_stem(query, target_hint);
    let Some(target_stem) = target_stem.as_deref() else {
        return 0.0;
    };
    let source_name = path.rsplit('/').next().unwrap_or(path);
    if file_stem(source_name).eq_ignore_ascii_case(target_stem)
        && source_file_can_implement_header(source_name)
    {
        -0.8
    } else {
        0.0
    }
}

pub(super) fn import_single_module_path_tiebreaker_bonus(
    base_score: f64,
    query: &str,
    path: &str,
    module: &str,
    target_hint: Option<&str>,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0
        || kind != CodeQueryKind::Imports
        || query_looks_like_import_path(query)
        || query_terms(query).len() != 1
        || path_looks_like_test_or_benchmark(path)
        || !import_target_mentions_query(module, target_hint, query)
    {
        return 0.0;
    }

    1.0 / path.len().max(1) as f64
}

pub(super) struct ImportSourceSignificance<'a> {
    pub(super) path: &'a str,
    pub(super) is_generated: bool,
    pub(super) module: &'a str,
    pub(super) target_hint: Option<&'a str>,
    pub(super) source_line_count: usize,
}

pub(super) fn import_source_significance_bonus(
    base_score: f64,
    query: &str,
    source: &ImportSourceSignificance<'_>,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0
        || kind != CodeQueryKind::Imports
        || source.source_line_count == 0
        || source.is_generated
        || path_looks_like_test_or_benchmark(source.path)
        || import_module_has_wildcard_binding(source.module)
        || !import_target_mentions_query(source.module, source.target_hint, query)
    {
        return 0.0;
    }

    ((source.source_line_count as f64).log2() * IMPORT_SOURCE_SIGNIFICANCE_PER_DOUBLING)
        .min(MAX_IMPORT_SOURCE_SIGNIFICANCE_BONUS)
}

fn import_module_has_wildcard_binding(module: &str) -> bool {
    let module = module.trim_end_matches([';', ' ']);
    module.contains('*') || module.ends_with("._")
}

pub(in super::super) fn import_reexport_surface_penalty(
    base_score: f64,
    query: &str,
    path: &str,
    module: &str,
    target_hint: Option<&str>,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Imports || query_looks_like_import_path(query) {
        return 0.0;
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    if !matches!(
        file_name,
        "__init__.py" | "mod.rs" | "index.js" | "index.ts"
    ) {
        return 0.0;
    }
    if import_target_mentions_query(module, target_hint, query) {
        -0.2
    } else {
        0.0
    }
}

pub(in super::super) fn import_target_symbol_bonus(
    query: &str,
    matched_symbol_name: Option<&str>,
) -> f64 {
    let Some(matched_symbol_name) = matched_symbol_name else {
        return 0.0;
    };
    let terms = query_terms(query);
    let Some(term) = terms.last() else {
        return 0.0;
    };
    if term.len() >= 3
        && matched_symbol_name
            .split_whitespace()
            .any(|name| name.eq_ignore_ascii_case(term))
    {
        2.0
    } else {
        0.0
    }
}

pub(super) fn import_same_file_usage_bonus(
    base_score: f64,
    usage_count: usize,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Imports || usage_count == 0 {
        return 0.0;
    }

    (usage_count as f64 * IMPORT_USAGE_BONUS_PER_REFERENCE).min(MAX_IMPORT_USAGE_BONUS)
}

pub(super) struct ImporterPathContext<'a> {
    pub(super) path: &'a str,
    pub(super) module: &'a str,
    pub(super) target_hint: Option<&'a str>,
    pub(super) matched_symbol_name: Option<&'a str>,
    pub(super) target_symbol_names: Option<&'a str>,
}

pub(super) fn import_importer_path_context_bonus(
    base_score: f64,
    usage_count: usize,
    query: &str,
    context: &ImporterPathContext<'_>,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || usage_count == 0 || kind != CodeQueryKind::Imports {
        return 0.0;
    }
    let importer_identity_terms = explicit_importer_identity_terms(query, context);
    if importer_identity_terms.is_empty() {
        return 0.0;
    }
    let path_terms = import_usage_identifier_terms(context.path);
    let matched_terms = importer_identity_terms
        .iter()
        .filter(|term| {
            path_terms
                .iter()
                .any(|path_term| identifier_terms_equivalent(path_term, term))
        })
        .count();

    (matched_terms as f64 * IMPORT_PATH_CONTEXT_BONUS_PER_TERM).min(MAX_IMPORT_PATH_CONTEXT_BONUS)
}

fn explicit_importer_identity_terms(query: &str, context: &ImporterPathContext<'_>) -> Vec<String> {
    let imported_identity_terms = [
        Some(context.module),
        context.target_hint,
        context.matched_symbol_name,
        context.target_symbol_names,
    ]
    .into_iter()
    .flatten()
    .flat_map(import_usage_identifier_terms)
    .collect::<Vec<_>>();

    query_local_binding_terms(query)
        .into_iter()
        .filter(|query_term| {
            !imported_identity_terms
                .iter()
                .any(|target_term| identifier_terms_equivalent(target_term, query_term))
        })
        .collect()
}

pub(super) fn import_target_directory_bonus(
    base_score: f64,
    query: &str,
    path: &str,
    target_hint: Option<&str>,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Imports || query_looks_like_import_path(query) {
        return 0.0;
    }
    let Some(target_parent) = target_hint.and_then(parent_dir) else {
        return 0.0;
    };
    if parent_dir(path).is_some_and(|parent| parent == target_parent)
        && path != target_hint.unwrap_or_default()
    {
        0.4
    } else {
        0.0
    }
}

pub(super) fn import_binding_context_bonus(
    base_score: f64,
    query: &str,
    module: &str,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Imports || query_looks_like_import_path(query) {
        return 0.0;
    }
    let Some(binding_count) = named_import_binding_count_for_query(module, query) else {
        return 0.0;
    };
    if binding_count <= 1 {
        return 0.0;
    }

    ((binding_count - 1) as f64 * IMPORT_BINDING_CONTEXT_BONUS_PER_BINDING)
        .min(MAX_IMPORT_BINDING_CONTEXT_BONUS)
}

pub(super) fn hybrid_import_sparse_query_penalty(
    base_score: f64,
    query: &str,
    path: &str,
    module: &str,
    target_hint: Option<&str>,
    _matched_symbol_name: Option<&str>,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Hybrid || query_looks_like_import_path(query) {
        return 0.0;
    }
    let terms = normalized_query_terms(query);
    if terms.len() < MIN_HYBRID_SPARSE_IMPORT_QUERY_TERMS {
        return 0.0;
    }

    let fields = [path, module, target_hint.unwrap_or_default()];
    let matched_terms = terms
        .iter()
        .filter(|term| {
            fields
                .iter()
                .any(|field| import_field_matches_query_term(field, term))
        })
        .count();
    let required_terms = terms.len().div_ceil(2);
    if matched_terms >= required_terms {
        return 0.0;
    }

    let missing_required_terms = required_terms - matched_terms;
    let penalty = (missing_required_terms as f64 * HYBRID_SPARSE_IMPORT_PENALTY_PER_TERM)
        .min(MAX_HYBRID_SPARSE_IMPORT_PENALTY)
        .min((base_score - MIN_SPARSE_IMPORT_BASE_SCORE).max(0.0));
    -penalty
}

const MIN_HYBRID_SPARSE_IMPORT_QUERY_TERMS: usize = 6;
const HYBRID_SPARSE_IMPORT_PENALTY_PER_TERM: f64 = 4.0;
const MAX_HYBRID_SPARSE_IMPORT_PENALTY: f64 = 16.0;
const MIN_SPARSE_IMPORT_BASE_SCORE: f64 = 0.5;
const MIN_IMPORT_COVERAGE_TERM_LEN: usize = 3;
const IMPORT_USAGE_BONUS_PER_REFERENCE: f64 = 0.08;
const MAX_IMPORT_USAGE_BONUS: f64 = 0.8;
const IMPORT_SOURCE_SIGNIFICANCE_PER_DOUBLING: f64 = 0.04;
const MAX_IMPORT_SOURCE_SIGNIFICANCE_BONUS: f64 = 0.5;
const IMPORT_PATH_CONTEXT_BONUS_PER_TERM: f64 = 0.65;
const MAX_IMPORT_PATH_CONTEXT_BONUS: f64 = 1.3;
const IMPORT_BINDING_CONTEXT_BONUS_PER_BINDING: f64 = 0.25;
const MAX_IMPORT_BINDING_CONTEXT_BONUS: f64 = 1.0;

fn normalized_query_terms(query: &str) -> Vec<String> {
    let mut terms = query_terms(query)
        .into_iter()
        .filter(|term| term.len() >= MIN_IMPORT_COVERAGE_TERM_LEN)
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();

    terms
}

fn import_field_matches_query_term(field: &str, term: &str) -> bool {
    let lower = field.to_ascii_lowercase();
    if lower.contains(term) {
        return true;
    }

    identifier_tokens(field).any(|candidate| {
        identifier_terms_equivalent(candidate, term)
            || candidate
                .split('_')
                .filter(|part| !part.is_empty())
                .any(|part| identifier_terms_equivalent(part, term))
            || camel_case_terms(candidate)
                .iter()
                .any(|part| identifier_terms_equivalent(part, term))
    })
}

fn query_looks_like_bare_import(query: &str) -> bool {
    let query = query.trim();
    query.starts_with("import ")
        && !query.contains(" from ")
        && !query.contains('{')
        && quoted_import_specifier(query).is_some()
}

fn import_expression_or_side_effect_bonus(module: &str) -> f64 {
    if module.contains("import(")
        || module.starts_with("import \"")
        || module.starts_with("import '")
    {
        2.25
    } else {
        0.0
    }
}

fn quoted_import_specifier(value: &str) -> Option<&str> {
    for quote in ['"', '\''] {
        let Some(start) = value.find(quote) else {
            continue;
        };
        let after_start = value.get(start + quote.len_utf8()..)?;
        let Some(end) = after_start.find(quote) else {
            continue;
        };
        let specifier = after_start.get(..end)?;
        if !specifier.trim().is_empty() {
            return Some(specifier);
        }
    }

    None
}

#[cfg(test)]
#[path = "scoring_tests.rs"]
mod tests;
