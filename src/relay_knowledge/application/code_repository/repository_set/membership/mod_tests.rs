// Direct tests for repository-set membership rules.

use super::*;

#[test]
fn member_filter_merge_preserves_order_without_duplicates() {
    assert_eq!(
        merged_filters(&["src".to_owned()], &["src".to_owned(), "tests".to_owned()]),
        ["src".to_owned(), "tests".to_owned()]
    );
}
