//! Real Git preserves alias writes and implicit class creation effects.
use super::*;
#[tokio::test]
async fn python_alias_and_class_creation_boundaries_survive_git_indexing() {
    let repo = FixtureRepo::create("python-alias-creation");
    let cases = [
        (
            "alias_selected",
            r###"import typing
from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
alias = typing
alias.overload = custom
@typing.overload
def alias_selected(x: int): return leaf()
def alias_selected(x): return leaf()
"###,
            false,
        ),
        (
            "alias_other",
            r###"import typing
from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
alias = typing
alias.unrelated = custom
@typing.overload
def alias_other(x: int): return leaf()
def alias_other(x): return leaf()
"###,
            true,
        ),
        (
            "metaclass_effect",
            r###"import typing
from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
class Meta(type):
 def __new__(cls, name, bases, attrs):
  typing.overload = custom
  return type.__new__(cls, name, bases, attrs)
class Helper(metaclass=Meta): pass
@typing.overload
def metaclass_effect(x: int): return leaf()
def metaclass_effect(x): return leaf()
"###,
            false,
        ),
        (
            "subclass_effect",
            r###"import typing
from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
class Base:
 def __init_subclass__(cls):
  typing.overload = custom
class Helper(Base): pass
@typing.overload
def subclass_effect(x: int): return leaf()
def subclass_effect(x): return leaf()
"###,
            false,
        ),
        (
            "descriptor_effect",
            r###"import typing
from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
class Descriptor:
 def __set_name__(self, owner, name):
  typing.overload = custom
descriptor = Descriptor()
import typing
class Helper:
 value = descriptor
@typing.overload
def descriptor_effect(x: int): return leaf()
def descriptor_effect(x): return leaf()
"###,
            false,
        ),
        (
            "plain_class",
            r###"import typing
from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
class Helper:
 value = 1
 def method(self):
  typing.overload = custom
@typing.overload
def plain_class(x: int): return leaf()
def plain_class(x): return leaf()
"###,
            true,
        ),
        (
            "chain",
            r###"import typing
def custom(fn): return fn
a = b = typing
b.overload = custom
def leaf(): return 1
@typing.overload
def chain(x: int): ...
def chain(x): return leaf()
"###,
            false,
        ),
        (
            "imported_descriptor",
            r###"import typing
class Target:
    from hooks import descriptor
def leaf(): return 1
@typing.overload
def imported_descriptor(x: int): ...
def imported_descriptor(x): return leaf()
"###,
            false,
        ),
    ];
    for (name, source, _) in cases {
        repo.write(&format!("src/{name}.py"), source);
    }
    repo.write(
        "src/hooks.py",
        r###"import typing
def custom(fn): return fn
class Hook:
    def __set_name__(self, owner, name): typing.overload = custom
descriptor = Hook()
"###,
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
