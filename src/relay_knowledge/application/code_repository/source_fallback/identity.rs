use crate::{
    code::{simple_source_identifier, source_line_defines_identity},
    domain::{CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest},
};

const MAX_DEFINITION_SOURCE_CANDIDATE_PATHS: usize = 8;

pub(super) fn source_identifier_ranges<'a>(
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

pub(super) fn source_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

pub(super) fn definition_source_candidate_paths(
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

pub(super) fn hybrid_results_cover_identity(results: &[CodeRetrievalHit], identity: &str) -> bool {
    results.iter().any(|hit| {
        hit.retrieval_layers.iter().any(|layer| {
            matches!(
                layer,
                CodeRetrievalLayer::Symbol | CodeRetrievalLayer::Definition
            )
        }) && (hit
            .canonical_symbol_id
            .as_deref()
            .is_some_and(|symbol_id| canonical_symbol_leaf_matches(symbol_id, identity))
            || hit
                .excerpt
                .lines()
                .any(|line| source_identifier_ranges(line, identity).next().is_some()))
    })
}

fn canonical_symbol_leaf_matches(canonical_symbol_id: &str, identity: &str) -> bool {
    canonical_symbol_id
        .rsplit(|character: char| !source_identifier_char(character))
        .find(|term| !term.is_empty())
        .is_some_and(|leaf| leaf == identity)
}

pub(super) fn push_candidate_path(paths: &mut Vec<String>, path: &str) {
    let normalized = normalize_filter_path(path);
    if !normalized.is_empty() && !paths.iter().any(|existing| existing == normalized) {
        paths.push(normalized.to_owned());
    }
}

pub(super) fn exact_file_filter(path: &str) -> bool {
    let path = normalize_filter_path(path);
    !path.is_empty()
        && path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'))
        && !path.ends_with('/')
}

pub(super) fn normalize_filter_path(path: &str) -> &str {
    let mut path = path.trim_end_matches(['/', '\\']);
    while let Some(stripped) = path.strip_prefix("./") {
        path = stripped;
    }

    path
}

pub(super) fn results_define_identity(results: &[CodeRetrievalHit], identity: &str) -> bool {
    results.iter().any(|hit| {
        hit.excerpt
            .lines()
            .map(str::trim)
            .any(|line| source_line_defines_identity(line, identity))
    })
}

pub(super) fn definition_identity(query: &str) -> Option<String> {
    let mut identity = None;
    for raw_token in query.split_whitespace().map(str::trim) {
        if raw_token.contains('/') || raw_token.contains('\\') {
            continue;
        }
        let terms = raw_token
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();
        if let Some(term) = terms.last().filter(|term| simple_source_identifier(term)) {
            identity = Some((*term).to_owned());
        }
    }

    identity
}

pub(super) fn source_grep_identity(query: &str) -> Option<String> {
    let identity = definition_identity(query)?;
    (query.split_whitespace().count() == 1).then_some(identity)
}

pub(super) fn reference_grep_query(query: &str) -> Option<String> {
    source_grep_identity(query).or_else(|| leading_source_identifier(query))
}

fn leading_source_identifier(query: &str) -> Option<String> {
    for raw_token in query.split_whitespace().map(str::trim) {
        if raw_token.contains('/') || raw_token.contains('\\') {
            continue;
        }
        let token = raw_token.trim_matches(|character: char| {
            !(character.is_ascii_alphanumeric()
                || character == '_'
                || character == '.'
                || character == ':')
        });
        if token.is_empty() {
            continue;
        }
        if (token.contains('.') || token.contains("::"))
            && token
                .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
                .filter(|term| simple_source_identifier(term))
                .count()
                >= 2
        {
            return Some(token.to_owned());
        }
        if let Some(term) = token
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .find(|term| term.len() >= 3 && simple_source_identifier(term))
        {
            return Some(term.to_owned());
        }
    }

    None
}
