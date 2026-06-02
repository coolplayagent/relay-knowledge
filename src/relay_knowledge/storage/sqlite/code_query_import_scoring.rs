use crate::domain::CodeQueryKind;

use super::{
    code_query_identifiers::identifier_terms_equivalent,
    code_query_path_ranking::path_looks_like_test_or_benchmark,
};

pub(super) fn import_line_priority(base_score: f64, line_start: u32, query: &str) -> f64 {
    if base_score <= 0.0 || !query_looks_like_import_path(query) {
        return 0.0;
    }

    1.0 / f64::from(line_start.clamp(1, 1_000))
}

pub(super) fn import_surface_bonus(base_score: f64, path: &str, kind: CodeQueryKind) -> f64 {
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

pub(super) fn import_public_dependency_surface_bonus(
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

pub(super) fn import_source_path_query_overlap_bonus(
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

pub(super) fn import_self_implementation_penalty(
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

pub(super) fn import_reexport_surface_penalty(
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

pub(super) fn import_target_symbol_bonus(query: &str, matched_symbol_name: Option<&str>) -> f64 {
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
    if base_score <= 0.0 || kind != CodeQueryKind::Imports || usage_count <= 1 {
        return 0.0;
    }

    ((usage_count - 1) as f64 * IMPORT_USAGE_BONUS_PER_REFERENCE).min(MAX_IMPORT_USAGE_BONUS)
}

pub(super) fn import_importer_path_context_bonus(
    base_score: f64,
    usage_count: usize,
    query: &str,
    path: &str,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0
        || usage_count == 0
        || kind != CodeQueryKind::Imports
        || !query_looks_like_import_path(query)
    {
        return 0.0;
    }
    let last_segment = query.rsplit(['/', '\\']).next().unwrap_or(query);
    let target_stem = last_segment
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(last_segment);
    let target_terms = import_usage_identifier_terms(target_stem);
    if target_terms.is_empty() {
        return 0.0;
    }
    let path_terms = import_usage_identifier_terms(path);
    let matched_terms = target_terms
        .iter()
        .filter(|term| {
            path_terms
                .iter()
                .any(|path_term| identifier_terms_equivalent(path_term, term))
        })
        .count();

    (matched_terms as f64 * IMPORT_PATH_CONTEXT_BONUS_PER_TERM).min(MAX_IMPORT_PATH_CONTEXT_BONUS)
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

pub(super) fn query_looks_like_import_path(query: &str) -> bool {
    let trimmed = query.trim();
    trimmed.contains('/') || trimmed.contains('\\') || query_contains_file_extension(trimmed)
}

fn query_contains_file_extension(query: &str) -> bool {
    query.split_whitespace().any(|term| {
        let term = term.trim_matches(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
        });
        let Some((stem, extension)) = term.rsplit_once('.') else {
            return false;
        };
        !stem.is_empty() && file_extension_is_path_like(extension)
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
        0.65
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

fn file_extension_is_path_like(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "c" | "cc"
            | "cpp"
            | "cs"
            | "go"
            | "gradle"
            | "h"
            | "hh"
            | "hpp"
            | "hxx"
            | "java"
            | "js"
            | "json"
            | "jsx"
            | "kt"
            | "md"
            | "php"
            | "py"
            | "rb"
            | "rs"
            | "scala"
            | "sh"
            | "swift"
            | "ts"
            | "tsx"
            | "txt"
            | "xml"
            | "yaml"
            | "yml"
    )
}

fn parent_dir(path: &str) -> Option<&str> {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
}

fn path_has_header_extension(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .and_then(|file_name| file_name.rsplit_once('.').map(|(_, extension)| extension))
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "h" | "hh" | "hpp" | "hxx" | "inc" | "ipp"
            )
        })
}

fn named_import_binding_count_for_query(module: &str, query: &str) -> Option<usize> {
    let (start, end) = named_import_bounds(module)?;
    let query_terms = query_terms(query);
    let mut binding_count = 0;
    let mut query_is_bound = false;
    for binding in module[start + 1..end].split(',') {
        let binding = binding.trim().trim_start_matches("type ").trim();
        if binding.is_empty() {
            continue;
        }
        binding_count += 1;
        if import_binding_name(binding)
            .is_some_and(|binding_name| import_binding_matches_query(binding_name, &query_terms))
        {
            query_is_bound = true;
        }
    }

    query_is_bound.then_some(binding_count)
}

pub(super) fn named_import_binding_terms(module: &str) -> Vec<String> {
    let Some((start, end)) = named_import_bounds(module) else {
        return Vec::new();
    };
    let mut terms = Vec::new();
    for binding in module[start + 1..end].split(',') {
        let Some(binding_names) = import_binding_names(binding) else {
            continue;
        };
        for term in import_usage_identifier_terms(binding_names.local) {
            if !terms.contains(&term) {
                terms.push(term);
            }
        }
    }

    terms
}

pub(super) fn named_import_binding_terms_for_query(
    module: &str,
    query: &str,
    matched_symbol_names: Option<&str>,
) -> Vec<String> {
    let Some((start, end)) = named_import_bounds(module) else {
        return Vec::new();
    };
    let requested_terms = query_terms(query);
    let matched_terms = matched_symbol_names.map(query_terms).unwrap_or_default();
    let mut terms = Vec::new();
    for binding in module[start + 1..end].split(',') {
        let Some(binding_names) = import_binding_names(binding) else {
            continue;
        };
        if !import_binding_matches_terms(binding_names, &requested_terms)
            && !import_binding_matches_terms(binding_names, &matched_terms)
        {
            continue;
        }
        for term in import_usage_identifier_terms(binding_names.local) {
            if !terms.contains(&term) {
                terms.push(term);
            }
        }
    }

    terms
}

fn named_import_bounds(module: &str) -> Option<(usize, usize)> {
    let start = module.find('{')?;
    let end = module[start + 1..].find('}')? + start + 1;
    (end > start).then_some((start, end))
}

#[derive(Clone, Copy)]
struct ImportBindingNames<'a> {
    imported: &'a str,
    local: &'a str,
}

fn import_binding_name(binding: &str) -> Option<&str> {
    import_binding_names(binding).map(|names| names.local)
}

fn import_binding_names(binding: &str) -> Option<ImportBindingNames<'_>> {
    let binding = binding.trim().trim_start_matches("type ").trim();
    let imported = binding
        .split_whitespace()
        .next()?
        .trim_matches(|character: char| !(character.is_ascii_alphanumeric() || character == '_'));
    let binding_name = binding
        .split_whitespace()
        .last()?
        .trim_matches(|character: char| !(character.is_ascii_alphanumeric() || character == '_'));
    (!imported.is_empty() && !binding_name.is_empty()).then_some(ImportBindingNames {
        imported,
        local: binding_name,
    })
}

fn import_binding_matches_query(binding_name: &str, query_terms: &[String]) -> bool {
    query_terms
        .iter()
        .any(|term| identifier_terms_equivalent(binding_name, term))
}

fn import_binding_matches_terms(binding_names: ImportBindingNames<'_>, terms: &[String]) -> bool {
    import_binding_matches_query(binding_names.imported, terms)
        || import_binding_matches_query(binding_names.local, terms)
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

fn target_stem_terms(query: &str, target_hint: Option<&str>) -> Vec<String> {
    target_stem(query, target_hint)
        .map(|stem| stem_terms(&stem))
        .unwrap_or_default()
}

fn target_stem(query: &str, target_hint: Option<&str>) -> Option<String> {
    let target = target_hint
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .unwrap_or(query);
    let file_name = target
        .trim_matches(|character: char| {
            !(character.is_ascii_alphanumeric()
                || matches!(character, '_' | '-' | '.' | '/' | '\\'))
        })
        .rsplit(['/', '\\'])
        .next()?;
    let stem = file_stem(file_name);
    (!stem.is_empty()).then(|| stem.to_ascii_lowercase())
}

fn file_stem(file_name: &str) -> &str {
    file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem)
}

fn stem_terms(stem: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for token in stem
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        for term in camel_case_terms(token) {
            if term.len() >= MIN_IMPORT_COVERAGE_TERM_LEN {
                terms.push(term);
            }
        }
    }
    terms.sort();
    terms.dedup();
    terms
}

fn source_file_can_implement_header(file_name: &str) -> bool {
    file_name
        .rsplit_once('.')
        .is_some_and(|(_, extension)| matches!(extension, "c" | "cc" | "cpp" | "cxx" | "m" | "mm"))
}

fn import_target_mentions_query(module: &str, target_hint: Option<&str>, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return false;
    }
    let query = query.to_ascii_lowercase();
    [module, target_hint.unwrap_or_default()]
        .into_iter()
        .map(str::to_ascii_lowercase)
        .any(|field| field.trim() == query || field.contains(&query))
}

pub(super) fn import_usage_identifier_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for token in identifier_tokens(value) {
        if import_usage_term_is_specific(token) {
            push_import_usage_term(&mut terms, token);
        }
        for part in token.split('_').filter(|part| !part.is_empty()) {
            if import_usage_term_is_specific(part) {
                push_import_usage_term(&mut terms, part);
            }
        }
        for term in camel_case_terms(token) {
            if import_usage_term_is_specific(&term) {
                push_import_usage_term(&mut terms, &term);
            }
        }
    }

    terms
}

fn push_import_usage_term(terms: &mut Vec<String>, term: &str) {
    let term = term.to_ascii_lowercase();
    if !terms.contains(&term) {
        terms.push(term);
    }
}

fn import_usage_term_is_specific(term: &str) -> bool {
    term.len() >= 5 || term.contains('_') || term_has_case_boundary(term)
}

fn term_has_case_boundary(value: &str) -> bool {
    let mut previous: Option<char> = None;
    for character in value.chars() {
        if character.is_ascii_uppercase()
            && previous.is_some_and(|previous| previous.is_ascii_lowercase())
        {
            return true;
        }
        previous = Some(character);
    }

    false
}

fn identifier_tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
}

fn camel_case_terms(token: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut start = 0;
    let mut previous: Option<char> = None;
    let chars = token.char_indices().collect::<Vec<_>>();
    for (index, (byte_index, character)) in chars.iter().enumerate() {
        let next = chars.get(index + 1).map(|(_, next)| *next);
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if *byte_index > start && starts_upper_word {
            terms.push(token[start..*byte_index].to_ascii_lowercase());
            start = *byte_index;
        }
        previous = Some(*character);
    }
    if start < token.len() {
        terms.push(token[start..].to_ascii_lowercase());
    }

    terms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensionless_path_import_context_bonus_uses_only_target_basename() {
        let services_only_bonus = import_importer_path_context_bonus(
            1.0,
            1,
            "services/cache",
            "src/services/bootstrap.ts",
            CodeQueryKind::Imports,
        );
        let basename_bonus = import_importer_path_context_bonus(
            1.0,
            1,
            "services/cache",
            "src/cache/cache_consumer.ts",
            CodeQueryKind::Imports,
        );

        assert_eq!(services_only_bonus, 0.0);
        assert!(basename_bonus > services_only_bonus);
    }

    #[test]
    fn import_line_priority_only_applies_to_path_like_queries() {
        assert_eq!(import_line_priority(3.0, 1, "ProviderShared"), 0.0);
        assert_eq!(
            import_line_priority(3.0, 1, "org.springframework.util.ObjectUtils"),
            0.0
        );
        assert!(import_line_priority(3.0, 10, "linux/debugfs.h") > 0.0);
        assert!(import_line_priority(3.0, 10, "./redaction") > 0.0);
        assert!(import_line_priority(3.0, 10, "shared.ts") > 0.0);
        assert_eq!(import_line_priority(0.0, 1, "linux/debugfs.h"), 0.0);
    }

    #[test]
    fn import_statement_shape_bonus_prefers_direct_imports_for_path_queries() {
        assert_eq!(
            import_statement_shape_bonus(
                2.0,
                "./protocol",
                "export type { StreamEnvelope } from \"./protocol\";",
                CodeQueryKind::Imports,
            ),
            0.0
        );
        assert_eq!(
            import_statement_shape_bonus(
                2.0,
                "./protocol",
                "import type { StreamEnvelope } from \"./protocol\";",
                CodeQueryKind::Imports,
            ),
            0.25
        );
    }

    #[test]
    fn import_statement_shape_bonus_matches_bare_import_queries_to_dynamic_imports() {
        assert_eq!(
            import_statement_shape_bonus(
                2.0,
                "import \"./protocol\"",
                "import { sendEnvelope } from \"./protocol\";",
                CodeQueryKind::Imports,
            ),
            0.0
        );
        assert_eq!(
            import_statement_shape_bonus(
                2.0,
                "import \"./protocol\"",
                "await import(\"./protocol\")",
                CodeQueryKind::Imports,
            ),
            0.65
        );
    }

    #[test]
    fn import_source_path_overlap_bonus_uses_robust_test_path_detection() {
        assert_eq!(
            import_source_path_query_overlap_bonus(
                3.0,
                "foo.h",
                "src/foo_test.cc",
                Some("include/foo.h"),
                CodeQueryKind::Imports,
            ),
            0.0
        );
        assert!(
            import_source_path_query_overlap_bonus(
                3.0,
                "foo.h",
                "src/foo.cc",
                Some("include/foo.h"),
                CodeQueryKind::Imports,
            ) > 0.0
        );
    }

    #[test]
    fn path_import_context_bonus_matches_target_stem_terms_to_importer_path() {
        assert_eq!(
            import_importer_path_context_bonus(
                3.0,
                2,
                "store/cache.hpp",
                "src/storage/cache_consumer.cc",
                CodeQueryKind::Imports,
            ),
            0.65
        );
        assert_eq!(
            import_importer_path_context_bonus(
                3.0,
                2,
                "store/cache.hpp",
                "src/storage/consumer.cc",
                CodeQueryKind::Imports,
            ),
            0.0
        );
        assert_eq!(
            import_importer_path_context_bonus(
                0.0,
                2,
                "store/cache.hpp",
                "src/storage/cache_consumer.cc",
                CodeQueryKind::Imports,
            ),
            0.0
        );
        assert_eq!(
            import_importer_path_context_bonus(
                3.0,
                0,
                "store/cache.hpp",
                "src/storage/cache_consumer.cc",
                CodeQueryKind::Imports,
            ),
            0.0
        );
    }

    #[test]
    fn import_binding_terms_keep_specific_local_names() {
        assert_eq!(
            named_import_binding_terms(
                "import { JsonObject, optionalArray, type ProviderShared as Shared } from './shared'",
            ),
            vec![
                "jsonobject".to_owned(),
                "object".to_owned(),
                "optionalarray".to_owned(),
                "optional".to_owned(),
                "array".to_owned(),
                "shared".to_owned()
            ]
        );
    }

    #[test]
    fn import_binding_terms_for_query_keep_only_matching_binding() {
        assert_eq!(
            named_import_binding_terms_for_query(
                "import { Target as LocalTarget, VeryCommon } from './module'",
                "Target",
                Some("Target"),
            ),
            vec![
                "localtarget".to_owned(),
                "local".to_owned(),
                "target".to_owned()
            ]
        );
    }

    #[test]
    fn hybrid_sparse_import_penalty_only_applies_to_long_concept_queries() {
        assert_eq!(
            hybrid_import_sparse_query_penalty(
                12.0,
                "linux/debugfs.h",
                "mm/cma_debug.c",
                "#include <linux/debugfs.h>",
                Some("include/linux/debugfs.h"),
                None,
                CodeQueryKind::Hybrid,
            ),
            0.0
        );
        assert_eq!(
            hybrid_import_sparse_query_penalty(
                12.0,
                "OpenAI Chat protocol SSE tool calls",
                "packages/llm/src/providers/openai.ts",
                "import * as OpenAIChat from \"../protocols/openai-chat\"",
                Some("packages/llm/src/protocols/openai-chat.ts"),
                None,
                CodeQueryKind::Imports,
            ),
            0.0
        );
        assert_eq!(
            hybrid_import_sparse_query_penalty(
                12.0,
                "OpenAI Chat protocol SSE tool",
                "packages/llm/src/providers/openai.ts",
                "import * as OpenAIChat from \"../protocols/openai-chat\"",
                Some("packages/llm/src/protocols/openai-chat.ts"),
                None,
                CodeQueryKind::Hybrid,
            ),
            0.0
        );
    }

    #[test]
    fn hybrid_sparse_import_penalty_demotes_low_coverage_import_edges() {
        let penalty = hybrid_import_sparse_query_penalty(
            18.0,
            "OpenAI Chat protocol SSE tool calls lifecycle finish events route transport",
            "packages/llm/src/providers/openai.ts",
            "import * as OpenAIChat from \"../protocols/openai-chat\"",
            Some("packages/llm/src/protocols/openai-chat.ts"),
            None,
            CodeQueryKind::Hybrid,
        );

        assert!(penalty <= -11.0, "penalty was {penalty}");
    }

    #[test]
    fn hybrid_sparse_import_penalty_preserves_imports_covering_query_terms() {
        let penalty = hybrid_import_sparse_query_penalty(
            18.0,
            "OpenAI Chat protocol SSE tool calls lifecycle finish events route transport",
            "packages/llm/src/route/transport/openai-chat.ts",
            "import { ToolStream, Lifecycle, finishEvents, route, transport } from \"../protocols/openai-chat\"",
            Some("packages/llm/src/protocols/openai-chat.ts"),
            Some("ToolStream Lifecycle finishEvents OpenAIChatProtocol"),
            CodeQueryKind::Hybrid,
        );

        assert_eq!(penalty, 0.0);
    }
}
