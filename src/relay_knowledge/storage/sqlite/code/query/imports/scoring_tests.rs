use super::*;

#[test]
fn extensionless_target_requires_extra_importer_identity_context() {
    let target_only = import_importer_path_context_bonus(
        1.0,
        1,
        "services/cache",
        &importer_path_context(
            "src/cache/cache_consumer.ts",
            "import cache from \"services/cache\";",
        ),
        CodeQueryKind::Imports,
    );
    let contextual = import_importer_path_context_bonus(
        1.0,
        1,
        "cache_consumer services/cache",
        &importer_path_context(
            "src/cache/cache_consumer.ts",
            "import cache from \"services/cache\";",
        ),
        CodeQueryKind::Imports,
    );

    assert_eq!(target_only, 0.0);
    assert!(contextual > target_only);
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
        2.25
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
fn path_import_context_bonus_requires_explicit_importer_identity() {
    assert_eq!(
        import_importer_path_context_bonus(
            3.0,
            2,
            "cache_consumer store/cache.hpp",
            &importer_path_context(
                "src/storage/cache_consumer.cc",
                "#include <store/cache.hpp>",
            ),
            CodeQueryKind::Imports,
        ),
        0.65
    );
    assert_eq!(
        import_importer_path_context_bonus(
            3.0,
            2,
            "cache_consumer store/cache.hpp",
            &importer_path_context("src/storage/consumer.cc", "#include <store/cache.hpp>"),
            CodeQueryKind::Imports,
        ),
        0.0
    );
    assert_eq!(
        import_importer_path_context_bonus(
            0.0,
            2,
            "cache_consumer store/cache.hpp",
            &importer_path_context(
                "src/storage/cache_consumer.cc",
                "#include <store/cache.hpp>",
            ),
            CodeQueryKind::Imports,
        ),
        0.0
    );
    assert_eq!(
        import_importer_path_context_bonus(
            3.0,
            0,
            "cache_consumer store/cache.hpp",
            &importer_path_context(
                "src/storage/cache_consumer.cc",
                "#include <store/cache.hpp>",
            ),
            CodeQueryKind::Imports,
        ),
        0.0
    );
}

#[test]
fn symbol_import_context_bonus_uses_explicit_importer_identity_terms() {
    let expected = importer_path_context(
        "spring-beans/src/ExtendedBeanInfo.java",
        "import org.springframework.util.ObjectUtils;",
    );
    let other = importer_path_context(
        "spring-beans/src/OtherBeanInfo.java",
        "import org.springframework.util.ObjectUtils;",
    );
    let target_only = import_importer_path_context_bonus(
        3.0,
        2,
        "org.springframework.util.ObjectUtils",
        &expected,
        CodeQueryKind::Imports,
    );
    let contextual = import_importer_path_context_bonus(
        3.0,
        2,
        "ExtendedBeanInfo org.springframework.util.ObjectUtils",
        &expected,
        CodeQueryKind::Imports,
    );
    let unrelated = import_importer_path_context_bonus(
        3.0,
        2,
        "ExtendedBeanInfo org.springframework.util.ObjectUtils",
        &other,
        CodeQueryKind::Imports,
    );

    assert_eq!(target_only, 0.0);
    assert!(contextual >= IMPORT_PATH_CONTEXT_BONUS_PER_TERM);
    assert_eq!(unrelated, 0.0);
}

#[test]
fn importer_usage_bonus_counts_the_first_use_after_the_import_statement() {
    assert!(import_same_file_usage_bonus(3.0, 1, CodeQueryKind::Imports) > 0.0);
    assert_eq!(
        import_same_file_usage_bonus(3.0, 0, CodeQueryKind::Imports),
        0.0
    );
}

fn importer_path_context<'a>(path: &'a str, module: &'a str) -> ImporterPathContext<'a> {
    ImporterPathContext {
        path,
        module,
        target_hint: None,
        matched_symbol_name: None,
        target_symbol_names: None,
    }
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
