use crate::{
    code::{
        SourceGrepKind, SourceGrepMatch, simple_source_identifier,
        source_fallback_reference_language_is_code, source_line_defines_identity,
    },
    domain::{CodeRetrievalHit, CodeRetrievalRequest},
};

use super::{
    identity::{exact_file_filter, source_identifier_char, source_identifier_ranges},
    imports::{quoted_import_specifier, relative_path_import_specifier},
    plan::CodeGrepFallbackPlan,
    surface::{exact_path_hybrid_source_line_score, source_type_declaration_line_matches_query},
};

const DYNAMIC_IMPORT_SOURCE_FALLBACK_BONUS: f64 = 1.1;
const HYBRID_EXACT_TYPE_DECLARATION_BONUS: f64 = 6.0;
const REFERENCE_DECLARATION_INTENT_BONUS: f64 = 2.2;
const REFERENCE_SOURCE_DECLARATION_PENALTY: f64 = -1.9;
const REFERENCE_SOURCE_COMMENT_PENALTY: f64 = -2.0;
const REFERENCE_DOCUMENT_SURFACE_PENALTY: f64 = -3.0;
const GENERATED_FILE_SCORE_MULTIPLIER: f64 = 0.35;

#[derive(Clone, Copy)]
pub(super) struct ScoreBounds {
    best: Option<f64>,
    lowest: Option<f64>,
}

impl ScoreBounds {
    pub(super) fn from_results(results: &[CodeRetrievalHit]) -> Self {
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

pub(super) fn grep_score(kind: SourceGrepKind, score_bounds: ScoreBounds) -> f64 {
    match kind {
        SourceGrepKind::Definition => score_bounds.best.unwrap_or(0.0) + 3.5,
        SourceGrepKind::References => score_bounds.best.unwrap_or(0.0) + 2.0,
        SourceGrepKind::Imports | SourceGrepKind::Hybrid => {
            score_bounds.lowest.map(|score| score - 0.25).unwrap_or(1.0)
        }
    }
}

pub(super) fn source_grep_match_score(
    request: &CodeRetrievalRequest,
    plan: &CodeGrepFallbackPlan,
    matched: &SourceGrepMatch,
    score_bounds: ScoreBounds,
    base_score: f64,
) -> f64 {
    if plan.kind == SourceGrepKind::Hybrid
        && plan.paths.iter().any(|path| exact_file_filter(path))
        && source_type_declaration_line_matches_query(&matched.excerpt, &request.query)
    {
        return score_bounds.best.unwrap_or(base_score) + HYBRID_EXACT_TYPE_DECLARATION_BONUS;
    }
    if plan.kind == SourceGrepKind::Hybrid {
        if let Some(score) = exact_path_hybrid_source_line_score(
            request,
            plan.paths.as_slice(),
            matched,
            score_bounds.lowest,
        ) {
            return score;
        }
    }

    let adjustment = match plan.kind {
        SourceGrepKind::References => {
            let language_adjustment =
                if source_fallback_reference_language_is_code(&matched.language_id) {
                    0.0
                } else {
                    REFERENCE_DOCUMENT_SURFACE_PENALTY
                };
            language_adjustment
                + reference_source_grep_score_adjustment(
                    &request.query,
                    &plan.query,
                    &matched.excerpt,
                )
        }
        SourceGrepKind::Imports => {
            import_source_grep_score_adjustment(&request.query, &plan.query, &matched.excerpt)
        }
        SourceGrepKind::Definition | SourceGrepKind::Hybrid => 0.0,
    };

    (base_score + adjustment).max(0.0)
}

pub(super) fn generated_adjusted_fallback_score(score: f64, is_generated: bool) -> f64 {
    if is_generated {
        score * GENERATED_FILE_SCORE_MULTIPLIER
    } else {
        score
    }
}

fn import_source_grep_score_adjustment(query: &str, specifier: &str, excerpt: &str) -> f64 {
    let line = excerpt.trim();
    if query_prefers_dynamic_import_source(query)
        && relative_path_import_specifier(specifier)
        && !source_line_starts_with_comment(line)
        && line.contains(specifier)
        && (line.contains("import(") || line.contains("import ("))
    {
        DYNAMIC_IMPORT_SOURCE_FALLBACK_BONUS
    } else {
        0.0
    }
}

fn query_prefers_dynamic_import_source(query: &str) -> bool {
    let query = query.trim();
    if quoted_import_specifier(query).is_none() {
        return false;
    }

    query.match_indices("import").any(|(index, _)| {
        let tail = &query[index + "import".len()..];
        let has_call = tail.starts_with('(') || tail.starts_with(" (");
        let has_token_boundary = query[..index].chars().last().is_none_or(|character| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '.'
        });

        has_call && has_token_boundary
    })
}

fn source_line_starts_with_comment(line: &str) -> bool {
    ["//", "#", "/*", "*", "--", "<!--"]
        .iter()
        .any(|prefix| line.starts_with(prefix))
}

pub(super) fn reference_source_grep_score_adjustment(
    query: &str,
    identity: &str,
    excerpt: &str,
) -> f64 {
    if !simple_source_identifier(identity) {
        return 0.0;
    }
    let line = excerpt.trim();
    if line.is_empty() {
        return 0.0;
    }
    if source_line_starts_with_comment(line) {
        return REFERENCE_SOURCE_COMMENT_PENALTY;
    }

    if source_reference_line_declares_identity(line, identity) {
        if reference_query_has_declaration_intent(query) {
            REFERENCE_DECLARATION_INTENT_BONUS
        } else {
            REFERENCE_SOURCE_DECLARATION_PENALTY
        }
    } else {
        0.0
    }
}

fn reference_query_has_declaration_intent(query: &str) -> bool {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .any(|term| matches!(term, "typedef" | "typealias" | "alias" | "using"))
}

fn source_reference_line_declares_identity(line: &str, identity: &str) -> bool {
    if source_line_defines_identity(line, identity) {
        return true;
    }

    source_identifier_ranges(line, identity).any(|(start, end)| {
        let before = line.get(..start).unwrap_or_default().trim_end();
        let after = line.get(end..).unwrap_or_default().trim_start();
        if before.ends_with('.')
            || before.ends_with("->")
            || before.ends_with(':')
            || identifier_is_assignment_value(before)
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
