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
