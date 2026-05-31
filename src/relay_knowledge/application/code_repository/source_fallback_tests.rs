use super::*;
use crate::domain::{RepositoryCodeRange, code_snapshot_scope_id};

#[test]
fn fallback_plan_uses_contextual_hits_and_exact_file_filters() {
    let request = request(
        "rk_read_fn",
        CodeQueryKind::Definition,
        vec!["include/driver_ops.h".to_owned()],
    );
    let hit = hit(
        "include/driver_ops.h",
        "struct rk_driver_ops {\n    rk_read_fn read;\n}",
    );

    let plan = plan_code_grep_fallback(&status(), &request, &[hit])
        .expect("contextual hit should plan fallback");

    assert_eq!(plan.identity.as_deref(), Some("rk_read_fn"));
    assert_eq!(plan.query, "rk_read_fn");
    assert_eq!(plan.paths, ["include/driver_ops.h"]);
}

#[test]
fn fallback_plan_uses_definition_query_target_not_command_word() {
    let request = request("find rk_read_fn", CodeQueryKind::Definition, Vec::new());
    let hit = hit(
        "include/driver_ops.h",
        "struct rk_driver_ops {\n    rk_read_fn read;\n}",
    );

    let plan = plan_code_grep_fallback(&status(), &request, &[hit])
        .expect("natural-language definition query should plan fallback");

    assert_eq!(plan.identity.as_deref(), Some("rk_read_fn"));
    assert_eq!(plan.query, "rk_read_fn");
}

#[test]
fn fallback_plan_skips_results_with_exact_declaration() {
    let request = request("rk_read_fn", CodeQueryKind::Definition, Vec::new());
    let mut hit = hit(
        "include/driver_ops.h",
        "typedef int (*rk_read_fn)(struct rk_device *dev);",
    );
    hit.retrieval_layers = vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];

    assert!(plan_code_grep_fallback(&status(), &request, &[hit]).is_none());
}

#[test]
fn hybrid_grep_fallback_fills_after_structured_hits() {
    let request = request("rk_helper", CodeQueryKind::Hybrid, Vec::new());
    let mut results = vec![hit("src/lib.c", "void structured_hit(void);")];
    let plan = plan_code_grep_fallback(&status(), &request, &results)
        .expect("partial hybrid results should plan fallback");
    let outcome = SourceGrepOutcome {
        matches: vec![SourceGrepMatch {
            path: "src/fallback.c".to_owned(),
            language_id: "c".to_owned(),
            excerpt: "rk_helper();".to_owned(),
            byte_range: RepositoryCodeRange { start: 10, end: 19 },
            line_range: RepositoryCodeRange { start: 4, end: 4 },
        }],
        degraded_reason: None,
    };

    append_code_grep_fallback(&status(), &request, &mut results, &plan, outcome);

    assert_eq!(results[0].path, "src/lib.c");
    let fallback = results
        .iter()
        .find(|hit| hit.path == "src/fallback.c")
        .expect("fallback hit should be appended");
    assert!(fallback.score < results[0].score);
    assert!(
        fallback
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
}

#[test]
fn hybrid_grep_fallback_skips_exact_symbol_coverage() {
    let request = request("ConnectorService", CodeQueryKind::Hybrid, Vec::new());
    let mut result = hit("src/service.py", "class ConnectorService:");
    result.language_id = "python".to_owned();
    result.retrieval_layers = vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];
    result.canonical_symbol_id =
        Some("repo://repo/src::relay_teams::connector::service::ConnectorService".to_owned());

    assert!(plan_code_grep_fallback(&status(), &request, &[result]).is_none());
}

#[test]
fn hybrid_grep_fallback_uses_text_fallback_for_non_symbol_coverage() {
    let request = request("ConnectorService", CodeQueryKind::Hybrid, Vec::new());
    let result = hit(
        "docs/service.md",
        "ConnectorService appears in deployment notes.",
    );

    let plan = plan_code_grep_fallback(&status(), &request, &[result])
        .expect("lexical-only context should still allow source fallback");

    assert_eq!(plan.kind, SourceGrepKind::Hybrid);
    assert!(plan.needs_scope_paths());
}

#[test]
fn hybrid_source_surface_fallback_refreshes_same_line_excerpt() {
    let request = request(
        "typed arrow payload projector trim provider record",
        CodeQueryKind::Hybrid,
        Vec::new(),
    );
    let mut result = hit(
        "src/protocol.ts",
        "trimPayload: PayloadProjector<string> = (payload) => payload.trim()",
    );
    result.line_range = RepositoryCodeRange { start: 13, end: 13 };
    result.retrieval_layers = vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];
    result.canonical_symbol_id = Some("repo://repo/src::protocol::trimPayload".to_owned());
    let mut type_result = hit(
        "src/protocol.ts",
        "type PayloadProjector<TPayload> = (payload: TPayload) => TPayload;",
    );
    type_result.line_range = RepositoryCodeRange { start: 11, end: 11 };
    type_result.retrieval_layers = vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];
    type_result.canonical_symbol_id =
        Some("repo://repo/src::protocol::PayloadProjector".to_owned());
    let plan = plan_code_grep_fallback(&status(), &request, &[result.clone(), type_result.clone()])
        .expect("hybrid API surface should plan same-file source refresh");
    let mut results = vec![result, type_result];

    assert_eq!(plan.query, "PayloadProjector");
    assert_eq!(plan.paths, ["src/protocol.ts"]);

    append_code_grep_fallback(
        &status(),
        &request,
        &mut results,
        &plan,
        SourceGrepOutcome {
            matches: vec![SourceGrepMatch {
                path: "src/protocol.ts".to_owned(),
                language_id: "typescript".to_owned(),
                excerpt:
                    "export const trimPayload: PayloadProjector<string> = (payload) => payload.trim();"
                        .to_owned(),
                byte_range: RepositoryCodeRange { start: 0, end: 82 },
                line_range: RepositoryCodeRange { start: 13, end: 13 },
            }, SourceGrepMatch {
                path: "src/protocol.ts".to_owned(),
                language_id: "typescript".to_owned(),
                excerpt:
                    "export type PayloadProjector<TPayload> = (payload: TPayload) => TPayload;"
                        .to_owned(),
                byte_range: RepositoryCodeRange { start: 0, end: 73 },
                line_range: RepositoryCodeRange { start: 11, end: 11 },
            }],
            degraded_reason: None,
        },
    );

    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .any(|hit| hit.excerpt.starts_with("export const trimPayload"))
    );
    assert!(
        results
            .iter()
            .any(|hit| hit.excerpt.starts_with("export type PayloadProjector"))
    );
    assert!(
        results[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
}

#[test]
fn hybrid_source_surface_fallback_skips_complete_exported_value_surfaces() {
    let request = request(
        "typed arrow payload projector trim provider record",
        CodeQueryKind::Hybrid,
        Vec::new(),
    );
    let mut result = hit(
        "src/protocol.ts",
        "export const trimPayload: PayloadProjector<string> = (payload) => payload.trim();",
    );
    result.retrieval_layers = vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];
    result.canonical_symbol_id = Some("repo://repo/src::protocol::trimPayload".to_owned());
    let mut type_result = hit(
        "src/protocol.ts",
        "export type PayloadProjector<TPayload> = (payload: TPayload) => TPayload;",
    );
    type_result.retrieval_layers = vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];
    type_result.canonical_symbol_id =
        Some("repo://repo/src::protocol::PayloadProjector".to_owned());
    let mut contextual_type_result = hit("src/provider.ts", "PayloadProjector<string>");
    contextual_type_result.retrieval_layers =
        vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];
    contextual_type_result.canonical_symbol_id =
        Some("repo://repo/src::protocol::PayloadProjector".to_owned());

    assert!(
        plan_code_grep_fallback(
            &status(),
            &request,
            &[contextual_type_result, result, type_result]
        )
        .is_none()
    );
}

#[test]
fn import_fallback_runs_for_unresolved_external_imports_without_degrading() {
    let request = request("ProviderShared", CodeQueryKind::Imports, Vec::new());
    let mut import_hit = hit("src/component.tsx", "react");
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("unresolved".to_owned());
    import_hit.edge_target_hint = Some("react".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
    let mut results = vec![import_hit];
    let plan = plan_code_grep_fallback(&status(), &request, &results)
        .expect("unresolved import should plan source fallback");
    assert_eq!(plan.query, "react");
    assert_eq!(plan.paths, ["src/component.tsx"]);
    assert!(!plan.needs_scope_paths());
    let outcome = SourceGrepOutcome {
        matches: vec![SourceGrepMatch {
            path: "src/component.tsx".to_owned(),
            language_id: "tsx".to_owned(),
            excerpt: "import React from \"react\";".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 26 },
            line_range: RepositoryCodeRange { start: 1, end: 1 },
        }],
        degraded_reason: None,
    };

    let reason = append_code_grep_fallback(&status(), &request, &mut results, &plan, outcome);

    assert_eq!(reason, None);
    assert!(results.iter().any(|hit| {
        hit.retrieval_layers
            .contains(&CodeRetrievalLayer::ImportGraph)
    }));
    assert!(results.iter().any(|hit| {
        hit.retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    }));
}

#[test]
fn import_fallback_searches_only_matching_unresolved_external_import_paths() {
    let request = request("ProviderShared", CodeQueryKind::Imports, Vec::new());
    let mut react_one = hit("src/component.tsx", "react");
    react_one.edge_kind = Some("import".to_owned());
    react_one.edge_resolution_state = Some("unresolved".to_owned());
    react_one.edge_target_hint = Some("import React from \"react\";".to_owned());
    react_one.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
    let mut vue = hit("src/other.tsx", "vue");
    vue.edge_kind = Some("import".to_owned());
    vue.edge_resolution_state = Some("unresolved".to_owned());
    vue.edge_target_hint = Some("import Vue from \"vue\";".to_owned());
    vue.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
    let mut react_two = hit("src/nested/panel.tsx", "react");
    react_two.edge_kind = Some("import".to_owned());
    react_two.edge_resolution_state = Some("unresolved".to_owned());
    react_two.edge_target_hint = Some("react".to_owned());
    react_two.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];

    let plan = plan_code_grep_fallback(&status(), &request, &[react_one, vue, react_two])
        .expect("unresolved external imports should plan fallback");

    assert_eq!(plan.query, "react");
    assert_eq!(plan.paths, ["src/component.tsx", "src/nested/panel.tsx"]);
    assert!(!plan.needs_scope_paths());
}

#[test]
fn import_fallback_scans_scope_for_relative_module_queries() {
    let request = request("import \"./protocol\"", CodeQueryKind::Imports, Vec::new());
    let mut import_hit = hit(
        "src/provider.ts",
        "import { sendEnvelope } from \"./protocol\";",
    );
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("resolved".to_owned());
    import_hit.edge_target_hint = Some("src/protocol.ts".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
    let mut results = vec![import_hit];

    let plan = plan_code_grep_fallback(&status(), &request, &results)
        .expect("relative import queries should scan indexed source text");

    assert_eq!(plan.query, "./protocol");
    assert!(plan.paths.is_empty());
    assert!(plan.needs_scope_paths());

    let reason = append_code_grep_fallback(
        &status(),
        &request,
        &mut results,
        &plan,
        SourceGrepOutcome {
            matches: vec![SourceGrepMatch {
                path: "src/index.ts".to_owned(),
                language_id: "typescript".to_owned(),
                excerpt: "export type { StreamEnvelope } from \"./protocol\";".to_owned(),
                byte_range: RepositoryCodeRange { start: 0, end: 47 },
                line_range: RepositoryCodeRange { start: 1, end: 1 },
            }],
            degraded_reason: None,
        },
    );

    assert!(reason.is_none());
    assert!(results.iter().any(|hit| {
        hit.path == "src/index.ts"
            && hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::TextFallback)
    }));
}

#[test]
fn import_fallback_ranks_dynamic_import_source_lines_before_static_text_echoes() {
    let request = request(
        "await import(\"./protocol\")",
        CodeQueryKind::Imports,
        Vec::new(),
    );
    let mut graph_hit = hit("src/provider.ts", "import \"./protocol\" target symbols");
    graph_hit.score = 3.75;
    graph_hit.edge_kind = Some("import".to_owned());
    graph_hit.edge_resolution_state = Some("resolved".to_owned());
    graph_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
    let mut results = vec![graph_hit];
    let plan = CodeGrepFallbackPlan {
        commit: "commit".to_owned(),
        query: "./protocol".to_owned(),
        paths: Vec::new(),
        path_filters: Vec::new(),
        language_filters: vec!["typescript".to_owned()],
        limit: 10,
        kind: SourceGrepKind::Imports,
        identity: None,
        needs_scope_paths: false,
    };

    append_code_grep_fallback(
        &status(),
        &request,
        &mut results,
        &plan,
        SourceGrepOutcome {
            matches: vec![
                SourceGrepMatch {
                    path: "src/provider.ts".to_owned(),
                    language_id: "typescript".to_owned(),
                    excerpt: "import { sendEnvelope } from \"./protocol\";".to_owned(),
                    byte_range: RepositoryCodeRange { start: 0, end: 41 },
                    line_range: RepositoryCodeRange { start: 3, end: 3 },
                },
                SourceGrepMatch {
                    path: "src/provider.ts".to_owned(),
                    language_id: "typescript".to_owned(),
                    excerpt: "await import(\"./protocol\");".to_owned(),
                    byte_range: RepositoryCodeRange {
                        start: 100,
                        end: 127,
                    },
                    line_range: RepositoryCodeRange { start: 8, end: 8 },
                },
                SourceGrepMatch {
                    path: "src/provider.ts".to_owned(),
                    language_id: "typescript".to_owned(),
                    excerpt: "// TODO: remove import(\"./protocol\")".to_owned(),
                    byte_range: RepositoryCodeRange {
                        start: 130,
                        end: 166,
                    },
                    line_range: RepositoryCodeRange { start: 9, end: 9 },
                },
            ],
            degraded_reason: None,
        },
    );

    let graph_rank = results
        .iter()
        .position(|hit| {
            hit.retrieval_layers
                .contains(&CodeRetrievalLayer::ImportGraph)
        })
        .expect("graph hit should remain");
    let dynamic_rank = results
        .iter()
        .position(|hit| hit.excerpt.contains("await import"))
        .expect("dynamic source hit should be returned");
    let static_rank = results
        .iter()
        .position(|hit| hit.excerpt.starts_with("import {"))
        .expect("static source hit should be returned");
    let comment_rank = results
        .iter()
        .position(|hit| hit.excerpt.starts_with("// TODO"))
        .expect("comment source hit should be returned");

    assert!(dynamic_rank < graph_rank);
    assert!(graph_rank < static_rank);
    assert!(graph_rank < comment_rank);
    assert!(results[dynamic_rank].score > results[static_rank].score);
    assert_eq!(results[static_rank].score, results[comment_rank].score);
}

#[test]
fn import_fallback_treats_import_call_queries_as_dynamic_import_intent() {
    for query in [
        "import(\"./protocol\")",
        "await import(\"./protocol\")",
        "return import(\"./protocol\")",
        "const protocol = import(\"./protocol\")",
        "where is import(\"./protocol\") called from",
        "await import(\"./protocol\", { with: { type: \"json\" } })",
    ] {
        let request = request(query, CodeQueryKind::Imports, Vec::new());
        let mut graph_hit = hit("src/provider.ts", "import \"./protocol\" target symbols");
        graph_hit.score = 3.75;
        graph_hit.edge_kind = Some("import".to_owned());
        graph_hit.edge_resolution_state = Some("resolved".to_owned());
        graph_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
        let mut results = vec![graph_hit];
        let plan = CodeGrepFallbackPlan {
            commit: "commit".to_owned(),
            query: "./protocol".to_owned(),
            paths: Vec::new(),
            path_filters: Vec::new(),
            language_filters: vec!["typescript".to_owned()],
            limit: 10,
            kind: SourceGrepKind::Imports,
            identity: None,
            needs_scope_paths: false,
        };

        append_code_grep_fallback(
            &status(),
            &request,
            &mut results,
            &plan,
            SourceGrepOutcome {
                matches: vec![SourceGrepMatch {
                    path: "src/provider.ts".to_owned(),
                    language_id: "typescript".to_owned(),
                    excerpt: "await import(\"./protocol\");".to_owned(),
                    byte_range: RepositoryCodeRange {
                        start: 100,
                        end: 127,
                    },
                    line_range: RepositoryCodeRange { start: 8, end: 8 },
                }],
                degraded_reason: None,
            },
        );

        let graph_rank = results
            .iter()
            .position(|hit| {
                hit.retrieval_layers
                    .contains(&CodeRetrievalLayer::ImportGraph)
            })
            .expect("graph hit should remain");
        let dynamic_rank = results
            .iter()
            .position(|hit| hit.excerpt.contains("await import"))
            .expect("dynamic source hit should be returned");

        assert!(dynamic_rank < graph_rank, "{query}");
        assert!(results[dynamic_rank].score > results[graph_rank].score);
    }
}

#[test]
fn import_fallback_keeps_graph_imports_before_dynamic_text_for_non_dynamic_queries() {
    for query in ["./protocol", "import \"./protocol\""] {
        let request = request(query, CodeQueryKind::Imports, Vec::new());
        let mut graph_hit = hit(
            "src/provider.ts",
            "import type { StreamEnvelope } from \"./protocol\";",
        );
        graph_hit.score = 2.25;
        graph_hit.edge_kind = Some("import".to_owned());
        graph_hit.edge_resolution_state = Some("resolved".to_owned());
        graph_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
        let mut results = vec![graph_hit];
        let plan = CodeGrepFallbackPlan {
            commit: "commit".to_owned(),
            query: "./protocol".to_owned(),
            paths: Vec::new(),
            path_filters: Vec::new(),
            language_filters: vec!["typescript".to_owned()],
            limit: 10,
            kind: SourceGrepKind::Imports,
            identity: None,
            needs_scope_paths: false,
        };

        append_code_grep_fallback(
            &status(),
            &request,
            &mut results,
            &plan,
            SourceGrepOutcome {
                matches: vec![SourceGrepMatch {
                    path: "src/provider.ts".to_owned(),
                    language_id: "typescript".to_owned(),
                    excerpt: "await import(\"./protocol\");".to_owned(),
                    byte_range: RepositoryCodeRange {
                        start: 100,
                        end: 127,
                    },
                    line_range: RepositoryCodeRange { start: 8, end: 8 },
                }],
                degraded_reason: None,
            },
        );

        assert!(
            results[0]
                .retrieval_layers
                .contains(&CodeRetrievalLayer::ImportGraph),
            "non-dynamic import query should keep graph import evidence first for {query}: {results:?}",
        );
        let dynamic = results
            .iter()
            .find(|hit| hit.excerpt.contains("await import"))
            .expect("dynamic source fallback should still be retained");
        assert!(dynamic.score < results[0].score, "{query}");
    }
}

#[test]
fn import_fallback_keeps_graph_evidence_ahead_of_text_fallback() {
    let mut request = request("ProviderShared", CodeQueryKind::Imports, Vec::new());
    request.limit = 1;
    let mut import_hit = hit("src/component.tsx", "react");
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("unresolved".to_owned());
    import_hit.edge_target_hint = Some("react".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
    let plan = plan_code_grep_fallback(&status(), &request, &[import_hit.clone()])
        .expect("unresolved import should plan source fallback");
    let mut results = vec![import_hit];
    let outcome = SourceGrepOutcome {
        matches: vec![SourceGrepMatch {
            path: "src/component.tsx".to_owned(),
            language_id: "tsx".to_owned(),
            excerpt: "import React from \"react\";".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 26 },
            line_range: RepositoryCodeRange { start: 1, end: 1 },
        }],
        degraded_reason: None,
    };

    append_code_grep_fallback(&status(), &request, &mut results, &plan, outcome);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].excerpt, "react");
    assert_eq!(
        results[0].edge_resolution_state.as_deref(),
        Some("unresolved")
    );
    assert!(
        results[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::ImportGraph)
    );
}

#[test]
fn import_fallback_skips_empty_import_results() {
    let request = request("react", CodeQueryKind::Imports, Vec::new());

    assert!(plan_code_grep_fallback(&status(), &request, &[]).is_none());
}

#[test]
fn import_fallback_skips_resolved_import_graph_hits() {
    let request = request("crate::local", CodeQueryKind::Imports, Vec::new());
    let mut import_hit = hit("src/lib.rs", "use crate::local;");
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("resolved".to_owned());
    import_hit.edge_target_hint = Some("crate::local".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];

    assert!(plan_code_grep_fallback(&status(), &request, &[import_hit]).is_none());
}

#[test]
fn import_fallback_skips_ambiguous_import_graph_hits() {
    let request = request("RetryPolicy", CodeQueryKind::Imports, Vec::new());
    let mut import_hit = hit("src/app.rs", "use app::RetryPolicy;");
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("ambiguous".to_owned());
    import_hit.edge_target_hint = Some("app::RetryPolicy".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];

    assert!(plan_code_grep_fallback(&status(), &request, &[import_hit]).is_none());
}

#[test]
fn import_fallback_skips_local_unresolved_import_graph_hits() {
    let request = request("crate::local", CodeQueryKind::Imports, Vec::new());
    let mut import_hit = hit("src/lib.rs", "use crate::local;");
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("unresolved".to_owned());
    import_hit.edge_target_hint = Some("use crate::local;".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];

    assert!(plan_code_grep_fallback(&status(), &request, &[import_hit]).is_none());
}

#[test]
fn import_fallback_skips_dot_prefixed_local_unresolved_import_graph_hits() {
    let request = request(".pkg", CodeQueryKind::Imports, Vec::new());
    let mut import_hit = hit("pkg/app.py", "from .pkg import service");
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("unresolved".to_owned());
    import_hit.edge_target_hint = Some("from .pkg import service".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];

    assert!(plan_code_grep_fallback(&status(), &request, &[import_hit]).is_none());
}

#[test]
fn reference_grep_fallback_ranks_usage_before_array_declaration() {
    let request = request("rk_pipeline", CodeQueryKind::References, Vec::new());
    let mut results = vec![hit("src/pipeline.c", "int rk_dispatch(void);")];
    let plan = CodeGrepFallbackPlan {
        commit: "commit".to_owned(),
        query: "rk_pipeline".to_owned(),
        paths: Vec::new(),
        path_filters: Vec::new(),
        language_filters: vec!["c".to_owned()],
        limit: 10,
        kind: SourceGrepKind::References,
        identity: None,
        needs_scope_paths: false,
    };
    let outcome = SourceGrepOutcome {
        matches: vec![
            SourceGrepMatch {
                path: "src/pipeline.c".to_owned(),
                language_id: "c".to_owned(),
                excerpt: "static rk_stage_fn rk_pipeline[] = {".to_owned(),
                byte_range: RepositoryCodeRange { start: 10, end: 48 },
                line_range: RepositoryCodeRange { start: 4, end: 4 },
            },
            SourceGrepMatch {
                path: "src/pipeline.c".to_owned(),
                language_id: "c".to_owned(),
                excerpt: "total += rk_pipeline[index](dev);".to_owned(),
                byte_range: RepositoryCodeRange {
                    start: 90,
                    end: 123,
                },
                line_range: RepositoryCodeRange { start: 9, end: 9 },
            },
        ],
        degraded_reason: Some("source fallback".to_owned()),
    };

    append_code_grep_fallback(&status(), &request, &mut results, &plan, outcome);

    let usage_rank = results
        .iter()
        .position(|hit| hit.excerpt.contains("rk_pipeline[index]"))
        .expect("usage fallback should be returned");
    let declaration_rank = results
        .iter()
        .position(|hit| hit.excerpt.contains("rk_pipeline[]"))
        .expect("declaration fallback should be returned");
    assert!(usage_rank < declaration_rank);
    assert!(results[usage_rank].score > results[declaration_rank].score);
}

#[test]
fn reference_grep_fallback_keeps_assignment_values_at_base_score() {
    assert_eq!(
        reference_source_grep_score_adjustment("rk_driver_read", ".read = rk_driver_read,"),
        0.0
    );
    assert_eq!(
        reference_source_grep_score_adjustment("rk_driver_read", "return rk_driver_read;"),
        0.0
    );
}

fn request(query: &str, kind: CodeQueryKind, path_filters: Vec<String>) -> CodeRetrievalRequest {
    let selector = crate::domain::CodeRepositorySelector::new(
        "repo",
        "commit",
        path_filters,
        vec!["c".to_owned()],
    )
    .expect("selector should validate");
    CodeRetrievalRequest::new(
        query,
        selector,
        kind,
        10,
        crate::domain::FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}

fn status() -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "repo".to_owned(),
        root_path: "/tmp/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some(code_snapshot_scope_id("repo", "tree", &[], &[])),
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        state: "fresh".to_owned(),
        indexed_file_count: 1,
        symbol_count: 1,
        reference_count: 0,
        chunk_count: 1,
        stale: false,
        degraded_reason: None,
    }
}

fn hit(path: &str, excerpt: &str) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: path.to_owned(),
        language_id: "c".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 1 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: Some("symbol".to_owned()),
        canonical_symbol_id: Some("repo://repo/include::driver_ops::rk_driver_ops".to_owned()),
        file_id: Some("file".to_owned()),
        retrieval_layers: vec![CodeRetrievalLayer::Lexical],
        index_versions: vec!["code:scope:tree".to_owned()],
        stale: false,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 2.0,
        excerpt: excerpt.to_owned(),
    }
}
