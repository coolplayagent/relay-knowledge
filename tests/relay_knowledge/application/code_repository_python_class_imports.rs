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
