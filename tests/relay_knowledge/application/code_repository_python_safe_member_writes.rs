//! Real Git preserves unrelated writes with positively proven receivers.
use super::*;

#[tokio::test]
async fn python_proven_receivers_preserve_unrelated_property_and_dictionary_writes() {
    let repo = FixtureRepo::create("python-proven-member-writes");
    let cases = [
        (
            "local_registry_property",
            r###"class Registry: pass
registry=Registry()
from typing import overload
def custom(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
registry.overload=custom
@overload
def local_registry_property(): return leaf()
saved=local_registry_property
def local_registry_property(): return final_leaf()
def outer(): return saved,local_registry_property
"###,
        ),
        (
            "module_dictionary_member",
            r###"import typing as provider
def custom(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
provider.__dict__["unrelated_marker"]=1
@provider.overload
def module_dictionary_member(): return leaf()
saved=module_dictionary_member
def module_dictionary_member(): return final_leaf()
def outer(): return saved,module_dictionary_member
"###,
        ),
    ];
    for (name, source) in cases {
        repo.write(&format!("src/{name}.py"), source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Proven unrelated member writes"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-proven-member-writes"),
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
            context("index-proven-member-writes"),
        )
        .await
        .unwrap();
    for (name, _) in cases {
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
                context("query-proven-member-writes"),
            )
            .await;
        let result = result.unwrap();
        assert_eq!(result.results.len(), 1, "{name}");
        assert!(
            result.results[0]
                .canonical_symbol_id
                .as_deref()
                .is_some_and(|id| id.ends_with("::final_leaf")),
            "{name}"
        );
    }
}
