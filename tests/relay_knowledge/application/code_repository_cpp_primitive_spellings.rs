//! Real Git covers standard builtin equivalence without merging distinct overloads.
use super::*;

#[tokio::test]
async fn canonical_builtin_spelling_callers_preserve_distinct_overloads() {
    for (case, declaration, definition, matching) in [
        ("c_unsigned", "unsigned", "unsigned int", true),
        ("c_long", "long", "long int", true),
        ("c_short", "short", "signed short int", true),
        ("cpp_unsigned", "unsigned", "unsigned int", true),
        ("cpp_long", "long", "long int", true),
        ("cpp_signed", "signed", "int", true),
        ("cpp_short", "short", "short int", true),
        ("cpp_long_long", "long long", "signed long long int", true),
        (
            "cpp_unsigned_order",
            "long unsigned",
            "unsigned long int",
            true,
        ),
        ("cpp_long_double", "double long", "long double", true),
        ("cpp_different_signedness", "unsigned int", "int", false),
        ("cpp_different_width", "long", "int", false),
        ("cpp_different_plain_char", "char", "signed char", false),
        ("cpp_different_float", "float", "double", false),
        (
            "cpp_different_char_signedness",
            "signed char",
            "unsigned char",
            false,
        ),
    ] {
        let repo = FixtureRepo::create(&format!("canonical-builtin-{case}"));
        let (header, implementation) = if case.starts_with("c_") {
            ("foo.h", "foo.c")
        } else {
            ("foo.hpp", "foo.cpp")
        };
        repo.write(&format!("src/{header}"), &format!("int helper({declaration} old_name);\nstatic inline int header_caller(void) {{ return helper(3); }}\n"));
        repo.write(
            &format!("src/{implementation}"),
            &format!("#include \"{header}\"\nint helper({definition} new_name) {{ return 7; }}\n"),
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
        assert!(
            callers.results.iter().all(|hit| hit
                .canonical_symbol_id
                .as_deref()
                .is_some_and(|id| id.ends_with("::header_caller"))),
            "{case}"
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
                usize::from(symbol.path.ends_with(".h") || symbol.path.ends_with(".hpp")),
                "{case}"
            );
            assert!(
                exact.results.iter().all(|hit| hit
                    .canonical_symbol_id
                    .as_deref()
                    .is_some_and(|id| id.ends_with("::header_caller"))),
                "{case}"
            );
        }
        for filter in [
            "path:foo",
            "name:header_caller",
            "path:foo name:header_caller",
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
