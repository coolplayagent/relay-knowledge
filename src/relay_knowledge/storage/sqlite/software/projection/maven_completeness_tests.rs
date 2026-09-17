//! Retained Maven facts remain visibly incomplete through atomic and fenced publication.
use super::test_support::{create_test_schema, seed_scope};
use super::*;

#[test]
fn maven_build_and_all_keep_degraded_status_until_pom_is_repaired() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_test_schema(&connection);
    super::super::schema::initialize_schema(&connection).unwrap();
    seed_scope(&connection);
    let valid =
        "<project><groupId>x</groupId><artifactId>root</artifactId><version>1</version></project>";
    connection.execute("INSERT INTO code_repository_files VALUES ('repo','scope-1','pom','pom.xml','xml','parsed',0)", []).unwrap();
    connection.execute("INSERT INTO code_repository_chunks VALUES ('repo','scope-1','pom','pom.xml','xml',?1,1,1)", [valid]).unwrap();
    let initial = refresh_projection(&mut connection, "scope-1").unwrap();
    assert_eq!(initial.status.freshness, SoftwareProjectionFreshness::Fresh);
    let targets = initial
        .build_targets
        .iter()
        .filter(|target| target.ecosystem == "maven")
        .count();
    assert!(targets > 0);
    for invalid in ["", "<other/>", "<project>"] {
        connection
            .execute(
                "UPDATE code_repository_chunks SET content=?1 WHERE chunk_id='pom'",
                [invalid],
            )
            .unwrap();
        let retained = refresh_projection(&mut connection, "scope-1").unwrap();
        assert_eq!(
            retained.status.freshness,
            SoftwareProjectionFreshness::Degraded
        );
        assert_eq!(retained.status.completeness_basis_points, 0);
        assert!(
            retained
                .status
                .last_error
                .as_ref()
                .unwrap()
                .contains("earlier snapshot")
        );
        for kind in [SoftwareGlobalKind::Build, SoftwareGlobalKind::All] {
            let request = SoftwareGlobalRequest::new(
                crate::domain::CodeRepositorySelector::new("repo", "commit-1", vec![], vec![])
                    .unwrap(),
                kind,
                crate::domain::FreshnessPolicy::AllowStale,
                500,
            )
            .unwrap();
            let result = projection(&mut connection, request).unwrap();
            assert_eq!(
                result.status.freshness,
                SoftwareProjectionFreshness::Degraded
            );
            assert!(
                !result.status.stale,
                "completed publication must not restart indefinitely"
            );
            assert_eq!(
                result
                    .build_targets
                    .iter()
                    .filter(|target| target.ecosystem == "maven")
                    .count(),
                targets
            );
        }
    }
    connection
        .execute(
            "UPDATE code_repository_chunks SET content=?1 WHERE chunk_id='pom'",
            [valid],
        )
        .unwrap();
    let repaired = refresh_projection(&mut connection, "scope-1").unwrap();
    assert_eq!(
        repaired.status.freshness,
        SoftwareProjectionFreshness::Fresh
    );
    assert!(repaired.status.last_error.is_none());
}
