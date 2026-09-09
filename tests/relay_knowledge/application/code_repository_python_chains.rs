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
