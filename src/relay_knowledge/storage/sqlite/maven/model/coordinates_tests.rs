//! Direct project-coordinate property alias contract.

use std::collections::BTreeMap;

use super::coordinates::{ProjectCoordinates, insert_project_properties};

#[test]
fn inserts_matching_project_and_pom_coordinate_aliases() {
    let mut properties = BTreeMap::new();
    insert_project_properties(
        &mut properties,
        &ProjectCoordinates {
            group_id: "com.example".to_owned(),
            artifact_id: "service".to_owned(),
            version: Some("1.2.3".to_owned()),
        },
    );

    assert_eq!(properties["project.groupId"], "com.example");
    assert_eq!(properties["pom.groupId"], "com.example");
    assert_eq!(properties["project.artifactId"], "service");
    assert_eq!(properties["pom.version"], "1.2.3");
}
