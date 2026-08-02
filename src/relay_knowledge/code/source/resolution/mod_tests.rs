use super::*;

#[test]
fn scoped_gitlink_filters_normalize_deduplicate_and_honor_root_scope() {
    assert_eq!(
        scoped_gitlink_filters(&[
            "./modules/core/".to_owned(),
            "modules/core".to_owned(),
            "plugins/api".to_owned(),
        ]),
        ["modules/core", "plugins/api"]
    );
    assert!(scoped_gitlink_filters(&["modules/core".to_owned(), ".".to_owned()]).is_empty());
}

#[test]
fn path_ancestors_remain_ordered_from_specific_to_rootward() {
    assert_eq!(
        path_and_ancestors("modules/core/src"),
        ["modules/core/src", "modules/core", "modules"]
    );
    assert!(path_and_ancestors("").is_empty());
}
