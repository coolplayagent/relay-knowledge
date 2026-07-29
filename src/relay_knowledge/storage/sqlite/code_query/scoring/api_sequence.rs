use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

const MIN_BASE_SCORE: f64 = 6.0;
const MIN_API_IDENTITIES: usize = 3;
const MAX_NONBLANK_LINES: usize = 18;
const MAX_SEQUENCE_SPAN_LINES: usize = 14;
const UNIQUE_API_SEQUENCE_BONUS: f64 = 4.55;

pub(super) fn compact_unique_api_sequence_chunk_bonus(
    base_score: f64,
    query: &str,
    content: &str,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score < MIN_BASE_SCORE
        || request.code_query_kind != CodeQueryKind::Hybrid
        || (path_looks_like_test_or_benchmark(path) && !query_mentions_test_or_benchmark(query))
    {
        return 0.0;
    }

    let identities = query_api_sequence_identities(query);
    if identities.len() < MIN_API_IDENTITIES {
        return 0.0;
    }

    if content_has_unique_compact_sequence(content, &identities) {
        UNIQUE_API_SEQUENCE_BONUS
    } else {
        0.0
    }
}

fn content_has_unique_compact_sequence(content: &str, identities: &[Vec<String>]) -> bool {
    let mut identity_counts = vec![0usize; identities.len()];
    let mut nonblank_lines = 0usize;
    let mut first_match = None;
    let mut last_match = 0usize;

    for (line_index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        nonblank_lines += 1;
        if nonblank_lines > MAX_NONBLANK_LINES || !line_looks_like_api_call(line) {
            continue;
        }
        let line_terms = identifier_terms(line);
        for (identity_index, identity) in identities.iter().enumerate() {
            if identity_terms_match_line(identity, &line_terms) {
                identity_counts[identity_index] += 1;
                first_match.get_or_insert(line_index);
                last_match = line_index;
            }
        }
    }

    if nonblank_lines == 0 || nonblank_lines > MAX_NONBLANK_LINES {
        return false;
    }
    let Some(first_match) = first_match else {
        return false;
    };
    let span = last_match.saturating_sub(first_match).saturating_add(1);

    span <= MAX_SEQUENCE_SPAN_LINES && identity_counts.iter().all(|count| *count == 1)
}

fn query_api_sequence_identities(query: &str) -> Vec<Vec<String>> {
    let mut identities = Vec::new();
    for raw in query.split_whitespace().map(str::trim) {
        if !token_looks_like_api_identity(raw) {
            continue;
        }
        let terms = identifier_terms(raw)
            .into_iter()
            .filter(|term| term.len() >= 3)
            .filter(|term| !matches!(term.as_str(), "the" | "and" | "for" | "with" | "from"))
            .collect::<Vec<_>>();
        if !terms.is_empty() && !identities.contains(&terms) {
            identities.push(terms);
        }
    }

    identities
}

fn token_looks_like_api_identity(token: &str) -> bool {
    let token = token.trim_matches(|character: char| {
        !(character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | ':'))
    });
    if token.contains('.') || token.contains("::") {
        return true;
    }

    let mut previous = None;
    token.chars().any(|character| {
        let boundary = character.is_ascii_uppercase()
            && previous.is_some_and(|prev: char| prev.is_ascii_lowercase());
        previous = Some(character);
        boundary
    })
}

fn line_looks_like_api_call(line: &str) -> bool {
    if line.is_empty()
        || line.starts_with("//")
        || line.starts_with('#')
        || line.starts_with('*')
        || !line.contains('(')
    {
        return false;
    }

    line.contains('.')
        || line.contains("::")
        || line.contains("->")
        || line.contains(":=")
        || line.contains(" = ")
}

fn identity_terms_match_line(identity_terms: &[String], line_terms: &[String]) -> bool {
    identity_terms.iter().all(|identity_term| {
        line_terms
            .iter()
            .any(|line_term| related_identifier_terms(line_term, identity_term))
    })
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

fn query_mentions_test_or_benchmark(query: &str) -> bool {
    identifier_terms(query).iter().any(|term| {
        matches!(
            term.as_str(),
            "test" | "tests" | "testing" | "bench" | "benchmark" | "benchmarks"
        )
    })
}

fn path_looks_like_test_or_benchmark(path: &str) -> bool {
    path.to_ascii_lowercase().split('/').any(|segment| {
        matches!(
            segment,
            "test" | "tests" | "__tests__" | "testing" | "bench" | "benchmark" | "benchmarks"
        ) || segment.ends_with("_test")
            || segment.ends_with(".test.ts")
            || segment.ends_with(".test.tsx")
            || segment.ends_with(".spec.ts")
            || segment.ends_with(".spec.tsx")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

    #[test]
    fn unique_api_sequence_bonus_prefers_complete_non_repeated_flows() {
        let request = request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            CodeQueryKind::Hybrid,
        );
        let compact = compact_unique_api_sequence_chunk_bonus(
            8.0,
            &request.query,
            "func main() {\n\
            w := worker.New(c, \"hello-world\", worker.Options{})\n\
            w.RegisterWorkflow(helloworld.Workflow)\n\
            w.RegisterActivity(helloworld.Activity)\n\
            err = w.Run(worker.InterruptCh())\n\
            }",
            "helloworld/worker/main.go",
            &request,
        );
        let repeated = compact_unique_api_sequence_chunk_bonus(
            8.0,
            &request.query,
            "func main() {\n\
            w := worker.New(c, queue.Name, worker.Options{})\n\
            w.RegisterWorkflow(flow.Workflow)\n\
            w.RegisterActivity(flow.FirstActivity)\n\
            w.RegisterActivity(flow.SecondActivity)\n\
            err = w.Run(worker.InterruptCh())\n\
            }",
            "workflow/worker/main.go",
            &request,
        );
        let partial = compact_unique_api_sequence_chunk_bonus(
            8.0,
            &request.query,
            "func main() {\n\
            w := worker.New(c, queue.Name, worker.Options{})\n\
            w.RegisterWorkflow(flow.Workflow)\n\
            err = w.Run(worker.InterruptCh())\n\
            }",
            "workflow/worker/main.go",
            &request,
        );

        assert!(compact >= UNIQUE_API_SEQUENCE_BONUS);
        assert_eq!(repeated, 0.0);
        assert_eq!(partial, 0.0);
        assert_eq!(
            compact_unique_api_sequence_chunk_bonus(
                8.0,
                &request.query,
                "worker.New RegisterWorkflow RegisterActivity InterruptCh",
                "worker/worker_test.go",
                &request,
            ),
            0.0
        );
    }

    fn request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
        let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
        CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
            .expect("request should validate")
    }
}
