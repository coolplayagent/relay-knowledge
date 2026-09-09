//! Runtime evaluation boundaries are retained through real repository indexing.
use super::*;
#[tokio::test]
async fn python_evaluation_boundaries_preserve_runtime_overload_identity() {
    let repo = FixtureRepo::create("python-evaluation");
    let cases = [
        (
            "lambda_default",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
sink=lambda value=(overload := custom): value
@overload
def lambda_default(x:int): ...
def lambda_default(x): return leaf()
"#,
            false,
        ),
        (
            "lambda_body",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
sink=lambda:(overload := custom)
@overload
def lambda_body(x:int): ...
def lambda_body(x): return leaf()
"#,
            true,
        ),
        (
            "generator_body",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
sink=((overload := custom) for item in [1])
@overload
def generator_body(x:int): ...
def generator_body(x): return leaf()
"#,
            true,
        ),
        (
            "generator_outer",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
import typing
sink=(item for item in (setattr(typing,"overload",custom),))
@typing.overload
def generator_outer(x:int): ...
def generator_outer(x): return leaf()
"#,
            false,
        ),
        (
            "list_body",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
sink=[(overload := custom) for item in [1]]
@overload
def list_body(x:int): ...
def list_body(x): return leaf()
"#,
            false,
        ),
        (
            "module_annotation",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
overload: object
@overload
def module_annotation(x:int): ...
def module_annotation(x): return leaf()
"#,
            true,
        ),
        (
            "class_annotation",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
class Box:
 from typing import overload
 overload: object
 @overload
 def class_annotation(x:int): ...
 def class_annotation(x): return leaf()
class_annotation=Box.class_annotation
"#,
            true,
        ),
        (
            "annotation_value",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
overload: object=custom
@overload
def annotation_value(x:int): ...
def annotation_value(x): return leaf()
"#,
            false,
        ),
        (
            "factory_call",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
def factory():
 global overload
 @overload
 def factory_call(x:int): ...
 def factory_call(x): return leaf()
 return factory_call
factory_call=factory()
overload=custom
"#,
            true,
        ),
        (
            "factory_later_import",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
def factory():
 global overload
 @overload
 def factory_later_import(x:int): ...
 def factory_later_import(x): return leaf()
 return factory_later_import
from typing import overload
factory_later_import=factory()
overload=custom
"#,
            true,
        ),
        (
            "factory_prior_custom",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
def factory():
 global overload
 @overload
 def factory_prior_custom(x:int): ...
 def factory_prior_custom(x): return leaf()
 return factory_prior_custom
overload=custom
factory_prior_custom=factory()
"#,
            false,
        ),
        (
            "generator_module_body",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
import typing
sink=(setattr(typing,"overload",custom) for item in [1])
@typing.overload
def generator_module_body(x:int): ...
def generator_module_body(x): return leaf()
"#,
            true,
        ),
        (
            "local_annotation",
            r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
def factory():
 overload: object
 @overload
 def local_annotation(x:int): ...
 def local_annotation(x): return leaf()
 return local_annotation
local_annotation=factory()
"#,
            false,
        ),
        (
            "factory_name_rebound",
            r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
def factory():
 global overload
 @overload
 def factory_name_rebound(x:int): ...
 def factory_name_rebound(x): return leaf()
 return factory_name_rebound
original_factory=factory
factory=lambda: None
factory()
overload=custom
factory_name_rebound=original_factory()
"#,
            false,
        ),
        (
            "factory_argument_write",
            r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
def factory(value):
 global overload
 @overload
 def factory_argument_write(x:int): ...
 def factory_argument_write(x): return leaf()
 return factory_argument_write
factory_argument_write=factory((overload:=custom))
"#,
            false,
        ),
        (
            "factory_other_call",
            r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
def mutate():
 global overload
 overload=custom
def factory():
 global overload
 @overload
 def factory_other_call(x:int): ...
 def factory_other_call(x): return leaf()
 return factory_other_call
mutate()
factory_other_call=factory()
"#,
            false,
        ),
    ];
    for (name, source, _) in cases {
        repo.write(&format!("src/{name}.py"), source);
    }
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
