//! Real Git preserves literal managers and rejects executable context hooks.
use super::*;

#[tokio::test]
async fn python_literal_context_managers_preserve_unrelated_overloads() {
    let repo = FixtureRepo::create("python-literal-manager");
    let cases = [
        (
            "context_original",
            r###"class Registry: pass
class Manager:
 def __enter__(self): return None
 def __exit__(self, *args): return False
registry = Registry()
manager = Manager()
from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
with manager as registry.overload:
 pass
@overload
def context_original(x: int): return leaf()
def context_original(x): return leaf()
"###,
            true,
        ),
        (
            "context_renamed",
            r###"class Store: pass
class Scope:
 def __enter__(self): return None
 def __exit__(self, *args): return False
store = Store()
scope = Scope()
from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
with scope as store.overload:
 pass
@overload
def context_renamed(x: int): return leaf()
def context_renamed(x): return leaf()
"###,
            true,
        ),
        (
            "context_enter_hook",
            r###"class Registry: pass
class Manager:
 def __enter__(self):
  global overload
  overload = custom
  return None
 def __exit__(self, *args): return False
registry = Registry()
manager = Manager()
from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
with manager as registry.overload:
 pass
@overload
def context_enter_hook(x: int): return leaf()
def context_enter_hook(x): return leaf()
"###,
            false,
        ),
        (
            "context_exit_hook",
            r###"class Registry: pass
class Manager:
 def __enter__(self): return None
 def __exit__(self, *args):
  global overload
  overload = custom
  return False
registry = Registry()
manager = Manager()
from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
with manager as registry.overload:
 pass
@overload
def context_exit_hook(x: int): return leaf()
def context_exit_hook(x): return leaf()
"###,
            false,
        ),
        (
            "context_custom_setter",
            r###"class Registry:
 def __setattr__(self, name, value):
  global overload
  overload = custom
class Manager:
 def __enter__(self): return None
 def __exit__(self, *args): return False
registry = Registry()
manager = Manager()
from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
with manager as registry.overload:
 pass
@overload
def context_custom_setter(x: int): return leaf()
def context_custom_setter(x): return leaf()
"###,
            false,
        ),
    ];
    for (name, source, _) in cases {
        repo.write(&format!("src/{name}.py"), source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Literal context manager proof"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-literal-manager"),
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
            context("index-literal-manager"),
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
                context("query-literal-manager"),
            )
            .await;
        if declaration {
            let result = result.unwrap();
            assert_eq!(result.results.len(), 1, "{name}");
            assert!(
                result.results[0]
                    .canonical_symbol_id
                    .as_deref()
                    .is_some_and(|id| id.ends_with("::leaf")),
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
