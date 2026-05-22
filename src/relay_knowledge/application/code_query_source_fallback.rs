use std::collections::{BTreeMap, BTreeSet};

use crate::{
    code::{
        SourceDeclarationMatch, SourceGrepKind, SourceGrepMatch, SourceGrepOutcome,
        SourceGrepRequest, simple_source_identifier, source_line_defines_identity,
    },
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest,
    },
};

const MAX_DEFINITION_SOURCE_CANDIDATE_PATHS: usize = 8;
const MAX_IMPORT_SOURCE_CANDIDATE_PATHS: usize = 32;
const EXTERNAL_IMPORT_GREP_DIAGNOSTIC: &str = "external dependency import is not indexed in the code graph; searched current repository source with internal grep fallback";
const REFERENCE_SOURCE_DECLARATION_PENALTY: f64 = -1.9;

pub(super) struct CodeGrepFallbackPlan {
    pub(super) commit: String,
    pub(super) query: String,
    pub(super) paths: Vec<String>,
    pub(super) path_filters: Vec<String>,
    pub(super) language_filters: Vec<String>,
    pub(super) limit: usize,
    pub(super) kind: SourceGrepKind,
    pub(super) identity: Option<String>,
    needs_scope_paths: bool,
}

impl CodeGrepFallbackPlan {
    pub(super) fn needs_scope_paths(&self) -> bool {
        self.needs_scope_paths
    }

    pub(super) fn with_scope_paths(mut self, scope_paths: Vec<String>) -> Self {
        if self.needs_scope_paths {
            self.paths = scope_paths;
            self.needs_scope_paths = false;
        }
        self
    }

    pub(super) fn source_request(&self) -> SourceGrepRequest {
        SourceGrepRequest {
            query: self.query.clone(),
            paths: self.paths.clone(),
            path_filters: self.path_filters.clone(),
            language_filters: self.language_filters.clone(),
            limit: self.limit,
            kind: self.kind,
        }
    }
}

pub(super) fn plan_code_grep_fallback(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    results: &[CodeRetrievalHit],
) -> Option<CodeGrepFallbackPlan> {
    let commit = status.last_indexed_commit.clone()?;
    let path_filters = merged_filters(&status.path_filters, &request.repository.path_filters);
    let language_filters = merged_filters(
        &status.language_filters,
        &request.repository.language_filters,
    );
    match request.code_query_kind {
        CodeQueryKind::Definition => {
            let identity = definition_identity(&request.query)?;
            if results_define_identity(results, &identity)
                && results.iter().any(|hit| {
                    hit.retrieval_layers
                        .contains(&CodeRetrievalLayer::Definition)
                })
            {
                return None;
            }
            let paths = definition_source_candidate_paths(request, results, &identity);
            Some(CodeGrepFallbackPlan {
                commit,
                query: identity.clone(),
                needs_scope_paths: paths.is_empty(),
                paths,
                path_filters,
                language_filters,
                limit: request.limit,
                kind: SourceGrepKind::Definition,
                identity: Some(identity),
            })
        }
        CodeQueryKind::References => {
            let identity = source_grep_identity(&request.query)?;
            if results.iter().any(|hit| {
                hit.retrieval_layers
                    .contains(&CodeRetrievalLayer::Reference)
            }) {
                return None;
            }
            Some(CodeGrepFallbackPlan {
                commit,
                query: identity,
                paths: Vec::new(),
                path_filters,
                language_filters,
                limit: request.limit,
                kind: SourceGrepKind::References,
                identity: None,
                needs_scope_paths: true,
            })
        }
        CodeQueryKind::Imports => {
            let query = import_grep_query(results)?;
            let paths = import_grep_candidate_paths(results, &query);
            let needs_scope_paths = paths.is_empty();
            Some(CodeGrepFallbackPlan {
                commit,
                query,
                paths,
                path_filters,
                language_filters,
                limit: request.limit,
                kind: SourceGrepKind::Imports,
                identity: None,
                needs_scope_paths,
            })
        }
        CodeQueryKind::Hybrid if results.len() < request.limit => {
            let identity = source_grep_identity(&request.query)?;
            Some(CodeGrepFallbackPlan {
                commit,
                query: identity,
                paths: Vec::new(),
                path_filters,
                language_filters,
                limit: request.limit.saturating_sub(results.len()).max(1),
                kind: SourceGrepKind::Hybrid,
                identity: None,
                needs_scope_paths: true,
            })
        }
        _ => None,
    }
}

pub(super) fn append_code_grep_fallback(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    results: &mut Vec<CodeRetrievalHit>,
    plan: &CodeGrepFallbackPlan,
    outcome: SourceGrepOutcome,
) -> Option<String> {
    if outcome.matches.is_empty() {
        return fallback_diagnostic(plan, outcome.degraded_reason);
    }
    let score_bounds = ScoreBounds::from_results(results);
    let base_fallback_score = grep_score(plan.kind, score_bounds);
    let metadata = path_metadata(results);
    for matched in outcome.matches {
        let fallback_score = source_grep_match_score(plan, &matched, base_fallback_score);
        if let Some(existing) = results.iter_mut().find(|hit| {
            hit.path == matched.path
                && hit.line_range.start == matched.line_range.start
                && hit.excerpt == matched.excerpt
        }) {
            add_code_grep_layers(existing, plan.kind);
            existing.score = existing.score.max(fallback_score);
            continue;
        }
        let path_metadata = metadata.get(&matched.path);
        results.push(code_grep_hit(
            status,
            &matched,
            path_metadata,
            plan.kind,
            fallback_score,
            outcome.degraded_reason.clone(),
        ));
    }
    dedupe_sort_truncate(results, request.limit);

    fallback_diagnostic(plan, outcome.degraded_reason)
}

fn add_code_grep_layers(hit: &mut CodeRetrievalHit, kind: SourceGrepKind) {
    if kind == SourceGrepKind::Definition {
        add_retrieval_layer(hit, CodeRetrievalLayer::Definition);
    }
    add_retrieval_layer(hit, CodeRetrievalLayer::Lexical);
    add_retrieval_layer(hit, CodeRetrievalLayer::TextFallback);
}

pub(super) fn append_definition_source_fallback(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    results: &mut Vec<CodeRetrievalHit>,
    declarations: Vec<SourceDeclarationMatch>,
) {
    if declarations.is_empty() {
        return;
    }
    let best_score = results.first().map_or(0.0, |hit| hit.score);
    let metadata = path_metadata(results);
    for declaration in declarations {
        if let Some(existing) = results.iter_mut().find(|hit| {
            hit.path == declaration.path
                && hit.line_range.start == declaration.line_range.start
                && hit.excerpt == declaration.excerpt
        }) {
            add_retrieval_layer(existing, CodeRetrievalLayer::Definition);
            add_retrieval_layer(existing, CodeRetrievalLayer::Lexical);
            add_retrieval_layer(existing, CodeRetrievalLayer::TextFallback);
            existing.score = existing.score.max(best_score + 4.0);
            continue;
        }
        let path_metadata = metadata.get(&declaration.path);
        results.push(CodeRetrievalHit {
            repository_id: status.repository_id.clone(),
            scope_id: status.last_indexed_scope_id.clone().unwrap_or_default(),
            resolved_commit_sha: status.last_indexed_commit.clone().unwrap_or_default(),
            tree_hash: status.tree_hash.clone().unwrap_or_default(),
            path: declaration.path,
            language_id: path_metadata
                .map(|metadata| metadata.language_id.clone())
                .unwrap_or_default(),
            byte_range: declaration.byte_range,
            line_range: declaration.line_range,
            symbol_snapshot_id: path_metadata
                .and_then(|metadata| metadata.symbol_snapshot_id.clone()),
            canonical_symbol_id: path_metadata
                .and_then(|metadata| metadata.canonical_symbol_id.clone()),
            file_id: path_metadata.and_then(|metadata| metadata.file_id.clone()),
            retrieval_layers: vec![
                CodeRetrievalLayer::Definition,
                CodeRetrievalLayer::Lexical,
                CodeRetrievalLayer::TextFallback,
            ],
            index_versions: vec![format!(
                "code:{}:{}",
                status
                    .last_indexed_scope_id
                    .as_deref()
                    .unwrap_or("unscoped"),
                status.tree_hash.as_deref().unwrap_or("unindexed")
            )],
            stale: status.stale,
            degraded_reason: status.degraded_reason.clone(),
            edge_kind: None,
            edge_resolution_state: None,
            edge_target_hint: None,
            edge_confidence_basis_points: None,
            edge_confidence_tier: None,
            score: best_score + 4.0,
            excerpt: declaration.excerpt,
        });
    }
    dedupe_sort_truncate(results, request.limit);
}

fn add_retrieval_layer(hit: &mut CodeRetrievalHit, layer: CodeRetrievalLayer) {
    if !hit.retrieval_layers.contains(&layer) {
        hit.retrieval_layers.push(layer);
    }
}

fn code_grep_hit(
    status: &CodeRepositoryStatus,
    matched: &SourceGrepMatch,
    path_metadata: Option<&HitPathMetadata>,
    kind: SourceGrepKind,
    score: f64,
    degraded_reason: Option<String>,
) -> CodeRetrievalHit {
    let mut layers = vec![
        CodeRetrievalLayer::Lexical,
        CodeRetrievalLayer::TextFallback,
    ];
    if kind == SourceGrepKind::Definition {
        layers.insert(0, CodeRetrievalLayer::Definition);
    }

    CodeRetrievalHit {
        repository_id: status.repository_id.clone(),
        scope_id: status.last_indexed_scope_id.clone().unwrap_or_default(),
        resolved_commit_sha: status.last_indexed_commit.clone().unwrap_or_default(),
        tree_hash: status.tree_hash.clone().unwrap_or_default(),
        path: matched.path.clone(),
        language_id: path_metadata
            .map(|metadata| metadata.language_id.clone())
            .unwrap_or_else(|| matched.language_id.clone()),
        byte_range: matched.byte_range.clone(),
        line_range: matched.line_range.clone(),
        symbol_snapshot_id: path_metadata.and_then(|metadata| metadata.symbol_snapshot_id.clone()),
        canonical_symbol_id: path_metadata
            .and_then(|metadata| metadata.canonical_symbol_id.clone()),
        file_id: path_metadata.and_then(|metadata| metadata.file_id.clone()),
        retrieval_layers: layers,
        index_versions: vec![format!(
            "code:{}:{}",
            status
                .last_indexed_scope_id
                .as_deref()
                .unwrap_or("unscoped"),
            status.tree_hash.as_deref().unwrap_or("unindexed")
        )],
        stale: status.stale,
        degraded_reason: degraded_reason.or_else(|| status.degraded_reason.clone()),
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score,
        excerpt: matched.excerpt.clone(),
    }
}

#[derive(Clone, Copy)]
struct ScoreBounds {
    best: Option<f64>,
    lowest: Option<f64>,
}

impl ScoreBounds {
    fn from_results(results: &[CodeRetrievalHit]) -> Self {
        let mut bounds = Self {
            best: None,
            lowest: None,
        };
        for hit in results {
            bounds.best = Some(bounds.best.map_or(hit.score, |best| best.max(hit.score)));
            bounds.lowest = Some(
                bounds
                    .lowest
                    .map_or(hit.score, |lowest| lowest.min(hit.score)),
            );
        }

        bounds
    }
}

fn grep_score(kind: SourceGrepKind, score_bounds: ScoreBounds) -> f64 {
    match kind {
        SourceGrepKind::Definition => score_bounds.best.unwrap_or(0.0) + 3.5,
        SourceGrepKind::References => score_bounds.best.unwrap_or(0.0) + 2.0,
        SourceGrepKind::Imports | SourceGrepKind::Hybrid => {
            score_bounds.lowest.map(|score| score - 0.25).unwrap_or(1.0)
        }
    }
}

fn fallback_diagnostic(
    plan: &CodeGrepFallbackPlan,
    degraded_reason: Option<String>,
) -> Option<String> {
    let Some(reason) = degraded_reason else {
        return (plan.kind == SourceGrepKind::Imports)
            .then(|| EXTERNAL_IMPORT_GREP_DIAGNOSTIC.to_owned());
    };
    if plan.kind == SourceGrepKind::Imports {
        Some(format!("{EXTERNAL_IMPORT_GREP_DIAGNOSTIC}; {reason}"))
    } else {
        Some(reason)
    }
}

fn source_grep_match_score(
    plan: &CodeGrepFallbackPlan,
    matched: &SourceGrepMatch,
    base_score: f64,
) -> f64 {
    let adjustment = match plan.kind {
        SourceGrepKind::References => {
            reference_source_grep_score_adjustment(&plan.query, &matched.excerpt)
        }
        SourceGrepKind::Definition | SourceGrepKind::Hybrid | SourceGrepKind::Imports => 0.0,
    };

    (base_score + adjustment).max(0.0)
}

fn reference_source_grep_score_adjustment(identity: &str, excerpt: &str) -> f64 {
    if !simple_source_identifier(identity) {
        return 0.0;
    }
    let line = excerpt.trim();
    if line.is_empty() || line.starts_with("//") || line.starts_with('*') {
        return 0.0;
    }

    if source_reference_line_declares_identity(line, identity) {
        REFERENCE_SOURCE_DECLARATION_PENALTY
    } else {
        0.0
    }
}

fn source_reference_line_declares_identity(line: &str, identity: &str) -> bool {
    if source_line_defines_identity(line, identity) {
        return true;
    }

    source_identifier_ranges(line, identity).any(|(start, end)| {
        let before = line.get(..start).unwrap_or_default().trim_end();
        let after = line.get(end..).unwrap_or_default().trim_start();
        if before.ends_with('.') || before.ends_with("->") || identifier_is_assignment_value(before)
        {
            return false;
        }
        if after.starts_with('[') && array_declarator_has_initializer(after) {
            return true;
        }

        declaration_prefix_before_identity(before)
            && before.split_whitespace().last() != Some(identity)
    })
}

fn source_identifier_ranges<'a>(
    line: &'a str,
    identity: &'a str,
) -> impl Iterator<Item = (usize, usize)> + 'a {
    line.match_indices(identity).filter_map(|(start, _)| {
        let end = start + identity.len();
        let has_start_boundary = line.get(..start).is_some_and(|prefix| {
            prefix
                .chars()
                .next_back()
                .is_none_or(|character| !source_identifier_char(character))
        });
        let has_end_boundary = line.get(end..).is_some_and(|suffix| {
            suffix
                .chars()
                .next()
                .is_none_or(|character| !source_identifier_char(character))
        });
        (has_start_boundary && has_end_boundary).then_some((start, end))
    })
}

fn declaration_prefix_before_identity(before: &str) -> bool {
    let mut tokens = before.split_whitespace();
    let Some(first_token) = tokens.next() else {
        return false;
    };
    if statement_prefix_token(first_token) {
        return false;
    }
    let token_count = before.split_whitespace().count();
    token_count >= 1
        && before
            .chars()
            .all(|character| !matches!(character, '=' | '+' | '-' | '*' | '/' | '%' | '?'))
}

fn statement_prefix_token(token: &str) -> bool {
    matches!(
        token.trim_matches(|character: char| !source_identifier_char(character)),
        "return"
            | "if"
            | "for"
            | "while"
            | "switch"
            | "case"
            | "sizeof"
            | "typeof"
            | "alignof"
            | "offsetof"
            | "throw"
            | "yield"
            | "await"
    )
}

fn array_declarator_has_initializer(after: &str) -> bool {
    let Some(equals_index) = after.find('=') else {
        return false;
    };
    !after
        .get(..equals_index)
        .is_some_and(|prefix| prefix.contains(')'))
}

fn identifier_is_assignment_value(before: &str) -> bool {
    before
        .chars()
        .rev()
        .find(|character| !character.is_whitespace())
        .is_some_and(|character| character == '=')
}

fn source_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn definition_source_candidate_paths(
    request: &CodeRetrievalRequest,
    results: &[CodeRetrievalHit],
    identity: &str,
) -> Vec<String> {
    let mut paths = Vec::new();
    for hit in results {
        if hit_mentions_identity(hit, identity) {
            push_candidate_path(&mut paths, &hit.path);
        }
    }
    for path in &request.repository.path_filters {
        if exact_file_filter(path) {
            push_candidate_path(&mut paths, path);
        }
    }
    paths.truncate(MAX_DEFINITION_SOURCE_CANDIDATE_PATHS);

    paths
}

fn hit_mentions_identity(hit: &CodeRetrievalHit, identity: &str) -> bool {
    hit.excerpt.contains(identity)
        || hit
            .canonical_symbol_id
            .as_deref()
            .is_some_and(|symbol_id| symbol_id.contains(identity))
}

fn push_candidate_path(paths: &mut Vec<String>, path: &str) {
    let normalized = normalize_filter_path(path);
    if !normalized.is_empty() && !paths.iter().any(|existing| existing == normalized) {
        paths.push(normalized.to_owned());
    }
}

fn exact_file_filter(path: &str) -> bool {
    let path = normalize_filter_path(path);
    !path.is_empty()
        && path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'))
        && !path.ends_with('/')
}

fn normalize_filter_path(path: &str) -> &str {
    let mut path = path.trim_end_matches(['/', '\\']);
    while let Some(stripped) = path.strip_prefix("./") {
        path = stripped;
    }

    path
}

fn results_define_identity(results: &[CodeRetrievalHit], identity: &str) -> bool {
    results.iter().any(|hit| {
        hit.excerpt
            .lines()
            .map(str::trim)
            .any(|line| source_line_defines_identity(line, identity))
    })
}

fn definition_identity(query: &str) -> Option<String> {
    for raw_token in query.split_whitespace().map(str::trim) {
        if raw_token.contains('/') || raw_token.contains('\\') {
            continue;
        }
        let terms = raw_token
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();
        if let Some(term) = terms.last().filter(|term| simple_source_identifier(term)) {
            return Some((*term).to_owned());
        }
    }

    None
}

fn source_grep_identity(query: &str) -> Option<String> {
    let identity = definition_identity(query)?;
    (query.split_whitespace().count() == 1).then_some(identity)
}

fn import_grep_query(results: &[CodeRetrievalHit]) -> Option<String> {
    results
        .iter()
        .find_map(unindexed_external_import_specifier)
        .and_then(|specifier| {
            if specifier.len() <= 128 {
                Some(specifier)
            } else {
                definition_identity(&specifier)
            }
        })
}

fn import_grep_candidate_paths(results: &[CodeRetrievalHit], specifier: &str) -> Vec<String> {
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

fn quoted_import_specifier(value: &str) -> Option<&str> {
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

fn local_import_specifier(specifier: &str) -> bool {
    let specifier = specifier.trim();
    specifier.starts_with('.')
        || specifier.starts_with('/')
        || matches!(specifier, "crate" | "self" | "super")
        || specifier.starts_with("crate::")
        || specifier.starts_with("self::")
        || specifier.starts_with("super::")
}

fn merged_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for value in left.iter().chain(right.iter()) {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }

    merged
}

struct HitPathMetadata {
    language_id: String,
    symbol_snapshot_id: Option<String>,
    canonical_symbol_id: Option<String>,
    file_id: Option<String>,
}

fn path_metadata(results: &[CodeRetrievalHit]) -> BTreeMap<String, HitPathMetadata> {
    let mut metadata = BTreeMap::new();
    for hit in results {
        metadata
            .entry(hit.path.clone())
            .or_insert_with(|| HitPathMetadata {
                language_id: hit.language_id.clone(),
                symbol_snapshot_id: hit.symbol_snapshot_id.clone(),
                canonical_symbol_id: hit.canonical_symbol_id.clone(),
                file_id: hit.file_id.clone(),
            });
    }

    metadata
}

fn dedupe_sort_truncate(results: &mut Vec<CodeRetrievalHit>, limit: usize) {
    let mut seen = BTreeSet::new();
    results
        .retain(|hit| seen.insert((hit.path.clone(), hit.line_range.start, hit.excerpt.clone())));
    results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line_range.start.cmp(&right.line_range.start))
    });
    results.truncate(limit);
}

#[cfg(test)]
#[path = "code_query_source_fallback_tests.rs"]
mod tests;
