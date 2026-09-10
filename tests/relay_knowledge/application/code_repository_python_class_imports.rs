//! Real Git preserves imports executed during class construction.
use super::*;
#[tokio::test]
async fn class_module_import_side_effects_survive_git_indexing() {
    let repo = FixtureRepo::create("python-class-import");
    let cases = [
        (
            "class_module_effect",
            "from typing import overload\nclass Helper:\n import side_effect\ndef leaf(): return 1\n@overload\ndef class_module_effect(x:int): ...\ndef class_module_effect(x): return leaf()\n",
            false,
        ),
        (
            "standard_class_import",
            "from typing import overload\nclass Helper:\n import typing\ndef leaf(): return 1\n@overload\ndef standard_class_import(x:int): ...\ndef standard_class_import(x): return leaf()\n",
            true,
        ),
        (
            "plain_class_control",
            "from typing import overload\nclass Helper:\n value=1\ndef leaf(): return 1\n@overload\ndef plain_class_control(x:int): ...\ndef plain_class_control(x): return leaf()\n",
            true,
        ),
    ];
    for (name, source, _) in cases {
        repo.write(&format!("src/{name}.py"), source);
    }
    repo.write(
        "src/side_effect.py",
        "import sys\ndef custom(fn): return fn\nsys.modules['__main__'].overload = custom\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Python binding target and syntax cases"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-full-origin-inventory"),
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
            context("index-chains"),
        )
        .await
        .unwrap();
    for (name, _, typed) in cases {
        let definitions = query(&service, name, CodeQueryKind::Definition).await;
        let canonical = definitions
            .results
            .iter()
            .find_map(|h| {
                h.canonical_symbol_id
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
                context("query-chains"),
            )
            .await;
        if typed {
            assert_eq!(result.unwrap().results.len(), 1, "{name}");
        } else {
            assert_eq!(
                result.unwrap_err().error_kind,
                ErrorKind::InvalidArgument,
                "{name}"
            );
        }
    }
}

#[tokio::test]
async fn redirected_class_definitions_preserve_real_canonical_ambiguity() {
    let repo = FixtureRepo::create("python-class-directed-definition");
    let cases = [
        (
            "nonlocal_function",
            "def leaf(): return 7\ndef final_leaf(): return 9\ndef outer():\n    from typing import overload\n    class Change:\n        nonlocal overload\n        def overload(fn): return fn\n    @overload\n    def nonlocal_function(): return leaf()\n    saved = nonlocal_function\n    def nonlocal_function(): return final_leaf()\n    return saved, nonlocal_function\n",
            false,
        ),
        (
            "nonlocal_assignment",
            "def leaf(): return 7\ndef final_leaf(): return 9\ndef outer():\n    from typing import overload\n    class Change:\n        nonlocal overload\n        overload = lambda fn: fn\n    @overload\n    def nonlocal_assignment(): return leaf()\n    saved = nonlocal_assignment\n    def nonlocal_assignment(): return final_leaf()\n    return saved, nonlocal_assignment\n",
            false,
        ),
        (
            "global_function",
            "def leaf(): return 7\ndef final_leaf(): return 9\nfrom typing import overload\ndef outer():\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def global_function(): return leaf()\n    saved = global_function\n    def global_function(): return final_leaf()\n    return saved, global_function\n",
            false,
        ),
        (
            "class_local_control",
            "def leaf(): return 7\ndef final_leaf(): return 9\ndef outer():\n    from typing import overload\n    class Change:\n        def overload(fn): return fn\n    @overload\n    def class_local_control(): return leaf()\n    saved = class_local_control\n    def class_local_control(): return final_leaf()\n    return saved, class_local_control\n",
            true,
        ),
        (
            "unrelated_nonlocal_control",
            "def leaf(): return 7\ndef final_leaf(): return 9\ndef outer():\n    from typing import overload\n    other = None\n    class Change:\n        nonlocal other\n        def other(fn): return fn\n    @overload\n    def unrelated_nonlocal_control(): return leaf()\n    saved = unrelated_nonlocal_control\n    def unrelated_nonlocal_control(): return final_leaf()\n    return saved, unrelated_nonlocal_control\n",
            true,
        ),
        (
            "deferred_nonlocal_control",
            "def leaf(): return 7\ndef final_leaf(): return 9\ndef outer():\n    from typing import overload\n    class Change:\n        def later():\n            nonlocal overload\n            overload = lambda fn: fn\n    @overload\n    def deferred_nonlocal_control(): return leaf()\n    saved = deferred_nonlocal_control\n    def deferred_nonlocal_control(): return final_leaf()\n    return saved, deferred_nonlocal_control\n",
            true,
        ),
        (
            "global_does_not_replace_local",
            "def leaf(): return 7\ndef final_leaf(): return 9\ndef outer():\n    from typing import overload\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def global_does_not_replace_local(): return leaf()\n    saved = global_does_not_replace_local\n    def global_does_not_replace_local(): return final_leaf()\n    return saved, global_does_not_replace_local\n",
            true,
        ),
    ];
    for (name, source, _) in cases {
        repo.write(&format!("src/{name}.py"), source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Redirected class definition bindings"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-class-directed-definition"),
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
            context("index-class-directed-definition"),
        )
        .await
        .unwrap();
    for (name, _, typed) in cases {
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
                context("query-class-directed-definition"),
            )
            .await;
        if typed {
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
            assert_eq!(
                result.unwrap_err().error_kind,
                ErrorKind::InvalidArgument,
                "{name}"
            );
        }
    }
}
