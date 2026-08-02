//! Direct contracts for code-impact module seed matching rules.

use super::seed::module_import_matches;

#[test]
fn module_import_matching_respects_boundaries() {
    assert!(module_import_matches("crate::foo::bar", "foo::bar"));
    assert!(module_import_matches("foo::bar::baz", "foo::bar"));
    assert!(module_import_matches(
        "use crate::foo::bar;",
        "crate::foo::bar"
    ));
    assert!(module_import_matches("from foo.bar import baz", "foo.bar"));
    assert!(!module_import_matches("foo::barista", "foo::bar"));
    assert!(!module_import_matches("foo::bar_baz", "foo::bar"));
    assert!(!module_import_matches("foo::bar-baz", "foo::bar"));
}
