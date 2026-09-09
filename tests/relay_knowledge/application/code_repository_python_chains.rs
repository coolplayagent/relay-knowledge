//! Chained writes and transparent decorator syntax survive the real Git pipeline.
use super::*;
#[tokio::test]
async fn python_chained_targets_and_parenthesized_decorators_round_trip() {
    let repo = FixtureRepo::create("python-chain-parens");
    let cases = [
        (
            "comment_read",
            "from typing import overload\noverload and True",
            "(\n # comment\n overload\n)",
            true,
        ),
        (
            "chain_name",
            "from typing import overload\nother=overload=custom",
            "overload",
            false,
        ),
        (
            "chain_member",
            "import typing\nother=typing.overload=custom",
            "typing.overload",
            false,
        ),
        (
            "chain_tuple",
            "from typing import overload\nsink=(other,overload)=(custom,custom)",
            "overload",
            false,
        ),
        (
            "chain_read",
            "from typing import overload\nfirst=second=overload",
            "overload",
            true,
        ),
        (
            "paren_name",
            "from typing import overload",
            "((overload))",
            true,
        ),
        ("paren_module", "import typing", "(typing.overload)", true),
        (
            "paren_receiver",
            "import typing",
            "((typing)).overload",
            true,
        ),
        ("paren_custom", "overload=custom", "(overload)", false),
    ];
    for (name, prefix, decorator, _) in cases {
        repo.write(&format!("src/{name}.py"), &format!("def custom(fn): return fn\ndef leaf(): return 1\n{prefix}\n@{decorator}\ndef {name}(x:int): ...\ndef {name}(x): return leaf()\n"));
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
    for (name, _, _, typed) in cases {
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

#[tokio::test]
async fn python_expression_writes_and_comment_gaps_round_trip() {
    let repo = FixtureRepo::create("python-expression-writes");
    let cases = [
        (
            "walrus",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
(overload := custom)
@overload
def walrus(x:int): ...
def walrus(x): return leaf()
"#,
            false,
        ),
        (
            "nested_walrus",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
sink=(0,(overload := custom))
@overload
def nested_walrus(x:int): ...
def nested_walrus(x): return leaf()
"#,
            false,
        ),
        (
            "paren_member",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
import typing
(typing).overload=custom
@typing.overload
def paren_member(x:int): ...
def paren_member(x): return leaf()
"#,
            false,
        ),
        (
            "paren_other",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
import typing
(typing).other=custom
@typing.overload
def paren_other(x:int): ...
def paren_other(x): return leaf()
"#,
            true,
        ),
        (
            "deferred_walrus",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
sink=lambda:(overload := custom)
@overload
def deferred_walrus(x:int): ...
def deferred_walrus(x): return leaf()
"#,
            true,
        ),
        (
            "comment_import",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
def factory():
 global overload
 @overload
 def comment_import(x:int): ...
 def comment_import(x): return leaf()
 return comment_import
# no operation
from typing import overload
comment_import=factory()
"#,
            true,
        ),
        (
            "comment_custom",
            r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
def factory():
 global overload
 @overload
 def comment_custom(x:int): ...
 def comment_custom(x): return leaf()
 return comment_custom
# no operation
overload=custom
comment_custom=factory()
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
