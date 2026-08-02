//! Direct repository-relative POM path invariants.

use super::relative_pom_path;

#[test]
fn relative_pom_paths_normalize_segments_without_escaping_the_repository() {
    assert_eq!(
        relative_pom_path("apps/api/pom.xml", "../../parent/./pom.xml"),
        Some("parent/pom.xml".to_owned())
    );
    assert_eq!(
        relative_pom_path("apps/api/pom.xml", "../pom.xml"),
        Some("apps/pom.xml".to_owned())
    );
    assert_eq!(relative_pom_path("pom.xml", "../pom.xml"), None);
    assert_eq!(relative_pom_path("apps/pom.xml", "/pom.xml"), None);
}
