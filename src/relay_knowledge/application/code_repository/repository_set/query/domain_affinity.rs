use crate::domain::{CodeRepositorySetMemberStatus, CodeRetrievalHit};

const MAX_PRIORITY_DOMAIN_AFFINITY_BONUS: f64 = 5.0;

pub(super) fn priority_domain_affinity_bonus(
    query: &str,
    hit: &CodeRetrievalHit,
    member: &CodeRepositorySetMemberStatus,
) -> f64 {
    if member.member.priority <= 0 {
        return 0.0;
    }
    let query_terms = query_domain_terms(query);
    if query_terms.is_empty() {
        return 0.0;
    }
    let surface = searchable_surface(hit);
    let matched_terms = query_terms
        .iter()
        .filter(|term| surface.contains(term.as_str()))
        .count();
    if matched_terms == 0 {
        return 0.0;
    }

    let priority_scale = f64::from(member.member.priority.clamp(1, 10)) / 10.0;
    (matched_terms as f64 * MAX_PRIORITY_DOMAIN_AFFINITY_BONUS * priority_scale)
        .min(MAX_PRIORITY_DOMAIN_AFFINITY_BONUS)
}

fn query_domain_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for token in query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
    {
        push_domain_term(&mut terms, &token.to_ascii_lowercase());
        let compact = token
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        push_domain_term(&mut terms, &compact);
        for part in token.split('_').filter(|part| !part.is_empty()) {
            push_domain_term(&mut terms, &part.to_ascii_lowercase());
        }
        push_camel_terms(token, &mut terms);
    }

    terms
}

fn push_camel_terms(token: &str, terms: &mut Vec<String>) {
    let chars = token.char_indices().collect::<Vec<_>>();
    if chars.is_empty() {
        return;
    }

    let mut start = 0usize;
    for index in 1..chars.len() {
        let previous = chars[index - 1].1;
        let current = chars[index].1;
        let next = chars.get(index + 1).map(|(_, character)| *character);
        if (previous.is_ascii_lowercase() && current.is_ascii_uppercase())
            || (previous.is_ascii_uppercase()
                && current.is_ascii_uppercase()
                && next.is_some_and(|character| character.is_ascii_lowercase()))
            || previous.is_ascii_alphabetic() != current.is_ascii_alphabetic()
        {
            push_domain_term(terms, &token[start..chars[index].0].to_ascii_lowercase());
            start = chars[index].0;
        }
    }
    push_domain_term(terms, &token[start..].to_ascii_lowercase());
}

fn push_domain_term(terms: &mut Vec<String>, term: &str) {
    if term.len() >= 4 && !generic_domain_term(term) && !terms.iter().any(|value| value == term) {
        terms.push(term.to_owned());
    }
}

fn generic_domain_term(term: &str) -> bool {
    matches!(
        term,
        "component"
            | "config"
            | "create"
            | "factory"
            | "handler"
            | "logs"
            | "receiver"
            | "request"
            | "response"
            | "service"
            | "type"
    )
}

fn searchable_surface(hit: &CodeRetrievalHit) -> String {
    format!("{} {}", hit.path, hit.excerpt)
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

#[cfg(test)]
#[path = "domain_affinity_tests.rs"]
mod tests;
