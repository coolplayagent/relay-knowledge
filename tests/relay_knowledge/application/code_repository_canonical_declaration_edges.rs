//! Real Git coverage for C-family declaration targets and exact snapshot selectors.
use super::*;

#[tokio::test]
async fn canonical_declaration_callers_preserve_signature_and_snapshot_boundaries() {
    for (case, declaration, definition, expression, matching) in [
        ("unnamed", "int", "int value", "3", true),
        ("renamed", "int old_name", "int new_name", "3", true),
        ("scalar_cv", "const int old_name", "int new_name", "3", true),
        ("type", "double value", "int value", "3.5", false),
        ("arity", "int first, int second", "int value", "3, 4", false),
        (
            "pointee_cv",
            "const int* value",
            "int* value",
            "nullptr",
            false,
        ),
    ] {
        let repo = FixtureRepo::create(&format!("canonical-declaration-{case}"));
        repo.write("src/foo.hpp", &format!("int helper({declaration});\ninline int header_caller() {{ return helper({expression}); }}\n"));
        repo.write(
            "src/foo.cpp",
            &format!("#include \"foo.hpp\"\nint helper({definition}) {{ return 7; }}\n"),
        );
        repo.git(["add", "."]);
        repo.git(["commit", "-m", "Declaration target evidence"]);
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
                context("index-declaration-targets"),
            )
            .await
            .unwrap();
        let definitions = query(&service, "helper", CodeQueryKind::Definition).await;
        let symbols = definitions
            .results
            .iter()
            .filter(|hit| {
                hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
                    && hit
                        .canonical_symbol_id
                        .as_deref()
                        .is_some_and(|id| id.ends_with("::helper"))
            })
            .collect::<Vec<_>>();
        assert_eq!(symbols.len(), 2, "{case}");
        let canonical = symbols[0].canonical_symbol_id.as_deref().unwrap();
        let callers = query(&service, canonical, CodeQueryKind::Callers).await;
        assert_eq!(
            callers.results.len(),
            usize::from(matching),
            "{case}: {:?}",
            callers.results
        );
        for symbol in symbols {
            let exact = query(
                &service,
                symbol.symbol_snapshot_id.as_deref().unwrap(),
                CodeQueryKind::Callers,
            )
            .await;
            assert_eq!(
                exact.results.len(),
                usize::from(symbol.path.ends_with(".hpp")),
                "{case}"
            );
        }
        for filter in [
            "path:foo.hpp",
            "name:header_caller",
            "path:foo.hpp name:header_caller",
        ] {
            assert_eq!(
                query(
                    &service,
                    &format!("{canonical} {filter}"),
                    CodeQueryKind::Callers
                )
                .await
                .results
                .len(),
                usize::from(matching),
                "{case}: {filter}"
            );
        }
        assert!(
            query(
                &service,
                &format!("{canonical} path:absent"),
                CodeQueryKind::Callers
            )
            .await
            .results
            .is_empty()
        );
    }
}

#[tokio::test]
async fn canonical_declaration_callers_require_explicit_snapshot_for_unknown_types() {
    for (case, source) in [
        (
            "macro",
            "#define T int\nint helper(T);\ninline int header_caller() { return helper(3); }\n#undef T\n#define T double\nint helper(T value) { return 7; }\n",
        ),
        (
            "array",
            "int helper(int values[3]);\ninline int header_caller(int* values) { return helper(values); }\nint helper(int* values) { return 7; }\n",
        ),
    ] {
        let repo = FixtureRepo::create(&format!("canonical-unknown-{case}"));
        repo.write("src/foo.cpp", source);
        repo.git(["add", "."]);
        repo.git(["commit", "-m", "Unknown type evidence"]);
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
                context("index-unknown-signature"),
            )
            .await
            .unwrap();
        let definitions = query(&service, "helper", CodeQueryKind::Definition).await;
        let canonical = definitions
            .results
            .iter()
            .filter_map(|hit| hit.canonical_symbol_id.as_deref())
            .find(|id| id.ends_with("::helper"))
            .unwrap();
        let error = service
            .query_code_repository(
                CodeRetrievalRequest::new(
                    canonical,
                    selector("fixture", "HEAD"),
                    CodeQueryKind::Callers,
                    10,
                    FreshnessPolicy::AllowStale,
                )
                .unwrap(),
                context("unknown-signature"),
            )
            .await
            .unwrap_err();
        assert_eq!(error.error_kind, ErrorKind::InvalidArgument, "{case}");
        assert!(
            error.message.contains("symbol_snapshot_id"),
            "{case}: {error:?}"
        );
    }
}
