use super::*;

#[test]
fn initial_map_routes_the_repository_software_model() {
    let map = KnowledgeMap::initial("now".to_owned());

    let route = map
        .routes
        .iter()
        .find(|route| route.topic == "software-model")
        .expect("software-model route should exist");
    let source = map
        .sources
        .iter()
        .find(|source| source.id == "repository-software-model")
        .expect("repository software-model source should exist");

    assert_eq!(route.source_order, ["repository-software-model"]);
    assert_eq!(source.kind, KnowledgeMapSourceKind::Repo);
    assert_eq!(source.uri, ".");
    assert_eq!(source.source_scope.as_deref(), Some("repo"));
    map.validate().expect("initial map should validate");
}

#[test]
fn initial_map_routes_the_repository_business_glossary() {
    let map = KnowledgeMap::initial("now".to_owned());
    let route = map
        .routes
        .iter()
        .find(|route| route.topic == "business-knowledge")
        .unwrap();
    let source = map
        .sources
        .iter()
        .find(|source| source.id == "repository-business-glossary")
        .unwrap();

    assert_eq!(route.source_order, ["repository-business-glossary"]);
    assert_eq!(source.kind, KnowledgeMapSourceKind::File);
    assert_eq!(source.uri, "knowledge/glossary/business-glossary.yaml");
    assert_eq!(source.source_scope.as_deref(), Some("repo"));
}

#[test]
fn business_route_upgrade_is_idempotent_and_rejects_reserved_drift() {
    let mut map = KnowledgeMap::initial("now".to_owned());
    map.remove_source("repository-business-glossary").unwrap();
    assert!(map.ensure_business_knowledge_route().unwrap());
    assert!(!map.ensure_business_knowledge_route().unwrap());

    map.sources
        .iter_mut()
        .find(|source| source.id == "repository-business-glossary")
        .unwrap()
        .uri = "other.yaml".to_owned();
    assert!(
        map.validate()
            .unwrap_err()
            .to_string()
            .contains("reserved source")
    );
}

#[test]
fn software_model_route_upgrade_is_idempotent() {
    let mut map = KnowledgeMap::initial("now".to_owned());
    map.remove_source("repository-software-model")
        .expect("legacy fixture should remove the new default");

    assert!(
        map.ensure_software_model_route()
            .expect("legacy map should upgrade")
    );
    assert!(
        !map.ensure_software_model_route()
            .expect("upgraded map should remain valid")
    );
}

#[test]
fn rejects_conflicting_reserved_software_model_source() {
    let mut map = KnowledgeMap::initial("now".to_owned());
    map.sources
        .iter_mut()
        .find(|source| source.id == "repository-software-model")
        .expect("reserved source should exist")
        .uri = "docs/generated-model.yaml".to_owned();

    let error = map.validate().expect_err("reserved source must not drift");

    assert!(error.to_string().contains("reserved source"));
    assert!(error.to_string().contains("uri '.'"));
}

#[test]
fn adds_source_and_route() {
    let mut map = KnowledgeMap::initial("now".to_owned());
    map.add_source(
        KnowledgeMapSource::new(
            "build-cargo".to_owned(),
            "build".to_owned(),
            KnowledgeMapSourceKind::Config,
            "Cargo.toml".to_owned(),
            Some("repo".to_owned()),
            None,
        )
        .expect("source should parse"),
    )
    .expect("source should add");

    assert!(map.topics.iter().any(|topic| topic.id == "build"));
    assert_eq!(
        map.routes
            .iter()
            .find(|route| route.topic == "build")
            .unwrap()
            .source_order,
        ["build-cargo"]
    );
    map.validate().expect("map should validate");
}

#[test]
fn keeps_multiple_sources_under_one_topic() {
    let mut map = KnowledgeMap::initial("now".to_owned());
    for (id, uri) in [
        (
            "cli-reference",
            "docs/zh/01-user-guide/03-cli-command-reference.md",
        ),
        (
            "cli-skill",
            "skills/relay-knowledge-cli/references/knowledge-map-workflows.md",
        ),
    ] {
        map.add_source(
            KnowledgeMapSource::new(
                id.to_owned(),
                "cli".to_owned(),
                KnowledgeMapSourceKind::Doc,
                uri.to_owned(),
                Some("docs".to_owned()),
                None,
            )
            .expect("source should parse"),
        )
        .expect("source should add");
    }

    assert_eq!(
        map.routes
            .iter()
            .find(|route| route.topic == "cli")
            .unwrap()
            .source_order,
        ["cli-reference".to_owned(), "cli-skill".to_owned()]
    );
    assert_eq!(
        map.sources
            .iter()
            .filter(|source| source.topic == "cli")
            .count(),
        2
    );
}

#[test]
fn moving_source_prunes_old_topic_route() {
    let mut map = KnowledgeMap::initial("now".to_owned());
    map.add_source(
        KnowledgeMapSource::new(
            "shared-doc".to_owned(),
            "build".to_owned(),
            KnowledgeMapSourceKind::Doc,
            "docs/build.md".to_owned(),
            None,
            None,
        )
        .expect("source should parse"),
    )
    .expect("source should add");

    map.update_source(KnowledgeMapChange {
        id: "shared-doc".to_owned(),
        topic: Some("cli".to_owned()),
        kind: None,
        uri: None,
        source_scope: None,
        description: None,
    })
    .expect("source should move");

    assert!(
        map.routes
            .iter()
            .find(|route| route.topic == "build")
            .is_none_or(|route| route.source_order.is_empty())
    );
    assert_eq!(
        map.routes
            .iter()
            .find(|route| route.topic == "cli")
            .expect("new route should exist")
            .source_order,
        ["shared-doc".to_owned()]
    );
}

#[test]
fn rejects_duplicate_sources_and_bad_routes() {
    let mut map = KnowledgeMap::initial("now".to_owned());
    let source = KnowledgeMapSource::new(
        "docs".to_owned(),
        "architecture".to_owned(),
        KnowledgeMapSourceKind::Doc,
        "docs/README.md".to_owned(),
        None,
        None,
    )
    .expect("source should parse");
    map.add_source(source.clone())
        .expect("first add should work");
    assert!(map.add_source(source).is_err());

    map.routes
        .iter_mut()
        .find(|route| route.topic == "architecture")
        .unwrap()
        .source_order
        .push("missing".to_owned());
    assert!(map.validate().is_err());
}

#[test]
fn rejects_duplicate_route_topics() {
    let mut map = routed_map();
    map.routes.push(KnowledgeMapRoute {
        topic: "architecture".to_owned(),
        source_order: Vec::new(),
        fallback: None,
    });

    let error = map.validate().expect_err("duplicate route should fail");

    assert!(error.to_string().contains("route topics must be unique"));
}

#[test]
fn rejects_duplicate_sources_inside_route_order() {
    let mut map = routed_map();
    map.routes
        .iter_mut()
        .find(|route| route.topic == "architecture")
        .unwrap()
        .source_order
        .push("docs".to_owned());

    let error = map
        .validate()
        .expect_err("duplicate route source should fail");

    assert!(error.to_string().contains("repeats source 'docs'"));
}

#[test]
fn rejects_unrouted_sources() {
    let mut map = routed_map();
    map.sources.push(
        KnowledgeMapSource::new(
            "unrouted".to_owned(),
            "architecture".to_owned(),
            KnowledgeMapSourceKind::Doc,
            "docs/unrouted.md".to_owned(),
            None,
            None,
        )
        .expect("source should parse"),
    );

    let error = map.validate().expect_err("unrouted source should fail");

    assert!(
        error
            .to_string()
            .contains("source 'unrouted' is not routed")
    );
}

#[test]
fn rejects_invalid_history_contracts() {
    let mut map = routed_map();
    map.history[0].summary.clear();
    assert!(map.validate().is_err());

    let mut map = routed_map();
    map.history.push(KnowledgeMapHistoryEntry {
        version: 3,
        action: "source.add".to_owned(),
        actor: "cli".to_owned(),
        summary: "Skipped a version.".to_owned(),
    });
    map.map_version = 3;
    let error = map.validate().expect_err("skipped history should fail");
    assert!(error.to_string().contains("contiguous"));

    let mut map = routed_map();
    map.map_version = 2;
    let error = map
        .validate()
        .expect_err("mismatched map version should fail");
    assert!(error.to_string().contains("must match map_version 2"));
}

fn routed_map() -> KnowledgeMap {
    let mut map = KnowledgeMap::initial("now".to_owned());
    map.add_source(
        KnowledgeMapSource::new(
            "docs".to_owned(),
            "architecture".to_owned(),
            KnowledgeMapSourceKind::Doc,
            "docs/README.md".to_owned(),
            None,
            None,
        )
        .expect("source should parse"),
    )
    .expect("source should add");
    map
}
