use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

pub(super) fn caller_context_density_bonus(
    base_score: f64,
    query: &str,
    caller_name: Option<&str>,
    callee_name: &str,
    path: &str,
    caller_excerpt: Option<&str>,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0 || request.code_query_kind != CodeQueryKind::Callers {
        return 0.0;
    }

    let target_terms = identifier_terms(&format!("{query} {callee_name}"));
    if target_terms.is_empty() {
        return 0.0;
    }

    caller_name_bonus(caller_name, &target_terms)
        + target_path_surface_bonus(path, &target_terms)
        + repeated_target_mention_bonus(caller_excerpt, callee_name)
}

fn caller_name_bonus(caller_name: Option<&str>, target_terms: &[String]) -> f64 {
    let Some(caller_name) = caller_name else {
        return 0.0;
    };
    let caller_terms = identifier_terms(caller_name);
    if caller_terms
        .iter()
        .any(|caller_term| target_terms.iter().any(|target| caller_term == target))
    {
        0.35
    } else {
        0.0
    }
}

fn target_path_surface_bonus(path: &str, target_terms: &[String]) -> f64 {
    let path_terms = identifier_terms(path);
    let has_surface_match = target_terms.iter().any(|target| {
        target.len() >= 4
            && path_terms
                .iter()
                .any(|path_term| related_identifier_terms(path_term, target))
    });
    if has_surface_match { 0.85 } else { 0.0 }
}

fn repeated_target_mention_bonus(caller_excerpt: Option<&str>, callee_name: &str) -> f64 {
    let Some(caller_excerpt) = caller_excerpt else {
        return 0.0;
    };
    let callee = callee_name.trim();
    if callee.is_empty() {
        return 0.0;
    }
    let mentions = caller_excerpt.match_indices(callee).count();
    if mentions <= 1 {
        0.0
    } else {
        (mentions.saturating_sub(1).min(3) as f64) * 0.2
    }
}

fn related_identifier_terms(left: &str, right: &str) -> bool {
    left == right
        || (left.len() >= 4
            && right.len() >= 4
            && (left.starts_with(right) || right.starts_with(left)))
}

fn identifier_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for raw in value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
    {
        terms.push(raw.to_ascii_lowercase());
        terms.extend(
            raw.split('_')
                .filter(|part| !part.is_empty())
                .map(str::to_ascii_lowercase),
        );
        push_camel_terms(raw, &mut terms);
    }
    terms.sort();
    terms.dedup();
    terms
}

fn push_camel_terms(token: &str, terms: &mut Vec<String>) {
    let chars = token.char_indices().collect::<Vec<_>>();
    if chars.is_empty() {
        return;
    }

    let mut start = 0;
    for index in 1..chars.len() {
        let previous = chars[index - 1].1;
        let current = chars[index].1;
        let next = chars.get(index + 1).map(|(_, character)| *character);
        let starts_word = previous.is_ascii_lowercase() && current.is_ascii_uppercase();
        let ends_acronym = previous.is_ascii_uppercase()
            && current.is_ascii_uppercase()
            && next.is_some_and(|character| character.is_ascii_lowercase());
        let changes_kind = previous.is_ascii_alphabetic() != current.is_ascii_alphabetic();
        if starts_word || ends_acronym || changes_kind {
            terms.push(token[start..chars[index].0].to_ascii_lowercase());
            start = chars[index].0;
        }
    }
    terms.push(token[start..].to_ascii_lowercase());
}
