use super::test_support::{edge, hit, member_status, set_hit};
use super::*;
use crate::domain::CodeRetrievalLayer;

#[test]
fn overlay_index_attaches_target_and_import_origin_evidence_in_edge_order() {
    let inbound = edge(
        "edge-in",
        "scope-service",
        Some("scope-app"),
        r#"{"from_path":"src/service.rs"}"#,
        9_000,
    );
    let outbound = edge(
        "edge-out",
        "scope-app",
        Some("scope-service"),
        r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
        6_000,
    );
    let unrelated = edge(
        "edge-other",
        "scope-other",
        Some("scope-service"),
        r#"{"from_path":"src/other.rs","from_line_start":1,"from_line_end":1}"#,
        10_000,
    );
    let mut wrong_target = edge(
        "edge-wrong",
        "scope-service",
        Some("scope-app"),
        "{}",
        8_000,
    );
    wrong_target.to_record_id = Some("symbol-other".to_owned());
    let edges = vec![inbound.clone(), outbound.clone(), unrelated, wrong_target];
    let index = OverlayEvidenceIndex::new(&edges);

    let evidence =
        index.evidence_for_hit(&hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false));
    assert_eq!(evidence, vec![inbound, outbound.clone()]);

    let mut import_hit = hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false);
    import_hit.symbol_snapshot_id = None;
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
    import_hit.edge_kind = Some("import".to_owned());
    assert_eq!(index.evidence_for_hit(&import_hit), vec![outbound]);
}

#[test]
fn overlay_index_caps_file_origin_evidence_for_non_import_hits() {
    let mut edges = Vec::new();
    for (index, confidence) in [0, 5_000, 10_000, 7_000].into_iter().enumerate() {
        edges.push(edge(
            &format!("edge-origin-{index}"),
            "scope-app",
            Some("scope-service"),
            r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
            confidence,
        ));
    }
    let index = OverlayEvidenceIndex::new(&edges);

    let evidence = index.evidence_for_hit(&hit(
        "repo-a",
        "scope-app",
        "src/client.rs",
        20,
        0.75,
        false,
    ));

    assert_eq!(evidence.len(), 2);
    assert_eq!(evidence[0].edge_id, "edge-origin-2");
    assert_eq!(evidence[1].edge_id, "edge-origin-3");
}

#[test]
fn overlay_index_dedupes_and_caps_multi_key_evidence() {
    let mut edges = Vec::new();
    for index in 0..8 {
        edges.push(edge(
            &format!("edge-{index}"),
            "scope-service",
            Some("scope-app"),
            r#"{"from_path":"src/service.rs"}"#,
            5_000,
        ));
    }
    let mut file_edge = edge(
        "edge-file",
        "scope-service",
        Some("scope-app"),
        r#"{"from_path":"src/service.rs"}"#,
        4_000,
    );
    file_edge.to_record_kind = "code_file".to_owned();
    file_edge.to_record_id = Some("file-1".to_owned());
    edges.insert(2, file_edge.clone());
    let index = OverlayEvidenceIndex::new(&edges);
    assert_eq!(
        index
            .target_symbols
            .get(&("scope-app".to_owned(), "symbol-1".to_owned()))
            .map(Vec::len),
        Some(MAX_TARGET_EVIDENCE_PER_RECORD)
    );

    let evidence =
        index.evidence_for_hit(&hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false));

    assert_eq!(evidence.len(), 5);
    assert_eq!(evidence[2], file_edge);
}

#[test]
fn ranking_helpers_keep_existing_merge_policy() {
    let member = member_status("app", "scope-app", 7);
    let base_hit = hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false);
    let evidence = vec![edge(
        "edge-in",
        "scope-service",
        Some("scope-app"),
        r#"{"from_path":"src/service.rs"}"#,
        9_000,
    )];
    assert!(repository_set_score("", &base_hit, &member, &evidence) > base_hit.score);
    assert!(
        repository_set_score(
            "",
            &hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, true),
            &member,
            &[]
        ) < base_hit.score
    );

    let mut results = vec![
        CodeRepositorySetQueryHit {
            member: member.member.clone(),
            hit: hit("repo-a", "scope-app", "src/client.rs", 1, 0.50, false),
            overlay_evidence: Vec::new(),
            score: 0.50,
        },
        CodeRepositorySetQueryHit {
            member: member.member.clone(),
            hit: hit("repo-a", "scope-app", "src/client.rs", 1, 0.90, false),
            overlay_evidence: evidence,
            score: 0.90,
        },
        CodeRepositorySetQueryHit {
            member: member.member.clone(),
            hit: hit("repo-a", "scope-app", "src/client.rs", 2, 0.80, false),
            overlay_evidence: Vec::new(),
            score: 0.80,
        },
    ];
    assert!(dedupe_sort_truncate(&mut results, 1, ""));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].score, 0.90);

    assert!(evidence_origin("not-json").is_none());
    assert!(evidence_origin("{}").is_none());
}

#[test]
fn candidate_limit_keeps_single_member_depth_and_shares_multi_member_budget() {
    assert_eq!(per_member_candidate_limit(10, 0), 0);
    assert_eq!(per_member_candidate_limit(1, 1), 6);
    assert_eq!(per_member_candidate_limit(20, 1), 50);
    assert_eq!(per_member_candidate_limit(10, 2), 15);
    assert_eq!(per_member_candidate_limit(20, 2), 30);
    assert_eq!(per_member_candidate_limit(20, 4), 15);
}

#[test]
fn overlay_selector_dedupes_candidate_origin_and_target_keys() {
    let first = hit("repo-a", "scope-a", "src/client.rs", 1, 1.0, false);
    let duplicate = first.clone();
    let second = hit("repo-b", "scope-b", "src/service.rs", 2, 1.0, false);

    let selector = overlay_edge_selector([&first, &duplicate, &second]);

    assert_eq!(
        selector.origin_files,
        vec![
            ("scope-a".to_owned(), "src/client.rs".to_owned()),
            ("scope-b".to_owned(), "src/service.rs".to_owned()),
        ]
    );
    assert_eq!(selector.target_records.len(), 4);
    assert!(selector.target_records.contains(&(
        "scope-a".to_owned(),
        "code_symbol_snapshot".to_owned(),
        "symbol-1".to_owned(),
    )));
    assert!(selector.target_records.contains(&(
        "scope-b".to_owned(),
        "code_file".to_owned(),
        "file-1".to_owned(),
    )));
}

#[test]
fn evidence_backed_member_priority_is_bounded_workspace_ranking_intent() {
    let preferred = member_status("app", "scope-app", 10);
    let dependency = member_status("sdk", "scope-sdk", 0);
    let preferred_hit = hit("repo-app", "scope-app", "src/client.rs", 1, 11.20, false);
    let dependency_hit = hit("repo-sdk", "scope-sdk", "src/client.rs", 1, 12.20, false);
    let evidence = vec![edge(
        "edge-in",
        "scope-app",
        Some("scope-sdk"),
        r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
        10_000,
    )];

    assert!(
        repository_set_score("", &preferred_hit, &preferred, &evidence)
            > repository_set_score("", &dependency_hit, &dependency, &[])
    );
    assert!(
        repository_set_score("", &preferred_hit, &preferred, &[])
            < repository_set_score("", &dependency_hit, &dependency, &[])
    );
    let mut ambiguous_package = evidence[0].clone();
    ambiguous_package.resolution_state = "ambiguous".to_owned();
    ambiguous_package.to_source_scope = None;
    ambiguous_package.to_repository_id = None;
    ambiguous_package.to_record_id = None;
    ambiguous_package.confidence_basis_points = 5_000;
    assert!(
        member_priority_bonus(10, true, &[ambiguous_package])
            > member_priority_bonus(10, true, &[])
    );
    assert_eq!(
        member_priority_bonus(100, true, &evidence),
        member_priority_bonus(10, true, &evidence)
    );
    assert_eq!(
        member_priority_bonus(-100, true, &evidence),
        member_priority_bonus(-10, true, &evidence)
    );
}

#[test]
fn domain_affinity_requires_fresh_evidence_backed_priority() {
    let member = member_status("app", "scope-app", 10);
    let query = "metric_sink pipeline";
    let fresh_hit = hit(
        "repo-app",
        "scope-app",
        "connectors/metricsink/metricsink.go",
        1,
        10.0,
        false,
    );
    let stale_hit = hit(
        "repo-app",
        "scope-app",
        "connectors/metricsink/metricsink.go",
        1,
        10.0,
        true,
    );
    let evidence = vec![edge(
        "edge-in",
        "scope-sdk",
        Some("scope-app"),
        r#"{"from_path":"src/sdk.rs","from_line_start":1,"from_line_end":1}"#,
        10_000,
    )];

    let unsupported_score = repository_set_score(query, &fresh_hit, &member, &[]);
    let supported_score = repository_set_score(query, &fresh_hit, &member, &evidence);
    let stale_score = repository_set_score(query, &stale_hit, &member, &evidence);

    assert!(supported_score > unsupported_score + 1.0);
    assert!(unsupported_score < fresh_hit.score + 1.0);
    assert!(stale_score < supported_score);
}

#[test]
fn repository_set_ties_prefer_less_specialized_paths() {
    let member = member_status("app", "scope-app", 0);
    let mut results = vec![
        CodeRepositorySetQueryHit {
            member: member.member.clone(),
            hit: hit(
                "repo-app",
                "scope-app",
                "samples/verbose_client/main.rs",
                1,
                1.0,
                false,
            ),
            overlay_evidence: Vec::new(),
            score: 2.0,
        },
        CodeRepositorySetQueryHit {
            member: member.member.clone(),
            hit: hit("repo-app", "scope-app", "samples/client.rs", 1, 1.0, false),
            overlay_evidence: Vec::new(),
            score: 2.0,
        },
    ];

    assert!(!dedupe_sort_truncate(&mut results, 2, ""));

    assert_eq!(results[0].hit.path, "samples/client.rs");
}

#[test]
fn repository_set_top_k_diversifies_relevant_members() {
    let app = member_status("app", "scope-app", 0);
    let sdk = member_status("sdk", "scope-sdk", 0);
    let mut results = vec![
        set_hit(&app, 1, 12.0),
        set_hit(&app, 2, 11.8),
        set_hit(&app, 3, 11.6),
        set_hit(&app, 4, 11.4),
        set_hit(&sdk, 10, 8.9),
        set_hit(&sdk, 11, 8.7),
    ];

    assert!(dedupe_sort_truncate(&mut results, 5, ""));

    assert_eq!(results[0].member.repository_alias, "app");
    assert_eq!(
        results
            .iter()
            .filter(|result| result.member.repository_alias == "sdk")
            .count(),
        2
    );
}

#[test]
fn bridge_support_bonus_promotes_present_usage_and_target_pair() {
    let app = member_status("app", "scope-app", 0);
    let service = member_status("svc", "scope-service", 0);
    let bridge = edge(
        "edge-bridge",
        "scope-app",
        Some("scope-service"),
        r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
        10_000,
    );
    let mut results = vec![
        CodeRepositorySetQueryHit {
            member: app.member.clone(),
            hit: hit("repo-app", "scope-app", "src/client.rs", 20, 0.80, false),
            overlay_evidence: vec![bridge.clone()],
            score: 0.80,
        },
        CodeRepositorySetQueryHit {
            member: service.member.clone(),
            hit: hit(
                "repo-service",
                "scope-service",
                "src/service.rs",
                1,
                0.70,
                false,
            ),
            overlay_evidence: vec![bridge.clone()],
            score: 0.70,
        },
    ];

    apply_bridge_support_bonus(&mut results);
    prune_returned_overlay_evidence(&mut results);

    assert!(results[0].score > 0.80);
    assert!(results[1].score > 0.70);
    assert_eq!(results[0].overlay_evidence, vec![bridge.clone()]);
    assert_eq!(results[1].overlay_evidence, vec![bridge]);
}

#[test]
fn bridge_support_bonus_requires_both_resolved_endpoints() {
    let app = member_status("app", "scope-app", 0);
    let missing_target = edge(
        "edge-missing",
        "scope-app",
        Some("scope-service"),
        r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
        10_000,
    );
    let mut unresolved = missing_target.clone();
    unresolved.resolution_state = "unresolved".to_owned();
    let mut results = vec![
        CodeRepositorySetQueryHit {
            member: app.member.clone(),
            hit: hit("repo-app", "scope-app", "src/client.rs", 20, 0.80, false),
            overlay_evidence: vec![missing_target],
            score: 0.80,
        },
        CodeRepositorySetQueryHit {
            member: app.member.clone(),
            hit: hit("repo-app", "scope-app", "src/other.rs", 30, 0.70, false),
            overlay_evidence: vec![unresolved],
            score: 0.70,
        },
    ];

    apply_bridge_support_bonus(&mut results);

    assert_eq!(results[0].score, 0.80);
    assert_eq!(results[1].score, 0.70);
}

#[test]
fn returned_overlay_evidence_prunes_origin_only_file_noise() {
    let app = member_status("app", "scope-app", 0);
    let origin_only = edge(
        "edge-origin-only",
        "scope-app",
        Some("scope-service"),
        r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
        10_000,
    );
    let mut results = vec![CodeRepositorySetQueryHit {
        member: app.member.clone(),
        hit: hit("repo-app", "scope-app", "src/client.rs", 20, 0.80, false),
        overlay_evidence: vec![origin_only],
        score: 0.80,
    }];

    prune_returned_overlay_evidence(&mut results);

    assert!(results[0].overlay_evidence.is_empty());
}
