//! Request prefixes intersect the indexed base before fallback candidate budgets.

use super::pipeline_tests::{hit, request, status};
use super::*;
use crate::domain::RepositoryCodeRange;

#[test]
fn slash_only_groups_disable_fallback_before_candidate_discovery() {
    for filter in ["/", "///", "\\", "/\\/"] {
        for kind in [
            CodeQueryKind::Definition,
            CodeQueryKind::References,
            CodeQueryKind::Imports,
            CodeQueryKind::Hybrid,
        ] {
            let request = request("ScopeNeedle", kind, vec![filter.to_owned()]);
            assert!(
                plan_code_grep_fallback(&status(), &request, &[]).is_none(),
                "a nonmatching request group must stop before discovery and materialization"
            );
        }
        let mut restricted_status = status();
        restricted_status.path_filters = vec![filter.to_owned()];
        let unrestricted_request = request("ScopeNeedle", CodeQueryKind::Hybrid, Vec::new());
        assert!(plan_code_grep_fallback(&restricted_status, &unrestricted_request, &[]).is_none());
        restricted_status.path_filters.push("include".to_owned());
        let mixed_request = request(
            "ScopeNeedle",
            CodeQueryKind::Hybrid,
            vec![filter.to_owned(), "./include/api/".to_owned()],
        );
        let plan = plan_code_grep_fallback(&restricted_status, &mixed_request, &[]).unwrap();
        assert_eq!(plan.path_filters, ["include/api"]);
        assert!(plan.needs_scope_paths());
    }
}

#[test]
fn fallback_plan_intersects_prefix_groups_without_unrestricting_disjoint_scopes() {
    for (base, requested, expected) in [
        (vec![], vec![], Some(vec![])),
        (vec!["src"], vec![], Some(vec!["src"])),
        (vec![], vec!["src/api"], Some(vec!["src/api"])),
        (vec!["src"], vec!["src"], Some(vec!["src"])),
        (vec!["./src/"], vec!["./src/api/"], Some(vec!["src/api"])),
        (vec!["src/api"], vec!["src"], Some(vec!["src/api"])),
        (vec!["."], vec!["src"], Some(vec!["src"])),
        (vec!["src"], vec!["."], Some(vec!["src"])),
        (vec!["src"], vec!["src/missing/"], Some(vec!["src/missing"])),
        (vec!["src"], vec!["src2"], None),
        (vec!["src/api"], vec!["src/other"], None),
        (
            vec!["src", "docs"],
            vec!["src/api", "docs/guide", "other"],
            Some(vec!["src/api", "docs/guide"]),
        ),
    ] {
        let mut status = status();
        status.path_filters = base.iter().map(|s| (*s).to_owned()).collect();
        let request = request(
            "ScopeNeedle",
            CodeQueryKind::Hybrid,
            requested.iter().map(|s| (*s).to_owned()).collect(),
        );
        let actual = plan_code_grep_fallback(&status, &request, &[]).map(|plan| plan.path_filters);
        assert_eq!(
            actual,
            expected.map(|paths| paths.into_iter().map(str::to_owned).collect()),
            "base={base:?}, requested={requested:?}"
        );
    }
}

#[test]
fn fallback_plan_filters_explicit_paths_before_candidate_budget_and_declaration_reads() {
    let mut status = status();
    status.path_filters = vec!["include".to_owned()];
    let request = request(
        "rk_read_fn",
        CodeQueryKind::Definition,
        vec!["./include/api/".to_owned()],
    );
    let mut hits = (0..crate::code::SOURCE_GREP_CANDIDATE_FILE_LIMIT + 1)
        .map(|index| {
            hit(
                &format!("include/api2/other{index}.h"),
                "rk_read_fn callback;",
            )
        })
        .collect::<Vec<_>>();
    hits.push(hit("include/api/valid.h", "rk_read_fn callback;"));
    let plan = plan_code_grep_fallback(&status, &request, &hits).unwrap();
    assert_eq!(plan.paths, ["include/api/valid.h"]);
    assert_eq!(plan.path_filters, ["include/api"]);
    assert!(!plan.needs_scope_paths());
    assert_eq!(plan.source_request().paths, ["include/api/valid.h"]);
}

#[test]
fn fallback_plan_keeps_discovery_scoped_after_rejecting_explicit_paths() {
    let request = request(
        "rk_read_fn",
        CodeQueryKind::Definition,
        vec!["include/api".to_owned()],
    );
    let plan = plan_code_grep_fallback(
        &status(),
        &request,
        &[hit("include/api2/other.h", "rk_read_fn callback;")],
    )
    .unwrap();
    assert!(plan.paths.is_empty());
    assert_eq!(plan.path_filters, ["include/api"]);
    assert!(plan.needs_scope_paths());
}

#[test]
fn fallback_append_checks_both_scope_groups_for_grep_and_declarations() {
    for (base, requested, allowed) in [
        (vec!["include/api"], vec![], vec!["include/api/valid.h"]),
        (
            vec!["include"],
            vec!["./include/api/"],
            vec!["include/api/valid.h"],
        ),
        (vec!["include/api"], vec!["include/api2"], vec![]),
        (
            vec![],
            vec![],
            vec![
                "include/api/valid.h",
                "include/api2/other.h",
                "outside/file.h",
            ],
        ),
    ] {
        let mut status = status();
        status.path_filters = base.iter().map(|s| (*s).to_owned()).collect();
        let request = request(
            "rk_read_fn",
            CodeQueryKind::Definition,
            requested.iter().map(|s| (*s).to_owned()).collect(),
        );
        let plan = plan_code_grep_fallback(
            &super::pipeline_tests::status(),
            &super::pipeline_tests::request("rk_read_fn", CodeQueryKind::Definition, Vec::new()),
            &[],
        )
        .unwrap();
        let matches = [
            "include/api/valid.h",
            "include/api2/other.h",
            "outside/file.h",
        ]
        .into_iter()
        .map(|path| SourceGrepMatch {
            path: path.to_owned(),
            language_id: "c".to_owned(),
            excerpt: "typedef int (*rk_read_fn)(void);".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 30 },
            line_range: RepositoryCodeRange { start: 1, end: 1 },
            is_generated: false,
        })
        .collect::<Vec<_>>();
        let declarations = matches
            .iter()
            .map(|item| crate::code::SourceDeclarationMatch {
                path: item.path.clone(),
                excerpt: item.excerpt.clone(),
                byte_range: item.byte_range.clone(),
                line_range: item.line_range.clone(),
                is_generated: false,
            })
            .collect();
        let mut grep_hits = Vec::new();
        append_code_grep_fallback(
            &status,
            &request,
            &mut grep_hits,
            &plan,
            SourceGrepOutcome {
                matches,
                degraded_reason: None,
            },
        );
        let mut declaration_hits = Vec::new();
        append_definition_source_fallback(&status, &request, &mut declaration_hits, declarations);
        let expected = allowed
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        for hits in [&grep_hits, &declaration_hits] {
            assert_eq!(
                hits.iter()
                    .map(|hit| hit.path.as_str())
                    .collect::<std::collections::BTreeSet<_>>(),
                expected
            );
        }
    }
}
