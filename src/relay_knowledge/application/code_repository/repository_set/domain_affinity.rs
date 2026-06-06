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
mod tests {
    use super::*;
    use crate::domain::{
        CodeRepositorySetMember, CodeRepositorySetMemberStatus, CodeRetrievalLayer,
        RepositoryCodeRange,
    };

    #[test]
    fn priority_domain_affinity_promotes_prioritized_member_specific_terms() {
        let member = member_status("preferred", "scope-preferred", 10);
        let target = hit(
            "repo-preferred",
            "scope-preferred",
            "connectors/metricsink/metricsink.go",
            1,
            10.0,
            false,
        );
        let generic = hit(
            "repo-preferred",
            "scope-preferred",
            "connectors/generic.go",
            1,
            10.0,
            false,
        );

        assert!(
            priority_domain_affinity_bonus(
                "sink.NewFactory EmitBatches metric_sink factory pipeline",
                &target,
                &member,
            ) > 0.0
        );
        assert_eq!(
            priority_domain_affinity_bonus(
                "sink.NewFactory EmitBatches metric_sink factory pipeline",
                &generic,
                &member,
            ),
            0.0
        );
    }

    #[test]
    fn priority_domain_affinity_requires_positive_priority() {
        let member = member_status("dependency", "scope-dependency", 0);
        let target = hit(
            "repo-dependency",
            "scope-dependency",
            "connectors/metricsink/metricsink.go",
            1,
            10.0,
            false,
        );

        assert_eq!(
            priority_domain_affinity_bonus("metric_sink pipeline", &target, &member),
            0.0
        );
    }

    fn member_status(
        repository_alias: &str,
        source_scope: &str,
        priority: i32,
    ) -> CodeRepositorySetMemberStatus {
        CodeRepositorySetMemberStatus {
            member: CodeRepositorySetMember {
                set_id: "set-workspace".to_owned(),
                repository_id: format!("repo-{repository_alias}"),
                repository_alias: repository_alias.to_owned(),
                ref_selector: "HEAD".to_owned(),
                resolved_commit_sha: format!("commit-{source_scope}"),
                source_scope: source_scope.to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
                priority,
            },
            tree_hash: format!("tree-{source_scope}"),
            indexed_path_filters: Vec::new(),
            indexed_language_filters: Vec::new(),
            freshness_state: "fresh".to_owned(),
            stale: false,
            indexed_file_count: 1,
            symbol_count: 1,
            reference_count: 0,
            chunk_count: 1,
            degraded_reason: None,
        }
    }

    fn hit(
        repository_id: &str,
        scope_id: &str,
        path: &str,
        line: u32,
        score: f64,
        stale: bool,
    ) -> CodeRetrievalHit {
        CodeRetrievalHit {
            repository_id: repository_id.to_owned(),
            scope_id: scope_id.to_owned(),
            resolved_commit_sha: format!("commit-{scope_id}"),
            tree_hash: format!("tree-{scope_id}"),
            path: path.to_owned(),
            language_id: "go".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 10 },
            line_range: RepositoryCodeRange {
                start: line,
                end: line,
            },
            symbol_snapshot_id: None,
            canonical_symbol_id: None,
            file_id: Some("file-1".to_owned()),
            retrieval_layers: vec![CodeRetrievalLayer::Lexical],
            index_versions: vec!["code:1".to_owned()],
            stale,
            staleness_hint: None,
            degraded_reason: None,
            edge_kind: None,
            edge_resolution_state: None,
            edge_target_hint: None,
            edge_confidence_basis_points: None,
            edge_confidence_tier: None,
            score,
            excerpt: String::new(),
        }
    }
}
