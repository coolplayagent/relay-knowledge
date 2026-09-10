//! Real Git keeps independent standard-module reads separate from dynamic dispatch.
use super::*;

#[tokio::test]
async fn python_independent_module_attributes_preserve_imported_decorators() {
    let repo = FixtureRepo::create("python-independent-module");
    let cases = [
        (
            "bare_default",
            r###"from typing import overload as ov
import typing
def custom(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
def outer(value=ov):
    @ov
    def bare_default(): return leaf()
    saved = bare_default
    def bare_default(): return final_leaf()
    return saved, bare_default
"###,
            true,
        ),
        (
            "preceding_annotation",
            r###"from typing import overload as ov
import typing
def custom(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
def annotated(value: typing.Any): pass
def outer(value=ov):
    @ov
    def preceding_annotation(): return leaf()
    saved = preceding_annotation
    def preceding_annotation(): return final_leaf()
    return saved, preceding_annotation
"###,
            true,
        ),
        (
            "preceding_alias_annotation",
            r###"from typing import overload as ov
import typing
def custom(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
import typing as types_module
def annotated(value: types_module.Any): pass
def outer(value=ov):
    @ov
    def preceding_alias_annotation(): return leaf()
    saved = preceding_alias_annotation
    def preceding_alias_annotation(): return final_leaf()
    return saved, preceding_alias_annotation
"###,
            true,
        ),
        (
            "unknown_attribute",
            r###"from typing import overload as ov
import typing
def custom(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
class Provider:
    def __getattribute__(self, name):
        global ov
        ov = custom
        return object
typing = Provider()
value = typing.Any
def outer(value=ov):
    @ov
    def unknown_attribute(): return leaf()
    saved = unknown_attribute
    def unknown_attribute(): return final_leaf()
    return saved, unknown_attribute
"###,
            false,
        ),
        (
            "reassigned_module",
            r###"from typing import overload as ov
import typing
def custom(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
typing = custom
def outer(value=ov):
    @ov
    def reassigned_module(): return leaf()
    saved = reassigned_module
    def reassigned_module(): return final_leaf()
    return saved, reassigned_module
"###,
            true,
        ),
        (
            "missing_attribute_hook",
            r###"from typing import overload as ov
import typing
def custom(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
saved_hook = typing.__getattr__
def lookup(name):
    global ov
    ov = custom
    return object
typing.__getattr__ = lookup
value = typing.absent_runtime_member
typing.__getattr__ = saved_hook
def outer(value=ov):
    @ov
    def missing_attribute_hook(): return leaf()
    saved = missing_attribute_hook
    def missing_attribute_hook(): return final_leaf()
    return saved, missing_attribute_hook
"###,
            false,
        ),
        (
            "module_class_hook",
            r###"from typing import overload as ov
import typing
def custom(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
import types
class ModuleHook(types.ModuleType):
    def __getattribute__(self, name):
        global ov
        ov = custom
        return super().__getattribute__(name)
typing.__class__ = ModuleHook
value = typing.Any
typing.__class__ = types.ModuleType
def outer(value=ov):
    @ov
    def module_class_hook(): return leaf()
    saved = module_class_hook
    def module_class_hook(): return final_leaf()
    return saved, module_class_hook
"###,
            false,
        ),
        (
            "changed_module_alias",
            r###"from typing import overload as ov
import typing
def custom(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
class Provider:
    def __getattribute__(self, name):
        global ov
        ov = custom
        return object
typing = Provider()
alias = typing
value = alias.Any
def outer(value=ov):
    @ov
    def changed_module_alias(): return leaf()
    saved = changed_module_alias
    def changed_module_alias(): return final_leaf()
    return saved, changed_module_alias
"###,
            false,
        ),
    ];
    for (name, source, _) in cases {
        repo.write(&format!("src/{name}.py"), source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Independent module receiver proof"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-independent-module"),
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
            context("index-independent-module"),
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
                context("query-independent-module"),
            )
            .await;
        if declaration {
            let result = result.unwrap();
            assert_eq!(result.results.len(), 1, "{name}");
            assert!(
                result.results[0]
                    .canonical_symbol_id
                    .as_deref()
                    .is_some_and(|id| id.ends_with("::final_leaf")),
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
