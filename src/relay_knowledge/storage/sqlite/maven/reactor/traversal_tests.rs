use super::super::tests::database;
use super::*;

#[test]
fn changing_leaf_returns_direct_and_transitive_downstream_evidence() {
    let connection = database();
    let changed = BTreeSet::from(["c/src/main/java/Api.java".into()]);
    let impacts = downstream(&connection, "scope", &changed, &[]).unwrap();
    assert_eq!(impacts.len(), 2);
    assert_eq!(impacts[0].chain, "b/pom.xml -> c/pom.xml");
    assert_eq!(impacts[1].chain, "a/pom.xml -> b/pom.xml -> c/pom.xml");
    assert!(
        downstream(&connection, "other", &changed, &[])
            .unwrap()
            .is_empty()
    );
    assert!(
        downstream(&connection, "scope", &BTreeSet::new(), &[])
            .unwrap()
            .is_empty()
    );
    assert!(
        downstream(&connection, "scope", &changed, &["a".into()])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn profile_edges_are_not_assumed_active_and_cycles_terminate() {
    let connection = database();
    connection
        .execute(
            "UPDATE maven_reactor_edges SET profile = 'opt-in' WHERE kind = 'depends_on'",
            [],
        )
        .unwrap();
    let changed = BTreeSet::from(["c/pom.xml".into()]);
    assert!(
        downstream(&connection, "scope", &changed, &[])
            .unwrap()
            .is_empty()
    );
    connection
        .execute("UPDATE maven_reactor_edges SET profile = NULL", [])
        .unwrap();
    let mut impacts = downstream(
        &connection,
        "scope",
        &BTreeSet::from(["a/pom.xml".into(), "b/pom.xml".into(), "c/pom.xml".into()]),
        &[],
    )
    .unwrap();
    assert!(impacts.is_empty());
    impacts = downstream(&connection, "scope", &changed, &[]).unwrap();
    assert_eq!(impacts.len(), 2);
}

#[test]
fn cyclic_dependency_graph_visits_each_module_once() {
    let connection = database();
    let (modules, mut edges) = super::super::tests::fixture();
    let mut edge = edges
        .iter()
        .find(|edge| edge.relationship.relationship_kind == "depends_on")
        .unwrap()
        .relationship
        .clone();
    edge.relationship_id = "cycle".into();
    edge.source_id = modules
        .iter()
        .find(|module| module.directory == "c")
        .unwrap()
        .target
        .target_id
        .clone();
    edge.target_id = modules
        .iter()
        .find(|module| module.directory == "a")
        .unwrap()
        .target
        .target_id
        .clone();
    edge.evidence_path = "c/pom.xml".into();
    edges.push(super::super::Edge {
        relationship: edge,
        dependency_scope: "compile".into(),
        profile: None,
    });
    super::super::persistence::persist(&connection, "scope", &modules, &edges).unwrap();
    assert_eq!(
        downstream(
            &connection,
            "scope",
            &BTreeSet::from(["c/src/Api.java".into()]),
            &[]
        )
        .unwrap()
        .len(),
        2
    );
}

#[test]
fn deep_dependency_graph_fails_explicitly_at_depth_budget() {
    let connection = database();
    let poms = (0..66).map(|index| {
        let dependency = if index < 65 { format!("<dependencies><dependency><groupId>x</groupId><artifactId>m{}</artifactId><version>1</version></dependency></dependencies>", index+1) } else { String::new() };
        (format!("m{index}/pom.xml"), format!("<project><groupId>x</groupId><artifactId>m{index}</artifactId><version>1</version>{dependency}</project>"))
    }).collect::<Vec<_>>();
    let refs = poms
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect::<Vec<_>>();
    let (modules, edges) = super::super::build::facts(
        &super::super::tests::models(&refs),
        crate::domain::GraphVersion::ZERO,
    )
    .unwrap();
    super::super::persistence::persist(&connection, "scope", &modules, &edges).unwrap();
    assert!(
        downstream(
            &connection,
            "scope",
            &BTreeSet::from(["m65/pom.xml".into()]),
            &[]
        )
        .is_err()
    );
}

#[test]
fn bom_and_parent_changes_reach_consumers_and_transitive_children() {
    let connection = database();
    let input = super::super::tests::models(&[
        (
            "bom/pom.xml",
            "<project><groupId>x</groupId><artifactId>bom</artifactId><version>1</version><packaging>pom</packaging></project>",
        ),
        (
            "parent/pom.xml",
            "<project><groupId>x</groupId><artifactId>parent</artifactId><version>1</version><packaging>pom</packaging><dependencyManagement><dependencies><dependency><groupId>x</groupId><artifactId>bom</artifactId><version>1</version><type>pom</type><scope>import</scope></dependency></dependencies></dependencyManagement></project>",
        ),
        (
            "child/pom.xml",
            "<project><parent><groupId>x</groupId><artifactId>parent</artifactId><version>1</version><relativePath>../parent/pom.xml</relativePath></parent><artifactId>child</artifactId><packaging>pom</packaging></project>",
        ),
        (
            "grandchild/pom.xml",
            "<project><parent><groupId>x</groupId><artifactId>child</artifactId><version>1</version><relativePath>../child/pom.xml</relativePath></parent><artifactId>grandchild</artifactId></project>",
        ),
    ]);
    let (modules, edges) =
        super::super::build::facts(&input, crate::domain::GraphVersion::ZERO).unwrap();
    super::super::persistence::persist(&connection, "scope", &modules, &edges).unwrap();
    let parent_impacts = downstream(
        &connection,
        "scope",
        &BTreeSet::from(["parent/pom.xml".into()]),
        &[],
    )
    .unwrap();
    assert_eq!(parent_impacts.len(), 2);
    assert_eq!(parent_impacts[0].chain, "child/pom.xml -> parent/pom.xml");
    assert_eq!(
        parent_impacts[1].chain,
        "grandchild/pom.xml -> child/pom.xml -> parent/pom.xml"
    );
    assert_eq!(
        downstream(
            &connection,
            "scope",
            &BTreeSet::from(["bom/pom.xml".into()]),
            &[]
        )
        .unwrap()
        .len(),
        3
    );
    assert!(
        downstream(
            &connection,
            "scope",
            &BTreeSet::from(["grandchild/pom.xml".into()]),
            &[]
        )
        .unwrap()
        .is_empty()
    );
    connection
        .execute(
            "UPDATE maven_reactor_edges SET profile = 'opt-in' WHERE dependency_scope = 'import'",
            [],
        )
        .unwrap();
    assert!(
        downstream(
            &connection,
            "scope",
            &BTreeSet::from(["bom/pom.xml".into()]),
            &[]
        )
        .unwrap()
        .is_empty()
    );
}
