use super::*;

#[test]
fn unspecified_range_starts_at_the_commit_version() {
    let commit_version = GraphVersion::new(7);

    assert_eq!(
        storage_version_range(
            GraphVersionRange::open_from(GraphVersion::ZERO),
            commit_version,
        ),
        GraphVersionRange::open_from(commit_version)
    );
}

#[test]
fn explicit_range_is_preserved() {
    let range = GraphVersionRange::new(GraphVersion::new(3), Some(GraphVersion::new(5)))
        .expect("range should be ordered");

    assert_eq!(storage_version_range(range, GraphVersion::new(7)), range);
}
