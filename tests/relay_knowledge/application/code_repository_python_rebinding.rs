//! Python decorator bindings use namespace writes, not matching property names.
use super::*;
#[tokio::test]
async fn redirected_python_bindings_reject_later_custom_values_but_ignore_properties() {
    let repo = FixtureRepo::create("python-rebinding");
    repo.write(
        "src/sample.py",
        r#"from typing import overload, get_overloads
from typing import overload as global_ov
from types import SimpleNamespace
events=[]
def custom(fn): events.append(fn.__qualname__); return fn
def leaf(): return 1
registry=SimpleNamespace()
registry.overload=custom
@overload
def attribute_choice(x:int): ...
def attribute_choice(x): return leaf()
def global_outer():
    global global_ov
    @global_ov
    def global_choice(): return leaf()
    def global_choice(): return leaf()
    return global_choice
def nonlocal_outer():
    from typing import overload
    def inner():
        nonlocal overload
        @overload
        def nonlocal_choice(): return leaf()
        def nonlocal_choice(): return leaf()
        return nonlocal_choice
    overload=custom
    return inner()
def implicit_global_outer():
    @global_ov
    def implicit_global_choice(): return leaf()
    def implicit_global_choice(): return leaf()
    return implicit_global_choice
def implicit_nonlocal_outer():
    from typing import overload
    def inner():
        @overload
        def implicit_nonlocal_choice(): return leaf()
        def implicit_nonlocal_choice(): return leaf()
        return implicit_nonlocal_choice
    overload=custom
    return inner()
global_ov=custom
global_outer()
nonlocal_outer()
import typing as property_module
import typing as subscript_module
import typing as mutation_module
from typing import get_overloads
from types import SimpleNamespace
events=[]
registry=SimpleNamespace()
registry.property_module=custom
@property_module.overload
def module_property_choice(x:int): ...
def module_property_choice(x): return leaf()
mapping={}
mapping[subscript_module]=custom
@subscript_module.overload
def module_subscript_choice(x:int): ...
def module_subscript_choice(x): return leaf()
mutation_module.overload=custom
@mutation_module.overload
def module_mutation_choice(): return leaf()
def module_mutation_choice(): return leaf()
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Python namespace bindings"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-python-rebinding"),
        )
        .await
        .unwrap();
    for name in [
        "attribute_choice",
        "global_choice",
        "nonlocal_choice",
        "implicit_global_choice",
        "implicit_nonlocal_choice",
        "module_property_choice",
        "module_subscript_choice",
        "module_mutation_choice",
    ] {
        let definitions = query(&service, name, CodeQueryKind::Definition).await;
        let canonical = definitions
            .results
            .iter()
            .find_map(|hit| {
                hit.canonical_symbol_id.as_deref().filter(|id| {
                    id.ends_with(&format!("::{name}")) || id.ends_with(&format!(".{name}"))
                })
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
                context("python-rebinding-query"),
            )
            .await;
        if matches!(
            name,
            "attribute_choice" | "module_property_choice" | "module_subscript_choice"
        ) {
            let response = result.unwrap();
            assert_eq!(response.results.len(), 1);
            assert!(
                response.results[0]
                    .canonical_symbol_id
                    .as_deref()
                    .unwrap()
                    .ends_with("::leaf")
            );
        } else {
            assert_eq!(result.unwrap_err().error_kind, ErrorKind::InvalidArgument);
        }
    }
}
