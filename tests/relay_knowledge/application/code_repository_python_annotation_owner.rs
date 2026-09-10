//! Real Git preserves callable ambiguity after annotation-only class declarations.
use super::*;

#[tokio::test]
async fn class_annotation_only_names_preserve_redirected_callable_ambiguity() {
    let repo = FixtureRepo::create("python-annotation-owner");
    let cases = [
        (
            "class_only",
            "from typing import overload\ndef custom(fn): return fn\ndef leaf(): return 7\ndef final_leaf(): return 9\nclass Outer:\n    overload: object\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def class_only(): return leaf()\n    saved = class_only\n    def class_only(): return final_leaf()\n",
            false,
        ),
        (
            "class_import_control",
            "from typing import overload\ndef custom(fn): return fn\ndef leaf(): return 7\ndef final_leaf(): return 9\nclass Outer:\n    from typing import overload\n    overload: object\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def class_import_control(): return leaf()\n    saved = class_import_control\n    def class_import_control(): return final_leaf()\n",
            true,
        ),
        (
            "class_assigned_control",
            "from typing import overload\ndef custom(fn): return fn\ndef leaf(): return 7\ndef final_leaf(): return 9\nclass Outer:\n    overload: object = custom\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def class_assigned_control(): return leaf()\n    saved = class_assigned_control\n    def class_assigned_control(): return final_leaf()\n",
            false,
        ),
        (
            "class_only_unchanged_control",
            "from typing import overload\ndef custom(fn): return fn\ndef leaf(): return 7\ndef final_leaf(): return 9\nclass Outer:\n    overload: object\n    @overload\n    def class_only_unchanged_control(): return leaf()\n    saved = class_only_unchanged_control\n    def class_only_unchanged_control(): return final_leaf()\n",
            true,
        ),
        (
            "function_annotation_unbound",
            "from typing import overload\ndef custom(fn): return fn\ndef leaf(): return 7\ndef final_leaf(): return 9\ndef outer():\n    overload: object\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def function_annotation_unbound(): return leaf()\n    saved = function_annotation_unbound\n    def function_annotation_unbound(): return final_leaf()\n    return saved, function_annotation_unbound\n",
            false,
        ),
        (
            "function_assigned_control",
            "from typing import overload\ndef custom(fn): return fn\ndef leaf(): return 7\ndef final_leaf(): return 9\ndef outer():\n    overload: object = custom\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def function_assigned_control(): return leaf()\n    saved = function_assigned_control\n    def function_assigned_control(): return final_leaf()\n    return saved, function_assigned_control\n",
            false,
        ),
    ];
    for (name, source, _) in cases {
        repo.write(&format!("src/{name}.py"), source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Class annotation-only binding ownership"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-annotation-owner"),
        )
        .await
        .unwrap();
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-annotation-owner"),
        )
        .await
        .unwrap();
    for (name, _, declaration) in cases {
        let definitions = query(&service, name, CodeQueryKind::Definition).await;
        let canonical = definitions
            .results
            .iter()
            .find_map(|hit| {
                hit.canonical_symbol_id
                    .as_deref()
                    .filter(|id| id.ends_with(name))
            })
            .unwrap();
        let result = service
            .query_code_repository(
                CodeRetrievalRequest::new(
                    canonical,
                    selector("fixture", "HEAD"),
                    CodeQueryKind::Callees,
                    10,
                    FreshnessPolicy::AllowStale,
                )
                .unwrap(),
                context("query-annotation-owner"),
            )
            .await;
        if declaration {
            let result = result.unwrap();
            assert_eq!(result.results.len(), 1, "{name}");
            assert!(
                result.results[0]
                    .canonical_symbol_id
                    .as_deref()
                    .is_some_and(|id| id.ends_with("final_leaf")),
                "{name}"
            );
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.error_kind, ErrorKind::InvalidArgument, "{name}");
            assert!(
                error.message.contains("multiple definitions"),
                "{name}: {}",
                error.message
            );
        }
    }
}
