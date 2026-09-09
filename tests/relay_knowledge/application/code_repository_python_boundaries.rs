//! Real Git retains eager execution, local binding, and finalizer proofs.
use super::*;
#[tokio::test]
async fn python_statement_boundaries_survive_real_git_indexing() {
    let repo = FixtureRepo::create("python-statement-boundaries");
    let cases = [
        (
            "module_unknown_call",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def mutate():
 global overload
 overload = custom
mutate()
@overload
def module_unknown_call(x: int): return leaf()
def module_unknown_call(x): return leaf()
"#,
            false,
        ),
        (
            "class_unknown_call",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def mutate():
 global overload
 overload = custom
class Container:
 mutate()
 @overload
 def class_unknown_call(x: int): return leaf()
 def class_unknown_call(x): return leaf()
class_unknown_call = Container.class_unknown_call
"#,
            false,
        ),
        (
            "later_local_assignment",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def factory():
 @overload
 def later_local_assignment(x: int): return leaf()
 def later_local_assignment(x): return leaf()
 overload = custom
 return later_local_assignment
result = factory()
"#,
            false,
        ),
        (
            "later_local_import",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def factory():
 @overload
 def later_local_import(x: int): return leaf()
 def later_local_import(x): return leaf()
 from typing import overload
 return later_local_import
result = factory()
"#,
            false,
        ),
        (
            "for_attribute_target",
            r#"class Registry: pass
registry = Registry()
from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
for registry.overload in [custom]:
 pass
@overload
def for_attribute_target(x: int): return leaf()
def for_attribute_target(x): return leaf()
"#,
            true,
        ),
        (
            "with_attribute_target",
            r#"class Registry: pass
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
def with_attribute_target(x: int): return leaf()
def with_attribute_target(x): return leaf()
"#,
            true,
        ),
        (
            "definition_default",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def helper(value=(overload := custom)): pass
@overload
def definition_default(x: int): return leaf()
def definition_default(x): return leaf()
"#,
            false,
        ),
        (
            "definition_decorator",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
@(overload := custom)
def helper(): pass
@overload
def definition_decorator(x: int): return leaf()
def definition_decorator(x): return leaf()
"#,
            false,
        ),
        (
            "class_base_expression",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
class Helper((overload := custom, object)[1]): pass
@overload
def class_base_expression(x: int): return leaf()
def class_base_expression(x): return leaf()
"#,
            false,
        ),
        (
            "future_annotation",
            r#"from __future__ import annotations
import typing
from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
marker: setattr(typing, "overload", custom)
@typing.overload
def future_annotation(x: int): return leaf()
def future_annotation(x): return leaf()
"#,
            true,
        ),
        (
            "finally_import",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
try:
 overload = custom
finally:
 from typing import overload
@overload
def finally_import(x: int): return leaf()
def finally_import(x): return leaf()
"#,
            true,
        ),
        (
            "finally_custom",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
try:
 from typing import overload
finally:
 overload = custom
@overload
def finally_custom(x: int): return leaf()
def finally_custom(x): return leaf()
"#,
            false,
        ),
        (
            "deferred_definition_body",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def helper():
 global overload
 overload = custom
@overload
def deferred_definition_body(x: int): return leaf()
def deferred_definition_body(x): return leaf()
"#,
            true,
        ),
        (
            "for_name_target",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
for overload in [custom]:
 pass
@overload
def for_name_target(x: int): return leaf()
def for_name_target(x): return leaf()
"#,
            false,
        ),
        (
            "comprehension_local",
            r#"from typing import overload
sink=[overload for overload in []]
def leaf(): return 1
@overload
def comprehension_local(x:int): ...
def comprehension_local(x): return leaf()
"#,
            true,
        ),
        (
            "comprehension_walrus",
            r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
sink=[(overload:=custom) for item in [1]]
@overload
def comprehension_walrus(x:int): ...
def comprehension_walrus(x): return leaf()
"#,
            false,
        ),
        (
            "decorator_implicit_call",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def alter(fn):
 global overload
 overload = custom
 return fn
@alter
def helper(): pass
@overload
def decorator_implicit_call(x: int): return leaf()
def decorator_implicit_call(x): return leaf()
"#,
            false,
        ),
        (
            "class_body_global_write",
            r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
class Helper:
 global overload
 overload = custom
@overload
def class_body_global_write(x: int): return leaf()
def class_body_global_write(x): return leaf()
"#,
            false,
        ),
        (
            "class_local_control",
            r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
class Helper:
 overload=custom
@overload
def class_local_control(x:int): ...
def class_local_control(x): return leaf()
"#,
            true,
        ),
        (
            "class_method_deferred_control",
            r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
class Helper:
 def alter(self):
  global overload
  overload=custom
@overload
def class_method_deferred_control(x:int): ...
def class_method_deferred_control(x): return leaf()
"#,
            true,
        ),
        (
            "shadow_setattr",
            r#"import typing
def custom(fn): return fn
def leaf(): return 1
def setattr(obj,key,value):
 obj.overload=custom
setattr(typing,"other",None)
@typing.overload
def shadow_setattr(x:int): ...
def shadow_setattr(x): return leaf()
"#,
            false,
        ),
        (
            "shadow_delattr",
            r#"import typing
def custom(fn): return fn
def leaf(): return 1
def delattr(obj,key):
 obj.overload=custom
delattr(typing,"other")
@typing.overload
def shadow_delattr(x:int): ...
def shadow_delattr(x): return leaf()
"#,
            false,
        ),
        (
            "class_module_local",
            r#"import typing
def leaf(): return 1
class Helper:
 typing=None
@typing.overload
def class_module_local(x:int): ...
def class_module_local(x): return leaf()
"#,
            true,
        ),
        (
            "mutator_parameter_annotation",
            r#"import typing
def leaf(): return 1
def helper(value: setattr): pass
setattr(typing,"other",None)
@typing.overload
def mutator_parameter_annotation(x:int): ...
def mutator_parameter_annotation(x): return leaf()
"#,
            true,
        ),
        (
            "class_shadow_decorator",
            r#"import typing
def custom(fn): return fn
def alter(fn):
 typing.overload=custom
 return fn
def leaf(): return 1
class Fake:
 overload=alter
class Helper:
 typing=Fake
 @typing.overload
 def method(): pass
@typing.overload
def class_shadow_decorator(x:int): ...
def class_shadow_decorator(x): return leaf()
"#,
            false,
        ),
        (
            "class_typing_decorator",
            r#"import typing
def leaf(): return 1
class Helper:
 @typing.overload
 def method(self,x:int): ...
 def method(self,x): return x
@typing.overload
def class_typing_decorator(x:int): ...
def class_typing_decorator(x): return leaf()
"#,
            true,
        ),
    ];
    for (name, source, _) in cases {
        repo.write(&format!("src/{name}.py"), source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Python binding target and syntax cases"]);
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
