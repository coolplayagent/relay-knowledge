//! Real Git preserves conditional alias and sibling decorator expression effects.
use super::*;
#[tokio::test]
async fn python_control_aliases_and_sibling_decorators_survive_git_indexing() {
    let repo = FixtureRepo::create("python-control-decorators");
    let cases = [
        (
            "sibling_comment",
            r###"import typing
def custom(fn): return fn
def leaf(): return 1
def make_mutator():
 typing.overload = custom
 return custom
@make_mutator()
# intervening comment
@typing.overload
def sibling_comment(x: int): return leaf()
def sibling_comment(x): return leaf()
"###,
            false,
        ),
        (
            "later_comment",
            r###"import typing
def custom(fn): return fn
def leaf(): return 1
def make_mutator():
 typing.overload = custom
 return custom
@typing.overload
# intervening comment
@make_mutator()
def later_comment(x: int): return leaf()
def later_comment(x): return leaf()
"###,
            true,
        ),
        (
            "conditional_alias",
            r###"import typing
def custom(fn): return fn
def leaf(): return 1
if True:
 alias = typing
alias.overload = custom
@typing.overload
def conditional_alias(x: int): return leaf()
def conditional_alias(x): return leaf()
"###,
            false,
        ),
        (
            "conditional_alias_other",
            r###"import typing
def custom(fn): return fn
def leaf(): return 1
if True:
 alias = typing
alias.other = custom
@typing.overload
def conditional_alias_other(x: int): return leaf()
def conditional_alias_other(x): return leaf()
"###,
            true,
        ),
        (
            "conditional_alias_nested",
            r###"import typing
def custom(fn): return fn
def leaf(): return 1
if True:
 if True:
  alias = typing
alias.overload = custom
@typing.overload
def conditional_alias_nested(x: int): return leaf()
def conditional_alias_nested(x): return leaf()
"###,
            false,
        ),
        (
            "sibling_expression",
            r###"import typing
def custom(fn): return fn
def leaf(): return 1
def make_mutator():
 typing.overload = custom
 return custom
@make_mutator()
@typing.overload
def sibling_expression(x: int): return leaf()
def sibling_expression(x): return leaf()
"###,
            false,
        ),
        (
            "sibling_later_expression",
            r###"import typing
def custom(fn): return fn
def leaf(): return 1
def make_mutator():
 typing.overload = custom
 return custom
@typing.overload
@make_mutator()
def sibling_later_expression(x: int): return leaf()
def sibling_later_expression(x): return leaf()
"###,
            true,
        ),
        (
            "sibling_reference",
            r###"import typing
def custom(fn): return fn
def leaf(): return 1
@custom
@typing.overload
def sibling_reference(x: int): return leaf()
def sibling_reference(x): return leaf()
"###,
            true,
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
