use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

const DESIGNATED_INITIALIZER_TERMS: &[&str] = &[
    "callback",
    "callbacks",
    "designated",
    "dispatch",
    "initializer",
    "initializers",
    "operation",
    "operations",
    "table",
    "tables",
];
const DESIGNATED_INITIALIZER_BASE_BONUS: f64 = 1.8;
const DESIGNATED_INITIALIZER_MAX_BONUS: f64 = 6.6;
const DESIGNATOR_LINE_BONUS: f64 = 0.45;
const CALLABLE_ASSIGNMENT_BONUS: f64 = 0.85;
const OPERATION_TABLE_SURFACE_BONUS: f64 = 0.75;

pub(super) fn designated_initializer_chunk_bonus(
    base_score: f64,
    query: &str,
    content: &str,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0
        || request.code_query_kind != CodeQueryKind::Hybrid
        || (path_looks_like_test_or_benchmark(path) && !query_mentions_test_or_benchmark(query))
        || !query_designated_initializer_intent(query)
    {
        return 0.0;
    }

    let shape = designated_initializer_shape(content);
    if shape.designator_lines == 0 {
        return 0.0;
    }
    let query_terms = meaningful_terms(query);
    let content_terms = meaningful_terms(content);
    let matched_terms = matched_query_terms(&query_terms, &content_terms);
    if matched_terms.is_empty() {
        return 0.0;
    }

    let coverage = matched_terms.len() as f64 / query_terms.len().max(1) as f64;
    let designator_density = shape.designator_lines.min(4) as f64 * DESIGNATOR_LINE_BONUS;
    let callable_density = shape.callable_assignments.min(3) as f64 * CALLABLE_ASSIGNMENT_BONUS;
    let operation_surface = operation_table_surface_bonus(&query_terms, &content_terms, &shape);
    (DESIGNATED_INITIALIZER_BASE_BONUS
        + designator_density
        + callable_density
        + operation_surface
        + (coverage * 0.6))
        .min(DESIGNATED_INITIALIZER_MAX_BONUS)
}

fn query_designated_initializer_intent(query: &str) -> bool {
    let terms = meaningful_terms(query);
    terms
        .iter()
        .filter(|term| DESIGNATED_INITIALIZER_TERMS.contains(&term.as_str()))
        .take(2)
        .count()
        >= 2
}

struct DesignatedInitializerShape {
    designator_lines: usize,
    callable_assignments: usize,
}

fn operation_table_surface_bonus(
    query_terms: &[String],
    content_terms: &[String],
    shape: &DesignatedInitializerShape,
) -> f64 {
    if shape.callable_assignments < 2
        || !query_terms.iter().any(|term| {
            matches!(
                term.as_str(),
                "callback" | "callbacks" | "dispatch" | "operation" | "operations"
            )
        })
        || !query_terms.iter().any(|term| {
            matches!(
                term.as_str(),
                "initializer" | "initializers" | "table" | "tables"
            )
        })
        || !content_terms.iter().any(|term| {
            matches!(
                term.as_str(),
                "callback"
                    | "callbacks"
                    | "dispatch"
                    | "handler"
                    | "handlers"
                    | "operation"
                    | "operations"
                    | "ops"
                    | "vtable"
                    | "vtbl"
            )
        })
    {
        return 0.0;
    }

    OPERATION_TABLE_SURFACE_BONUS
}

fn designated_initializer_shape(content: &str) -> DesignatedInitializerShape {
    let mut shape = DesignatedInitializerShape {
        designator_lines: 0,
        callable_assignments: 0,
    };
    for line in content.lines().map(str::trim) {
        if !line_has_designated_initializer_assignment(line) {
            continue;
        }
        shape.designator_lines += 1;
        if line_assigns_callable_identifier(line) {
            shape.callable_assignments += 1;
        }
    }

    shape
}

fn line_has_designated_initializer_assignment(line: &str) -> bool {
    if line.starts_with("//") || line.starts_with('*') || !line.contains('=') {
        return false;
    }
    let Some((left, _)) = line.split_once('=') else {
        return false;
    };
    left.split([',', '{']).map(str::trim).any(|part| {
        part.starts_with('.')
            || (part.starts_with('[')
                && part.contains(']')
                && part.split(']').next().is_some_and(|index| index.len() > 2))
    })
}

fn line_assigns_callable_identifier(line: &str) -> bool {
    let Some((_, right)) = line.split_once('=') else {
        return false;
    };
    let value = right
        .trim()
        .trim_end_matches(',')
        .trim_end_matches(';')
        .trim();
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn matched_query_terms(query_terms: &[String], content_terms: &[String]) -> Vec<String> {
    let mut matched = Vec::new();
    for query_term in query_terms {
        if matched.contains(query_term) {
            continue;
        }
        if content_terms
            .iter()
            .any(|content_term| related_identifier_terms(content_term, query_term))
        {
            matched.push(query_term.clone());
        }
    }

    matched
}

fn related_identifier_terms(left: &str, right: &str) -> bool {
    left == right
        || (left.len() >= 4
            && right.len() >= 4
            && (left.starts_with(right) || right.starts_with(left)))
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

fn query_mentions_test_or_benchmark(query: &str) -> bool {
    meaningful_terms(query).iter().any(|term| {
        matches!(
            term.as_str(),
            "test" | "tests" | "testing" | "bench" | "benchmark" | "benchmarks"
        )
    })
}

fn meaningful_terms(value: &str) -> Vec<String> {
    let mut terms = identifier_terms(value)
        .into_iter()
        .filter(|term| term.len() >= 3)
        .filter(|term| !matches!(term.as_str(), "the" | "and" | "for" | "with" | "from"))
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
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
        let (byte_index, character) = chars[index];
        let (_, previous) = chars[index - 1];
        let next = chars.get(index + 1).map(|(_, next)| *next);
        let starts_upper_word = character.is_ascii_uppercase()
            && (previous.is_ascii_lowercase()
                || previous.is_ascii_digit()
                || next.is_some_and(|next| next.is_ascii_lowercase()));
        if starts_upper_word {
            terms.push(token[start..byte_index].to_ascii_lowercase());
            start = byte_index;
        }
    }
    if start < token.len() {
        terms.push(token[start..].to_ascii_lowercase());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

    #[test]
    fn designated_initializer_bonus_prefers_callback_table_entries() {
        let hybrid = request(
            "operation table read callback dispatch designated initializer",
            CodeQueryKind::Hybrid,
        );
        let bonus = designated_initializer_chunk_bonus(
            2.0,
            &hybrid.query,
            "const struct rk_driver_ops rk_default_ops = {\n\
                .open = rk_driver_open,\n\
                .read = rk_driver_read,\n\
                .close = rk_driver_close,\n\
            };",
            "src/driver_ops.c",
            &hybrid,
        );
        let declaration = designated_initializer_chunk_bonus(
            2.0,
            &hybrid.query,
            "struct rk_driver_ops {\n    rk_read_fn read;\n};",
            "include/driver_ops.h",
            &hybrid,
        );

        assert!(
            bonus > 3.0,
            "designated initializer bonus too small: {bonus}"
        );
        assert_eq!(declaration, 0.0);
    }

    #[test]
    fn designated_initializer_bonus_prefers_multi_callable_tables() {
        let hybrid = request(
            "operation table read callback dispatch designated initializer",
            CodeQueryKind::Hybrid,
        );
        let sparse = designated_initializer_chunk_bonus(
            2.0,
            &hybrid.query,
            "static const struct rk_table_row rk_rows[] = {\n\
                [RK_STAGE_READ] = {\n\
                    .name = \"read\",\n\
                    .read = rk_driver_read,\n\
                },\n\
            };",
            "src/generated_table.c",
            &hybrid,
        );
        let multi_callable = designated_initializer_chunk_bonus(
            2.0,
            &hybrid.query,
            "const struct rk_driver_ops rk_default_ops = {\n\
                .open = rk_driver_open,\n\
                .read = rk_driver_read,\n\
                .close = rk_driver_close,\n\
            };",
            "src/driver_ops.c",
            &hybrid,
        );

        assert!(
            multi_callable > sparse,
            "sparse={sparse} multi={multi_callable}"
        );
    }

    #[test]
    fn designated_initializer_bonus_detects_operation_surface_shorthand() {
        let hybrid = request(
            "operation table read callback dispatch designated initializer",
            CodeQueryKind::Hybrid,
        );
        let generic_table = designated_initializer_chunk_bonus(
            2.0,
            &hybrid.query,
            "const struct rk_driver_table rk_default_table = {\n\
                .open = rk_driver_open,\n\
                .read = rk_driver_read,\n\
                .close = rk_driver_close,\n\
            };",
            "src/driver_table.c",
            &hybrid,
        );
        let operation_table = designated_initializer_chunk_bonus(
            2.0,
            &hybrid.query,
            "const struct rk_driver_ops rk_default_ops = {\n\
                .open = rk_driver_open,\n\
                .read = rk_driver_read,\n\
                .close = rk_driver_close,\n\
            };",
            "src/driver_ops.c",
            &hybrid,
        );

        assert!(
            operation_table > generic_table + 0.5,
            "operation_table={operation_table} generic_table={generic_table}"
        );
    }

    #[test]
    fn operation_surface_bonus_requires_multiple_callable_assignments() {
        let hybrid = request(
            "operation table read callback dispatch designated initializer",
            CodeQueryKind::Hybrid,
        );
        let sparse_operation_table = designated_initializer_chunk_bonus(
            2.0,
            &hybrid.query,
            "static const struct rk_driver_ops rk_default_ops = {\n\
                .name = \"read\",\n\
                .read = rk_driver_read,\n\
            };",
            "src/driver_ops.c",
            &hybrid,
        );

        assert!(
            sparse_operation_table < 5.0,
            "sparse operation table should not receive operation-surface bonus: {sparse_operation_table}"
        );
    }

    #[test]
    fn designated_initializer_bonus_ignores_tests_without_test_intent() {
        let hybrid = request(
            "operation table read callback dispatch designated initializer",
            CodeQueryKind::Hybrid,
        );

        assert_eq!(
            designated_initializer_chunk_bonus(
                2.0,
                &hybrid.query,
                ".read = rk_driver_read,",
                "tests/fake_driver.c",
                &hybrid,
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
