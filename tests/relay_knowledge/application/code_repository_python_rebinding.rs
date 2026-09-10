//! Python decorator bindings use namespace writes, not matching property names.
use super::*;
#[tokio::test]
async fn redirected_python_bindings_and_unknown_external_receivers_preserve_runtime_facts() {
    let repo = FixtureRepo::create("python-rebinding");
    repo.write(
        "src/sample.py",
        r#"from types import SimpleNamespace
registry=SimpleNamespace()
from typing import overload, get_overloads
from typing import overload as global_ov
events=[]
def custom(fn): events.append(fn.__qualname__); return fn
def leaf(): return 1
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
registry=SimpleNamespace()
import typing as property_module
import typing as subscript_module
import typing as mutation_module
from typing import get_overloads
from types import SimpleNamespace
events=[]
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
        // The property/subscript cases follow writes through an external
        // SimpleNamespace constructor without authorized receiver-origin proof.
        // Their local execution is harmless, but those earlier unknown effects
        // prevent declaration-only evidence just as actual custom bindings do.
        let error = result.expect_err(name);
        assert_eq!(error.error_kind, ErrorKind::InvalidArgument, "{name}");
        assert!(
            error.message.contains("multiple definitions"),
            "{name}: {error:?}"
        );
    }
}

#[tokio::test]
async fn python_import_fallback_and_module_members_round_trip_through_the_call_graph() {
    let repo = FixtureRepo::create("python-import-paths");
    repo.write(
        "src/members.py",
        r#"marker_one=object()
marker_two=object()
from types import SimpleNamespace
registry=SimpleNamespace()
import typing as property_module
import typing as subscript_module
import typing as mutation_module
from typing import get_overloads
from types import SimpleNamespace
events=[]
def custom(fn): events.append(fn.__qualname__); return fn
def leaf(): return 1
property_module.unrelated_marker=marker_one
@property_module.overload
def attribute_choice(x:int): ...
def attribute_choice(x): return leaf()
mapping={}
subscript_module.__dict__["unrelated_marker"]=marker_two
@subscript_module.overload
def subscript_choice(x:int): ...
def subscript_choice(x): return leaf()
mutation_module.__dict__["overload"]=custom
@mutation_module.overload
def mutation_choice(): return leaf()
def mutation_choice(): return leaf()
"#,
    );
    repo.write(
        "src/fallback.py",
        r#"try:
    from typing import overload
except ImportError:
    from typing_extensions import overload
from typing import get_overloads
def leaf(): return 1
@overload
def fallback_choice(x:int): ...
def fallback_choice(x): return leaf()
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Python import paths"]);
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
            context("index-python-imports"),
        )
        .await
        .unwrap();
    for name in [
        "attribute_choice",
        "subscript_choice",
        "mutation_choice",
        "fallback_choice",
    ] {
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
                context("python-imports-query"),
            )
            .await;
        if matches!(name, "mutation_choice" | "subscript_choice") {
            // The independent receiver proof for the earlier property-module
            // write stops at the intervening external import. The dictionary
            // operation itself has a separate positive real-Git control.
            let error = result.expect_err(name);
            assert_eq!(error.error_kind, ErrorKind::InvalidArgument, "{name}");
            assert!(
                error.message.contains("multiple definitions"),
                "{name}: {error:?}"
            );
        } else {
            let response = result.unwrap();
            assert_eq!(response.results.len(), 1);
            assert!(
                response.results[0]
                    .canonical_symbol_id
                    .as_deref()
                    .unwrap()
                    .ends_with("::leaf")
            );
        }
    }
}

#[tokio::test]
async fn python_later_imports_resolve_before_closure_execution_without_ignoring_rebindings() {
    let repo = FixtureRepo::create("python-later-import");
    repo.write(
        "src/late_alias.py",
        r#"def outer():
 def inner():
  @ov
  def late_alias(x:int): ...
  def late_alias(x): return x
  return late_alias
 from typing import overload as ov
 return inner
result=outer()()
"#,
    );
    repo.write(
        "src/custom_then_import.py",
        r#"def outer():
 def inner():
  @ov
  def custom_then_import(x:int): ...
  def custom_then_import(x): return x
  return custom_then_import
 ov=lambda fn: fn
 from typing import overload as ov
 return inner
result=outer()()
"#,
    );
    repo.write(
        "src/import_then_custom.py",
        r#"def outer():
 def inner():
  @ov
  def import_then_custom(x:int): ...
  def import_then_custom(x): return x
  return import_then_custom
 from typing import overload as ov
 ov=lambda fn: fn
 return inner
result=outer()()
"#,
    );
    repo.write(
        "src/call_before_import.py",
        r#"def outer():
 def inner():
  @ov
  def call_before_import(x:int): ...
  def call_before_import(x): return x
  return call_before_import
 result=inner()
 from typing import overload as ov
 return result
result=outer()
"#,
    );
    repo.write(
        "src/after_return.py",
        r#"def outer():
 def inner():
  @ov
  def after_return(x:int): ...
  def after_return(x): return x
  return after_return
 return inner
 from typing import overload as ov
result=outer()()
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Later Python import order"]);
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
            context("index-later-import"),
        )
        .await
        .unwrap();
    for name in [
        "late_alias",
        "custom_then_import",
        "import_then_custom",
        "call_before_import",
        "after_return",
    ] {
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
                context("later-import-query"),
            )
            .await;
        if matches!(name, "late_alias" | "custom_then_import") {
            assert!(result.is_ok(), "{name}: {result:?}");
        } else {
            assert_eq!(result.unwrap_err().error_kind, ErrorKind::InvalidArgument);
        }
    }
}

#[tokio::test]
async fn python_mutator_calls_and_duplicate_aliases_do_not_hide_runtime_definitions() {
    let repo = FixtureRepo::create("python-mutation-aliases");
    repo.write(
        "src/sample.py",
        r#"import typing
import types
events=[]
def custom(fn): events.append(fn.__qualname__); return fn
def leaf(): return 1
setattr(typing, "overload", custom)
@typing.overload
def mutation_choice(): return leaf()
def mutation_choice(): return leaf()
types.overload=custom
import typing as tm, types as tm
@tm.overload
def attribute_choice(): return leaf()
def attribute_choice(): return leaf()
from typing import overload as ov, no_type_check as ov
@ov
def from_choice(): return leaf()
def from_choice(): return leaf()
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Python mutation and import order"]);
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
            context("index-module-mutation"),
        )
        .await
        .unwrap();
    for name in ["mutation_choice", "attribute_choice", "from_choice"] {
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
        let error = service
            .query_code_repository(
                CodeRetrievalRequest::new(
                    canonical,
                    selector("fixture", "HEAD"),
                    CodeQueryKind::Callees,
                    10,
                    FreshnessPolicy::AllowStale,
                )
                .unwrap(),
                context("query-module-mutation"),
            )
            .await
            .unwrap_err();
        assert_eq!(error.error_kind, ErrorKind::InvalidArgument);
    }
}
