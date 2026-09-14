use super::super::tests::{fixture, models};
use super::*;

#[test]
fn module_identity_survives_snapshot_coordinate_and_line_changes() {
    let (modules, _) = fixture();
    let mut changed = models(&[(
        "a/pom.xml",
        "<project><groupId>new</groupId><artifactId>renamed</artifactId><version>2</version></project>",
    )]);
    changed[0].document.source_scope = "other".into();
    changed[0].line = 99;
    let (updated, _) = facts(&changed, GraphVersion::ZERO).unwrap();
    assert_eq!(
        modules
            .iter()
            .find(|module| module.target.evidence_path == "a/pom.xml")
            .unwrap()
            .target
            .target_id,
        updated[0].target.target_id
    );
}

#[test]
fn missing_version_mismatches_classifiers_and_duplicate_coordinates_stay_unresolved() {
    let mut input = models(&[
        (
            "a/pom.xml",
            "<project><groupId>x</groupId><artifactId>a</artifactId><version>1</version><dependencies><dependency><groupId>x</groupId><artifactId>b</artifactId><version>2</version></dependency><dependency><groupId>external</groupId><artifactId>lib</artifactId><version>1</version></dependency></dependencies></project>",
        ),
        (
            "b/pom.xml",
            "<project><groupId>x</groupId><artifactId>b</artifactId><version>1</version></project>",
        ),
    ]);
    let (_, edges) = facts(&input, GraphVersion::ZERO).unwrap();
    assert!(
        edges
            .iter()
            .all(|edge| edge.relationship.resolution_state == "unresolved")
    );
    input[0].dependencies[0].version = Some("1".into());
    input[0].dependencies[0].classifier = Some("tests".into());
    assert!(
        facts(&input, GraphVersion::ZERO)
            .unwrap()
            .1
            .iter()
            .all(|edge| edge.relationship.resolution_state == "unresolved")
    );
    input[0].dependencies[0].classifier = None;
    input[0].dependencies[0].version = None;
    assert!(
        facts(&input, GraphVersion::ZERO)
            .unwrap()
            .1
            .iter()
            .all(|edge| edge.relationship.resolution_state == "unresolved")
    );
    input[0].dependencies[0].version = Some("1".into());
    input[1].version = Some("${unknown}".into());
    input[0].dependencies[0].version = Some("${unknown}".into());
    assert!(
        facts(&input, GraphVersion::ZERO)
            .unwrap()
            .1
            .iter()
            .all(|edge| edge.relationship.resolution_state == "unresolved")
    );
    input[1].version = Some("1".into());
    input[0].dependencies[0].version = Some("1".into());
    let mut duplicate = input[1].clone();
    duplicate.document.path = "duplicate/pom.xml".into();
    input.push(duplicate);
    assert!(
        facts(&input, GraphVersion::ZERO)
            .unwrap()
            .1
            .iter()
            .any(|edge| edge.relationship.resolution_state == "ambiguous")
    );
}

#[test]
fn bounded_modules_and_missing_aggregate_target_are_explicit() {
    let input = models(&[(
        "pom.xml",
        "<project><groupId>x</groupId><artifactId>a</artifactId><version>1</version><modules><module>../../escape</module></modules></project>",
    )]);
    let (_, edges) = facts(&input, GraphVersion::ZERO).unwrap();
    assert_eq!(edges[0].relationship.resolution_state, "unresolved");
    assert!(facts(&vec![input[0].clone(); MAX_MODULES + 1], GraphVersion::ZERO).is_err());
}

#[test]
fn same_line_profile_variants_keep_separate_edges() {
    let mut input = models(&[(
        "a/pom.xml",
        "<project><groupId>x</groupId><artifactId>a</artifactId><version>1</version><dependencies><dependency><groupId>ext</groupId><artifactId>lib</artifactId><version>1</version></dependency></dependencies></project>",
    )]);
    let mut variant = input[0].dependencies[0].clone();
    variant.profile = Some("optional".into());
    input[0].dependencies.push(variant);
    assert_eq!(facts(&input, GraphVersion::ZERO).unwrap().1.len(), 2);
}

#[test]
fn system_artifact_does_not_resolve_to_matching_reactor_coordinate() {
    let input = models(&[
        (
            "a/pom.xml",
            "<project><groupId>x</groupId><artifactId>a</artifactId><version>1</version><dependencies><dependency><groupId>x</groupId><artifactId>b</artifactId><version>1</version><scope>system</scope><systemPath>/opt/lib/b.jar</systemPath></dependency></dependencies></project>",
        ),
        (
            "b/pom.xml",
            "<project><groupId>x</groupId><artifactId>b</artifactId><version>1</version></project>",
        ),
    ]);
    let (_, edges) = facts(&input, GraphVersion::ZERO).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].relationship.resolution_state, "unresolved");
    assert_eq!(edges[0].relationship.target_kind, "artifact");
    assert!(
        edges[0]
            .relationship
            .target_hint
            .as_deref()
            .unwrap()
            .contains("scope=system")
    );
}

#[test]
fn default_profile_modules_and_prefixed_poms_are_in_the_reactor() {
    let input = models(&[
        (
            "pom.xml",
            "<m:project xmlns:m='http://maven.apache.org/POM/4.0.0'><m:groupId>x</m:groupId><m:artifactId>root</m:artifactId><m:version>1</m:version><m:packaging>pom</m:packaging><m:profiles><m:profile><m:id>default</m:id><m:activation><m:activeByDefault>true</m:activeByDefault></m:activation><m:properties><m:member>child</m:member></m:properties><m:modules><m:module>${member}</m:module></m:modules></m:profile><m:profile><m:id>opt-in</m:id><m:modules><m:module>absent</m:module></m:modules></m:profile></m:profiles></m:project>",
        ),
        (
            "child/pom.xml",
            "<m:project xmlns:m='http://maven.apache.org/POM/4.0.0'><m:groupId>x</m:groupId><m:artifactId>child</m:artifactId><m:version>1</m:version><m:description/></m:project>",
        ),
    ]);
    let (modules, edges) = facts(&input, GraphVersion::ZERO).unwrap();
    assert_eq!(modules.len(), 2);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].relationship.relationship_kind, "aggregates");
    assert_eq!(edges[0].relationship.resolution_state, "resolved");
    assert_eq!(edges[0].relationship.target_hint.as_deref(), Some("child"));
}

#[test]
fn parent_edges_follow_effective_resolution_and_preserve_external_evidence() {
    let input = models(&[
        (
            "pom.xml",
            "<project><groupId>x</groupId><artifactId>root</artifactId><version>1</version><packaging>pom</packaging></project>",
        ),
        (
            "child/pom.xml",
            "<project><parent><groupId>x</groupId><artifactId>root</artifactId><version>1</version></parent><artifactId>child</artifactId></project>",
        ),
        (
            "external/pom.xml",
            "<project><parent><groupId>x</groupId><artifactId>remote</artifactId><version>1</version><relativePath/></parent><artifactId>external</artifactId></project>",
        ),
        (
            "mismatch/pom.xml",
            "<project><parent><groupId>x</groupId><artifactId>root</artifactId><version>2</version><relativePath>../pom.xml</relativePath></parent><artifactId>mismatch</artifactId></project>",
        ),
    ]);
    let (_, edges) = facts(&input, GraphVersion::ZERO).unwrap();
    assert_eq!(edges.len(), 3);
    for edge in edges {
        assert_eq!(edge.relationship.relationship_kind, "inherits_from");
        assert_eq!(
            edge.relationship.resolution_state,
            if edge.relationship.evidence_path == "child/pom.xml" {
                "resolved"
            } else {
                "unresolved"
            }
        );
    }
}

#[test]
fn xml_entities_resolve_module_paths_without_unescaping_cdata_twice() {
    let input = models(&[
        (
            "pom.xml",
            "<project><groupId>x</groupId><artifactId>root&#x2d;api</artifactId><version>1</version><modules><module>foo&amp;bar</module><module>num&#45;child</module><module><![CDATA[literal&amp;child]]></module></modules></project>",
        ),
        (
            "foo&bar/pom.xml",
            "<project><groupId>x</groupId><artifactId>child1</artifactId><version>1</version></project>",
        ),
        (
            "num-child/pom.xml",
            "<project><groupId>x</groupId><artifactId>child2</artifactId><version>1</version></project>",
        ),
        (
            "literal&amp;child/pom.xml",
            "<project><groupId>x</groupId><artifactId>child3</artifactId><version>1</version></project>",
        ),
    ]);
    let (nodes, edges) = facts(&input, GraphVersion::ZERO).unwrap();
    assert_eq!(nodes.len(), 4);
    assert!(nodes.iter().any(|node| node.target.name == "x:root-api:1"));
    assert_eq!(edges.len(), 3);
    assert!(
        edges
            .iter()
            .all(|edge| edge.relationship.resolution_state == "resolved")
    );
}
