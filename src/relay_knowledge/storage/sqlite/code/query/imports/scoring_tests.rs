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
