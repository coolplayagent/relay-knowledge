use super::*;

#[test]
fn flattens_repository_set_member_provenance() {
    let payload = serde_json::json!({
        "results": [{
            "member": {
                "repository_alias": "sdk",
                "source_scope": "sdk::HEAD",
                "resolved_commit_sha": "abc"
            },
            "hit": {
                "path": "client/client.go",
                "excerpt": "func Dial"
            },
            "score": 0.7
        }]
    });

    let hits = flatten_repository_set_hits(&payload);

    assert_eq!(hits[0]["repository_alias"], "sdk");
    assert_eq!(hits[0]["source_scope"], "sdk::HEAD");
    assert_eq!(hits[0]["path"], "client/client.go");
    assert_eq!(hits[0]["repository_set_score"], 0.7);
}

#[test]
fn selected_member_names_follow_category_filtered_cases() {
    let categories =
        crate::config::CategorySet::parse("semantic_vector").expect("categories should parse");
    let cases_config = serde_json::json!({
        "repository_sets": {
            "guarded_workspace": {
                "members": [
                    {"repository": "member_a"},
                    {"repository": "member_b"}
                ]
            },
            "regular_workspace": {
                "members": [
                    {"repository": "member_c"}
                ]
            }
        },
        "repository_set_query_cases": [
            {
                "id": "guardrail_case",
                "repository_set": "guarded_workspace",
                "guardrail": true
            },
            {
                "id": "regular_case",
                "repository_set": "regular_workspace"
            }
        ]
    });

    let members = selected_repository_set_member_names(&cases_config, "full", Some(&categories));

    assert!(members.contains("member_a"));
    assert!(members.contains("member_b"));
    assert!(!members.contains("member_c"));
}
