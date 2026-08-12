use super::resolve_update_base;

#[test]
fn explicit_update_base_remains_authoritative_for_non_git_snapshots() {
    let base = resolve_update_base(
        Some("filesystem:0123456789abcdef".to_owned()),
        None,
        "fixture",
    )
    .expect("explicit filesystem base should remain valid");

    assert_eq!(base, "filesystem:0123456789abcdef");
}

#[test]
fn implicit_update_base_unwraps_the_active_worktree_identity() {
    let base = resolve_update_base(
        None,
        Some("worktree:0123456789abcdef0123456789abcdef01234567:fedcba9876543210"),
        "fixture",
    )
    .expect("worktree identity should expose its clean Git base");

    assert_eq!(base, "0123456789abcdef0123456789abcdef01234567");
}

#[test]
fn implicit_update_base_rejects_an_unindexed_repository() {
    let error = resolve_update_base(None, None, "fixture")
        .expect_err("an implicit base requires a completed clean snapshot");

    assert!(error.contains("fixture"));
    assert!(error.contains("repo index --ref HEAD"));
}
